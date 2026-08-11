use std::{
    collections::HashSet,
    env,
    io::{Error, ErrorKind},
    path::Path,
    sync::Arc,
};

mod admin;
mod auth;
mod mcp;
mod quotes;
mod schema;

use askama::Template;
use axum::{
    Router,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::VARY},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use tower_http::services::ServeDir;

#[derive(Debug, Deserialize)]
struct ProjectsFile {
    project: Vec<Project>,
}

#[derive(Debug, Deserialize)]
struct Project {
    slug: String,
    title: String,
    category: String,
    year: String,
    summary: String,
    image: String,
    alt: String,
    href: Option<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    projects: &'a [Project],
    account: Option<auth::AccountView>,
    auth_error: bool,
    auth_cancelled: bool,
    production: bool,
}

#[derive(Template)]
#[template(path = "quotes.html")]
struct QuotesTemplate {
    account: Option<auth::AccountView>,
    quotes: Vec<quotes::QuoteListItem>,
}

#[derive(Template)]
#[template(path = "quote.html")]
struct QuoteTemplate {
    account: Option<auth::AccountView>,
    quote: quotes::QuoteDocument,
}

#[derive(Clone)]
struct AppState {
    projects: Arc<Vec<Project>>,
    auth: auth::Auth,
}

#[derive(Default, Deserialize)]
struct IndexQuery {
    auth: Option<String>,
}

#[derive(Default, Deserialize)]
struct LoginQuery {
    next: Option<String>,
}

enum PageError {
    Server,
    NotFound,
    Login { path: String, htmx: bool },
}

impl IntoResponse for PageError {
    fn into_response(self) -> Response {
        match self {
            Self::Server => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::Login { path, htmx } => {
                let mut response = if htmx {
                    StatusCode::NO_CONTENT.into_response()
                } else {
                    Redirect::to(&path).into_response()
                };
                response
                    .headers_mut()
                    .insert(VARY, HeaderValue::from_static("HX-Request"));
                if htmx {
                    response.headers_mut().insert(
                        "hx-redirect",
                        HeaderValue::from_str(&path).expect("valid internal login path"),
                    );
                }
                response
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenvy::dotenv().ok();
    let state = Arc::new(AppState {
        projects: Arc::new(load_projects()?),
        auth: auth::Auth::from_env()?,
    });
    let mcp_token = env::var("MCP_TOKEN")?;
    if mcp_token.len() < 32 || !mcp_token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "MCP_TOKEN must contain at least 32 non-whitespace ASCII characters",
        )
        .into());
    }
    let mcp = mcp::router(state.auth.clone(), mcp_token);

    let app = Router::new()
        .route("/", get(index))
        .route("/auth/discord", get(discord_login))
        .route("/auth/discord/callback", get(discord_callback))
        .route("/logout", post(logout))
        .route("/quotes", get(quotes_index))
        .route("/quotes/{id}", get(quote_detail))
        .merge(admin::router())
        .merge(mcp)
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    #[cfg(debug_assertions)]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let address = env::var("ADDRESS").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("Quiet Signal is running at http://{address}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<IndexQuery>,
) -> Result<Html<String>, StatusCode> {
    let account = match state.auth.account(&jar).await {
        Ok(account) => account,
        Err(error) => {
            eprintln!("Session lookup failed: {error}");
            None
        }
    };
    let html = IndexTemplate {
        projects: &state.projects,
        account,
        auth_error: query.auth.as_deref() == Some("error"),
        auth_cancelled: query.auth.as_deref() == Some("cancelled"),
        production: !cfg!(debug_assertions),
    }
    .render()
    .map_err(|error| {
        eprintln!("Template render failed: {error}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Html(html))
}

async fn discord_login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> (CookieJar, Redirect) {
    state.auth.begin_login(jar, query.next.as_deref())
}

async fn discord_callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<auth::CallbackQuery>,
) -> (CookieJar, Redirect) {
    state.auth.finish_login(jar, query).await
}

async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> (CookieJar, Redirect) {
    let jar = state.auth.logout(jar).await;
    (jar, Redirect::to("/"))
}

async fn quotes_index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Response, PageError> {
    let account = require_account(&state, &jar, "/quotes", is_htmx(&headers)).await?;
    let quotes = quotes::list(&state.auth, account.discord_id.clone())
        .await
        .map_err(internal_error)?;
    render(QuotesTemplate {
        account: Some(account),
        quotes,
    })
}

async fn quote_detail(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, PageError> {
    let return_to = format!("/quotes/{id}");
    let account = require_account(&state, &jar, &return_to, is_htmx(&headers)).await?;
    let quote = quotes::detail(&state.auth, account.discord_id.clone(), id)
        .await
        .map_err(internal_error)?
        .ok_or(PageError::NotFound)?;
    render(QuoteTemplate {
        account: Some(account),
        quote,
    })
}

async fn require_account(
    state: &AppState,
    jar: &CookieJar,
    return_to: &str,
    htmx: bool,
) -> Result<auth::AccountView, PageError> {
    match state.auth.account(jar).await.map_err(internal_error)? {
        Some(account) => Ok(account),
        None => Err(PageError::Login {
            path: format!("/auth/discord?next={return_to}"),
            htmx,
        }),
    }
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers
        .get("hx-request")
        .is_some_and(|value| value == "true")
}

fn render(template: impl Template) -> Result<Response, PageError> {
    template
        .render()
        .map(|html| Html(html).into_response())
        .map_err(internal_error)
}

fn internal_error(error: impl std::fmt::Display) -> PageError {
    eprintln!("Request failed: {error}");
    PageError::Server
}

fn load_projects() -> Result<Vec<Project>, Box<dyn std::error::Error + Send + Sync>> {
    let source = std::fs::read_to_string("content/projects.toml")?;
    let projects = toml::from_str::<ProjectsFile>(&source)?.project;
    let mut slugs = HashSet::new();

    for project in &projects {
        if project.slug.trim().is_empty()
            || project.title.trim().is_empty()
            || project.summary.trim().is_empty()
            || project.alt.trim().is_empty()
        {
            return Err(
                Error::new(ErrorKind::InvalidData, "project fields cannot be empty").into(),
            );
        }
        if !slugs.insert(&project.slug) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("duplicate project slug: {}", project.slug),
            )
            .into());
        }

        let asset = project.image.trim_start_matches("/static/");
        if !Path::new("static").join(asset).is_file() {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("missing project image: {}", project.image),
            )
            .into());
        }
    }

    Ok(projects)
}
