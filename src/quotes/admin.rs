use diesel::{
    Connection, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl, insert_into,
};
use serde::Serialize;

use crate::{
    auth::{self, Auth},
    schema::{quote_sections, quotes, users},
};

use super::{
    QuoteResult, date,
    form::{QuoteForm, QuoteFormSection, QuoteInput, SectionInput},
    reference, section_rows,
};

#[derive(Serialize)]
pub struct AdminQuoteListItem {
    pub id: String,
    pub discord_id: String,
    pub reference: String,
    pub title: String,
    pub recipient_name: String,
    pub recipient_avatar: Option<String>,
    pub recipient_initial: String,
    pub is_visible: bool,
    pub total_euros: i64,
    pub min_hours: i32,
    pub max_hours: Option<i32>,
    pub updated_at: String,
}

pub struct RecipientView {
    pub discord_id: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub initial: String,
}

pub async fn list(auth: &Auth) -> QuoteResult<Vec<AdminQuoteListItem>> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<Vec<AdminQuoteListItem>> {
        let mut connection = auth::connection(&pool)?;
        let rows = quotes::table
            .inner_join(users::table)
            .order((quotes::updated_at.desc(), quotes::id.desc()))
            .select((
                quotes::id,
                quotes::title,
                quotes::is_visible,
                quotes::updated_at,
                users::discord_id,
                users::display_name,
                users::avatar_hash,
            ))
            .load::<(String, String, bool, i64, String, String, Option<String>)>(&mut connection)?;

        let mut items = Vec::with_capacity(rows.len());
        // ponytail: one tiny query per quote; group in SQL only if the dashboard grows large.
        for (id, title, is_visible, updated_at, discord_id, display_name, avatar_hash) in rows {
            let estimates = quote_sections::table
                .filter(quote_sections::quote_id.eq(&id))
                .order(quote_sections::position.asc())
                .select((
                    quote_sections::estimate_min_minutes,
                    quote_sections::estimate_max_minutes,
                    quote_sections::price_cents,
                ))
                .load::<(i32, Option<i32>, i64)>(&mut connection)?;
            let name = if display_name == discord_id {
                discord_id.clone()
            } else {
                display_name
            };
            items.push(AdminQuoteListItem {
                reference: reference(&id),
                recipient_avatar: avatar_hash.map(|hash| avatar_url(&discord_id, &hash)),
                recipient_initial: initial(&name),
                recipient_name: name,
                discord_id,
                id,
                title,
                is_visible,
                total_euros: estimates.iter().map(|row| row.2 / 100).sum(),
                min_hours: estimates.iter().map(|row| row.0 / 60).sum(),
                max_hours: estimates
                    .iter()
                    .map(|row| row.1.map(|value| value / 60))
                    .sum(),
                updated_at: date(updated_at),
            });
        }
        Ok(items)
    })
    .await?
}

pub async fn edit_form(auth: &Auth, quote_id: String) -> QuoteResult<Option<QuoteForm>> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<Option<QuoteForm>> {
        let mut connection = auth::connection(&pool)?;
        let quote = quotes::table
            .filter(quotes::id.eq(&quote_id))
            .select((
                quotes::user_id,
                quotes::title,
                quotes::description,
                quotes::is_visible,
            ))
            .first::<(String, String, String, bool)>(&mut connection)
            .optional()?;
        let Some((discord_id, title, description, is_visible)) = quote else {
            return Ok(None);
        };
        let sections = section_rows(&mut connection, &quote_id)?
            .into_iter()
            .map(
                |(title, description, minimum, maximum, price_cents)| QuoteFormSection {
                    title,
                    description,
                    min_hours: hours(minimum),
                    max_hours: maximum.map(hours).unwrap_or_default(),
                    price_euros: (price_cents / 100).to_string(),
                },
            )
            .collect();
        Ok(Some(QuoteForm {
            id: Some(quote_id),
            discord_id,
            title,
            description,
            is_visible,
            sections,
            error: None,
        }))
    })
    .await?
}

pub async fn save(auth: &Auth, mut form: QuoteForm) -> QuoteResult<Result<String, QuoteForm>> {
    let input = match form.validate() {
        Ok(input) => input,
        Err(()) => return Ok(Err(form)),
    };
    let quote_id = form.id.clone();
    let pool = auth.pool();
    let result = tokio::task::spawn_blocking(move || -> QuoteResult<Option<String>> {
        let mut connection = auth::connection(&pool)?;
        connection
            .transaction::<_, diesel::result::Error, _>(|connection| {
                let timestamp = auth::now();
                insert_into(users::table)
                    .values((
                        users::discord_id.eq(&input.discord_id),
                        users::display_name.eq(&input.discord_id),
                        users::avatar_hash.eq(Option::<String>::None),
                        users::created_at.eq(timestamp),
                    ))
                    .on_conflict(users::discord_id)
                    .do_nothing()
                    .execute(connection)?;

                let id = match quote_id {
                    Some(id) => update_quote(connection, &id, &input, timestamp)?.then_some(id),
                    None => Some(
                        insert_into(quotes::table)
                            .values((
                                quotes::user_id.eq(&input.discord_id),
                                quotes::title.eq(&input.title),
                                quotes::description.eq(&input.description),
                                quotes::is_visible.eq(input.is_visible),
                                quotes::created_at.eq(timestamp),
                                quotes::updated_at.eq(timestamp),
                            ))
                            .returning(quotes::id)
                            .get_result(connection)?,
                    ),
                };
                let Some(id) = id else { return Ok(None) };
                insert_sections(connection, &id, &input.sections)?;
                Ok(Some(id))
            })
            .map_err(Into::into)
    })
    .await??;

    Ok(match result {
        Some(id) => Ok(id),
        None => {
            form.error = Some("This quote no longer exists.".into());
            Err(form)
        }
    })
}

pub async fn toggle_visibility(auth: &Auth, quote_id: String) -> QuoteResult<bool> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<bool> {
        let mut connection = auth::connection(&pool)?;
        let visible = quotes::table
            .find(&quote_id)
            .select(quotes::is_visible)
            .first::<bool>(&mut connection)
            .optional()?;
        match visible {
            Some(visible) => {
                diesel::update(quotes::table.find(quote_id))
                    .set((
                        quotes::is_visible.eq(!visible),
                        quotes::updated_at.eq(auth::now()),
                    ))
                    .execute(&mut connection)?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await?
}

pub async fn delete(auth: &Auth, quote_id: String) -> QuoteResult<bool> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<bool> {
        let mut connection = auth::connection(&pool)?;
        Ok(diesel::delete(quotes::table.find(quote_id)).execute(&mut connection)? > 0)
    })
    .await?
}

pub async fn recipient(auth: &Auth, discord_id: String) -> QuoteResult<RecipientView> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<RecipientView> {
        let mut connection = auth::connection(&pool)?;
        let row = users::table
            .find(&discord_id)
            .select((users::display_name, users::avatar_hash))
            .first::<(String, Option<String>)>(&mut connection)
            .optional()?;
        let known = row.filter(|(name, _)| name != &discord_id);
        let display_name = known.as_ref().map(|row| row.0.clone());
        let avatar_url = known.and_then(|row| row.1.map(|hash| avatar_url(&discord_id, &hash)));
        Ok(RecipientView {
            initial: initial(display_name.as_deref().unwrap_or(&discord_id)),
            discord_id,
            display_name,
            avatar_url,
        })
    })
    .await?
}

fn update_quote(
    connection: &mut diesel::SqliteConnection,
    id: &str,
    input: &QuoteInput,
    timestamp: i64,
) -> Result<bool, diesel::result::Error> {
    let changed = diesel::update(quotes::table.find(id))
        .set((
            quotes::user_id.eq(&input.discord_id),
            quotes::title.eq(&input.title),
            quotes::description.eq(&input.description),
            quotes::is_visible.eq(input.is_visible),
            quotes::updated_at.eq(timestamp),
        ))
        .execute(connection)?;
    if changed > 0 {
        diesel::delete(quote_sections::table.filter(quote_sections::quote_id.eq(id)))
            .execute(connection)?;
    }
    Ok(changed > 0)
}

fn insert_sections(
    connection: &mut diesel::SqliteConnection,
    quote_id: &str,
    sections: &[SectionInput],
) -> Result<(), diesel::result::Error> {
    for (position, section) in sections.iter().enumerate() {
        insert_into(quote_sections::table)
            .values((
                quote_sections::quote_id.eq(quote_id),
                quote_sections::position.eq(position as i32),
                quote_sections::title.eq(&section.title),
                quote_sections::description.eq(&section.description),
                quote_sections::estimate_min_minutes.eq(section.min_minutes),
                quote_sections::estimate_max_minutes.eq(section.max_minutes),
                quote_sections::price_cents.eq(section.price_cents),
            ))
            .execute(connection)?;
    }
    Ok(())
}

fn initial(name: &str) -> String {
    name.chars().next().unwrap_or('?').to_uppercase().collect()
}

fn avatar_url(discord_id: &str, hash: &str) -> String {
    format!("https://cdn.discordapp.com/avatars/{discord_id}/{hash}.webp?size=64")
}

fn hours(minutes: i32) -> String {
    if minutes % 60 == 0 {
        (minutes / 60).to_string()
    } else {
        format!("{:.2}", minutes as f64 / 60.0)
            .trim_end_matches('0')
            .to_string()
    }
}
