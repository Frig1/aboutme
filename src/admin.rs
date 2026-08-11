use std::{collections::HashMap, sync::Arc};

use askama::Template;
use axum::{
    Router,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::{AppState, PageError, auth, internal_error, is_htmx, quotes, render, require_account};

#[derive(Template)]
#[template(path = "admin/quotes.html")]
struct QuotesTemplate {
    account: Option<auth::AccountView>,
    quotes: Vec<quotes::AdminQuoteListItem>,
}

#[derive(Template)]
#[template(path = "admin/quote-form.html")]
struct QuoteFormTemplate {
    account: Option<auth::AccountView>,
    form: quotes::QuoteForm,
    recipient: Option<quotes::RecipientView>,
}

#[derive(Template)]
#[template(path = "admin/recipient.html")]
struct RecipientTemplate {
    recipient: quotes::RecipientView,
}

#[derive(Deserialize)]
struct RecipientQuery {
    discord_id: String,
}

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/quotes", get(index))
        .route("/admin/quotes/new", get(new).post(create))
        .route("/admin/quotes/{id}/edit", get(edit).post(update))
        .route("/admin/quotes/{id}/visibility", post(toggle_visibility))
        .route("/admin/quotes/{id}/delete", post(delete))
        .route("/admin/recipient", get(recipient))
}

async fn index(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Response, PageError> {
    let account = require_admin(&state, &jar, "/admin/quotes", is_htmx(&headers)).await?;
    let quotes = quotes::admin_list(&state.auth)
        .await
        .map_err(internal_error)?;
    render(QuotesTemplate {
        account: Some(account),
        quotes,
    })
}

async fn new(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
) -> Result<Response, PageError> {
    let account = require_admin(&state, &jar, "/admin/quotes/new", is_htmx(&headers)).await?;
    render(QuoteFormTemplate {
        account: Some(account),
        form: quotes::QuoteForm::empty(),
        recipient: None,
    })
}

async fn edit(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, PageError> {
    let path = format!("/admin/quotes/{id}/edit");
    let account = require_admin(&state, &jar, &path, is_htmx(&headers)).await?;
    let form = quotes::edit_form(&state.auth, id)
        .await
        .map_err(internal_error)?
        .ok_or(PageError::NotFound)?;
    let recipient = quotes::recipient(&state.auth, form.discord_id.clone())
        .await
        .map_err(internal_error)?;
    render(QuoteFormTemplate {
        account: Some(account),
        form,
        recipient: Some(recipient),
    })
}

async fn create(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(fields): Form<HashMap<String, String>>,
) -> Result<Response, PageError> {
    let account = require_admin(&state, &jar, "/admin/quotes/new", is_htmx(&headers)).await?;
    save(
        &state,
        account,
        quotes::QuoteForm::from_fields(&fields, None),
    )
    .await
}

async fn update(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(fields): Form<HashMap<String, String>>,
) -> Result<Response, PageError> {
    let path = format!("/admin/quotes/{id}/edit");
    let account = require_admin(&state, &jar, &path, is_htmx(&headers)).await?;
    save(
        &state,
        account,
        quotes::QuoteForm::from_fields(&fields, Some(id)),
    )
    .await
}

async fn save(
    state: &AppState,
    account: auth::AccountView,
    form: quotes::QuoteForm,
) -> Result<Response, PageError> {
    match quotes::save(&state.auth, form)
        .await
        .map_err(internal_error)?
    {
        Ok(_) => Ok(Redirect::to("/admin/quotes").into_response()),
        Err(form) => {
            let recipient = if form.discord_id.is_empty() {
                None
            } else {
                Some(
                    quotes::recipient(&state.auth, form.discord_id.clone())
                        .await
                        .map_err(internal_error)?,
                )
            };
            let mut response = render(QuoteFormTemplate {
                account: Some(account),
                form,
                recipient,
            })?;
            *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
            Ok(response)
        }
    }
}

async fn toggle_visibility(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, PageError> {
    require_admin(&state, &jar, "/admin/quotes", is_htmx(&headers)).await?;
    if !quotes::toggle_visibility(&state.auth, id)
        .await
        .map_err(internal_error)?
    {
        return Err(PageError::NotFound);
    }
    Ok(Redirect::to("/admin/quotes").into_response())
}

async fn delete(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, PageError> {
    require_admin(&state, &jar, "/admin/quotes", is_htmx(&headers)).await?;
    if !quotes::delete(&state.auth, id)
        .await
        .map_err(internal_error)?
    {
        return Err(PageError::NotFound);
    }
    Ok(Redirect::to("/admin/quotes").into_response())
}

async fn recipient(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(query): Query<RecipientQuery>,
) -> Result<Response, PageError> {
    require_admin(&state, &jar, "/admin/quotes/new", is_htmx(&headers)).await?;
    render(RecipientTemplate {
        recipient: quotes::recipient(&state.auth, query.discord_id)
            .await
            .map_err(internal_error)?,
    })
}

async fn require_admin(
    state: &AppState,
    jar: &CookieJar,
    return_to: &str,
    htmx: bool,
) -> Result<auth::AccountView, PageError> {
    let account = require_account(state, jar, return_to, htmx).await?;
    account
        .is_admin
        .then_some(account)
        .ok_or(PageError::NotFound)
}
