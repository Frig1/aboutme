use std::{error::Error, io::Error as IoError};

use diesel::{ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use time::OffsetDateTime;

use crate::{
    auth::{self, Auth},
    schema::{quote_sections, quotes},
};

mod admin;
mod form;

pub use admin::{
    AdminQuoteListItem, RecipientView, delete, edit_form, list as admin_list, recipient, save,
    toggle_visibility,
};
pub use form::{QuoteForm, QuoteFormSection};

type QuoteResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
type SectionRow = (String, String, i32, Option<i32>, i64);

pub struct QuoteListItem {
    pub id: String,
    pub reference: String,
    pub title: String,
    pub description: String,
    pub updated_at: String,
}

pub struct QuoteDocument {
    pub reference: String,
    pub title: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
    pub sections: Vec<QuoteSection>,
    pub total: String,
}

pub struct QuoteSection {
    pub number: usize,
    pub title: String,
    pub description: String,
    pub estimate: String,
    pub price: String,
}

pub async fn list(auth: &Auth, user_id: String) -> QuoteResult<Vec<QuoteListItem>> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<Vec<QuoteListItem>> {
        let mut connection = auth::connection(&pool)?;
        let rows = quotes::table
            .filter(quotes::user_id.eq(user_id))
            .filter(quotes::is_visible.eq(true))
            .order((quotes::updated_at.desc(), quotes::id.desc()))
            .select((
                quotes::id,
                quotes::title,
                quotes::description,
                quotes::updated_at,
            ))
            .load::<(String, String, String, i64)>(&mut connection)?;

        Ok(rows
            .into_iter()
            .map(|(id, title, description, updated_at)| QuoteListItem {
                reference: reference(&id),
                id,
                title,
                description,
                updated_at: date(updated_at),
            })
            .collect())
    })
    .await?
}

pub async fn detail(
    auth: &Auth,
    user_id: String,
    quote_id: String,
) -> QuoteResult<Option<QuoteDocument>> {
    let pool = auth.pool();
    tokio::task::spawn_blocking(move || -> QuoteResult<Option<QuoteDocument>> {
        let mut connection = auth::connection(&pool)?;
        let quote = quotes::table
            .filter(quotes::id.eq(&quote_id))
            .filter(quotes::user_id.eq(user_id))
            .filter(quotes::is_visible.eq(true))
            .select((
                quotes::title,
                quotes::description,
                quotes::created_at,
                quotes::updated_at,
            ))
            .first::<(String, String, i64, i64)>(&mut connection)
            .optional()?;
        let Some((title, description, created_at, updated_at)) = quote else {
            return Ok(None);
        };
        let rows = section_rows(&mut connection, &quote_id)?;
        let total_cents = rows.iter().try_fold(0_i64, |total, row| {
            total
                .checked_add(row.4)
                .ok_or_else(|| IoError::other("quote total overflow"))
        })?;
        let sections = rows
            .into_iter()
            .enumerate()
            .map(
                |(index, (title, description, minimum, maximum, price_cents))| QuoteSection {
                    number: index + 1,
                    title,
                    description,
                    estimate: estimate(minimum, maximum),
                    price: price(price_cents),
                },
            )
            .collect();

        Ok(Some(QuoteDocument {
            reference: reference(&quote_id),
            title,
            description,
            created_at: date(created_at),
            updated_at: date(updated_at),
            sections,
            total: price(total_cents),
        }))
    })
    .await?
}

fn section_rows(
    connection: &mut diesel::SqliteConnection,
    quote_id: &str,
) -> Result<Vec<SectionRow>, diesel::result::Error> {
    quote_sections::table
        .filter(quote_sections::quote_id.eq(quote_id))
        .order(quote_sections::position.asc())
        .select((
            quote_sections::title,
            quote_sections::description,
            quote_sections::estimate_min_minutes,
            quote_sections::estimate_max_minutes,
            quote_sections::price_cents,
        ))
        .load(connection)
}

fn reference(id: &str) -> String {
    format!("QUO-{}", id.get(..8).unwrap_or(id).to_uppercase())
}

fn date(timestamp: i64) -> String {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .map(|value| {
            let date = value.date();
            format!(
                "{:02}.{:02}.{}",
                date.day(),
                date.month() as u8,
                date.year()
            )
        })
        .unwrap_or_else(|_| "—".into())
}

fn estimate(minimum: i32, maximum: Option<i32>) -> String {
    match maximum {
        Some(maximum) if maximum > minimum => {
            format!("{}–{}", duration(minimum), duration(maximum))
        }
        Some(_) => duration(minimum),
        None => format!("from {}", duration(minimum)),
    }
}

fn duration(minutes: i32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

fn price(cents: i64) -> String {
    format!("€ {},{:02}", cents / 100, cents % 100)
}
