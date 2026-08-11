use std::{
    env,
    error::Error,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::response::Redirect;
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use diesel::{
    Connection, OptionalExtension, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    prelude::*,
    r2d2::{ConnectionManager, Pool},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::Duration;

use crate::schema::{sessions, users};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();
const SESSION_COOKIE: &str = "aboutme_session";
const OAUTH_STATE_COOKIE: &str = "discord_oauth_state";
const OAUTH_VERIFIER_COOKIE: &str = "discord_oauth_verifier";
const RETURN_TO_COOKIE: &str = "discord_return_to";
const SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;

pub(crate) type DbPool = Pool<ConnectionManager<SqliteConnection>>;
type AuthResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone)]
pub struct Auth {
    pool: DbPool,
    admin_discord_id: String,
    client_id: String,
    client_secret: String,
    callback_url: RedirectUrl,
    secure_cookies: bool,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AccountView {
    pub discord_id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub initial: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = users)]
struct NewUser<'a> {
    discord_id: &'a str,
    display_name: &'a str,
    avatar_hash: Option<&'a str>,
    created_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = sessions)]
struct NewSession<'a> {
    token_hash: &'a str,
    user_id: &'a str,
    created_at: i64,
    expires_at: i64,
}

impl Auth {
    pub fn from_env() -> AuthResult<Self> {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "data/db.sqlite".into());
        if let Some(parent) = Path::new(&database_url)
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }

        let mut connection = SqliteConnection::establish(&database_url)?;
        connection.batch_execute("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        connection.run_pending_migrations(MIGRATIONS)?;
        diesel::delete(sessions::table.filter(sessions::expires_at.le(now())))
            .execute(&mut connection)?;

        let manager = ConnectionManager::<SqliteConnection>::new(database_url);
        let pool = Pool::builder().max_size(4).build(manager)?;
        let app_url = env::var("APP_URL")?.trim_end_matches('/').to_owned();

        Ok(Self {
            pool,
            admin_discord_id: env::var("ADMIN_DISCORD_ID")?,
            client_id: env::var("DISCORD_CLIENT_ID")?,
            client_secret: env::var("DISCORD_CLIENT_SECRET")?,
            callback_url: RedirectUrl::new(format!("{app_url}/auth/discord/callback"))?,
            secure_cookies: app_url.starts_with("https://"),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    pub fn begin_login(&self, jar: CookieJar, return_to: Option<&str>) -> (CookieJar, Redirect) {
        let client = self.oauth_client();
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("identify".into()))
            .set_pkce_challenge(challenge)
            .url();

        let mut jar = jar
            .add(self.cookie(
                OAUTH_STATE_COOKIE,
                state.secret().clone(),
                Duration::minutes(10),
            ))
            .add(self.cookie(
                OAUTH_VERIFIER_COOKIE,
                verifier.secret().clone(),
                Duration::minutes(10),
            ));
        jar = match return_to.filter(|path| safe_return_to(path)) {
            Some(path) => {
                jar.add(self.cookie(RETURN_TO_COOKIE, path.to_owned(), Duration::minutes(10)))
            }
            None => jar.add(self.expired_cookie(RETURN_TO_COOKIE)),
        };

        (jar, Redirect::to(url.as_str()))
    }

    pub async fn finish_login(
        &self,
        jar: CookieJar,
        query: CallbackQuery,
    ) -> (CookieJar, Redirect) {
        let expected_state = jar
            .get(OAUTH_STATE_COOKIE)
            .map(|cookie| cookie.value().to_owned());
        let verifier = jar
            .get(OAUTH_VERIFIER_COOKIE)
            .map(|cookie| cookie.value().to_owned());
        let return_to = jar
            .get(RETURN_TO_COOKIE)
            .map(|cookie| cookie.value().to_owned())
            .filter(|path| safe_return_to(path))
            .unwrap_or_else(|| "/".into());
        let jar = self.clear_login_cookies(jar);

        if query.error.as_deref() == Some("access_denied") {
            return (jar, Redirect::to("/?auth=cancelled"));
        }

        match self.exchange_login(query, expected_state, verifier).await {
            Ok(token) => {
                let jar = jar.add(self.cookie(SESSION_COOKIE, token, Duration::days(30)));
                (jar, Redirect::to(&return_to))
            }
            Err(error) => {
                eprintln!("Discord login failed: {error}");
                (jar, Redirect::to("/?auth=error"))
            }
        }
    }

    pub async fn account(&self, jar: &CookieJar) -> AuthResult<Option<AccountView>> {
        let Some(token) = jar
            .get(SESSION_COOKIE)
            .map(|cookie| cookie.value().to_owned())
        else {
            return Ok(None);
        };
        let token_hash = hash(&token);
        let pool = self.pool.clone();
        let admin_discord_id = self.admin_discord_id.clone();

        tokio::task::spawn_blocking(move || -> AuthResult<Option<AccountView>> {
            let mut connection = connection(&pool)?;
            let row = sessions::table
                .inner_join(users::table)
                .filter(sessions::token_hash.eq(token_hash))
                .filter(sessions::expires_at.gt(now()))
                .select((users::discord_id, users::display_name, users::avatar_hash))
                .first::<(String, String, Option<String>)>(&mut connection)
                .optional()?;

            Ok(row.map(|(id, display_name, avatar)| AccountView {
                discord_id: id.clone(),
                initial: display_name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .collect(),
                avatar_url: avatar.map(|hash| {
                    format!("https://cdn.discordapp.com/avatars/{id}/{hash}.webp?size=64")
                }),
                is_admin: id == admin_discord_id,
                display_name,
            }))
        })
        .await?
    }

    pub async fn logout(&self, jar: CookieJar) -> CookieJar {
        if let Some(token) = jar.get(SESSION_COOKIE) {
            let token_hash = hash(token.value());
            let pool = self.pool.clone();
            let result = tokio::task::spawn_blocking(move || -> AuthResult<()> {
                let mut connection = connection(&pool)?;
                diesel::delete(sessions::table.find(token_hash)).execute(&mut connection)?;
                Ok(())
            })
            .await;
            if let Err(error) = result.unwrap_or_else(|error| Err(error.into())) {
                eprintln!("Session logout failed: {error}");
            }
        }

        jar.add(self.expired_cookie(SESSION_COOKIE))
    }

    async fn exchange_login(
        &self,
        query: CallbackQuery,
        expected_state: Option<String>,
        verifier: Option<String>,
    ) -> AuthResult<String> {
        let state = query.state.ok_or("missing OAuth state")?;
        if expected_state.as_deref() != Some(&state) {
            return Err("invalid OAuth state".into());
        }

        let token = self
            .oauth_client()
            .exchange_code(AuthorizationCode::new(
                query.code.ok_or("missing OAuth code")?,
            ))
            .set_pkce_verifier(PkceCodeVerifier::new(
                verifier.ok_or("missing PKCE verifier")?,
            ))
            .request_async(&self.http)
            .await?;

        let discord_user = self
            .http
            .get("https://discord.com/api/v10/users/@me")
            .bearer_auth(token.access_token().secret())
            .send()
            .await?
            .error_for_status()?
            .json::<DiscordUser>()
            .await?;
        drop(token);

        let display_name = discord_user
            .global_name
            .clone()
            .unwrap_or_else(|| discord_user.username.clone());
        let session_token = CsrfToken::new_random().secret().clone();
        let session_hash = hash(&session_token);
        let pool = self.pool.clone();
        let timestamp = now();

        tokio::task::spawn_blocking(move || -> AuthResult<()> {
            let mut connection = connection(&pool)?;
            connection.transaction::<_, diesel::result::Error, _>(|connection| {
                diesel::insert_into(users::table)
                    .values(NewUser {
                        discord_id: &discord_user.id,
                        display_name: &display_name,
                        avatar_hash: discord_user.avatar.as_deref(),
                        created_at: timestamp,
                    })
                    .on_conflict(users::discord_id)
                    .do_update()
                    .set((
                        users::display_name.eq(&display_name),
                        users::avatar_hash.eq(discord_user.avatar.as_deref()),
                    ))
                    .execute(connection)?;

                diesel::insert_into(sessions::table)
                    .values(NewSession {
                        token_hash: &session_hash,
                        user_id: &discord_user.id,
                        created_at: timestamp,
                        expires_at: timestamp + SESSION_SECONDS,
                    })
                    .execute(connection)?;

                diesel::delete(sessions::table.filter(sessions::expires_at.le(timestamp)))
                    .execute(connection)?;
                Ok(())
            })?;
            Ok(())
        })
        .await??;

        Ok(session_token)
    }

    fn oauth_client(
        &self,
    ) -> BasicClient<
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    > {
        BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(
                AuthUrl::new("https://discord.com/oauth2/authorize".into())
                    .expect("valid Discord auth URL"),
            )
            .set_token_uri(
                TokenUrl::new("https://discord.com/api/oauth2/token".into())
                    .expect("valid Discord token URL"),
            )
            .set_redirect_uri(self.callback_url.clone())
    }

    fn cookie(&self, name: &'static str, value: String, max_age: Duration) -> Cookie<'static> {
        Cookie::build((name, value))
            .http_only(true)
            .same_site(SameSite::Lax)
            .secure(self.secure_cookies)
            .path("/")
            .max_age(max_age)
            .build()
    }

    fn expired_cookie(&self, name: &'static str) -> Cookie<'static> {
        self.cookie(name, String::new(), Duration::ZERO)
    }

    fn clear_login_cookies(&self, jar: CookieJar) -> CookieJar {
        jar.add(self.expired_cookie(OAUTH_STATE_COOKIE))
            .add(self.expired_cookie(OAUTH_VERIFIER_COOKIE))
            .add(self.expired_cookie(RETURN_TO_COOKIE))
    }

    pub(crate) fn pool(&self) -> DbPool {
        self.pool.clone()
    }
}

pub(crate) fn connection(
    pool: &DbPool,
) -> AuthResult<diesel::r2d2::PooledConnection<ConnectionManager<SqliteConnection>>> {
    let mut connection = pool.get()?;
    connection.batch_execute("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    Ok(connection)
}

fn safe_return_to(path: &str) -> bool {
    path == "/quotes"
        || path == "/admin/quotes"
        || path == "/admin/quotes/new"
        || path
            .strip_prefix("/admin/quotes/")
            .and_then(|value| value.strip_suffix("/edit"))
            .is_some_and(valid_id)
        || path.strip_prefix("/quotes/").is_some_and(valid_id)
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs() as i64
}
