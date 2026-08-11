use std::sync::Arc;

use axum::{
    Router,
    extract::{Request, State},
    http::{
        StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{AppState, auth::Auth, quotes};

#[derive(Clone)]
struct QuoteMcp {
    auth: Auth,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QuoteId {
    /// UUID of the quote.
    id: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct QuoteWrite {
    /// Discord user ID of the recipient.
    discord_id: String,
    /// Short quote title.
    title: String,
    /// Overall quote description.
    description: String,
    /// Whether the recipient can see the quote.
    is_visible: bool,
    /// Ordered quote sections. At least one is required.
    sections: Vec<QuoteSectionWrite>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
struct QuoteSectionWrite {
    title: String,
    description: String,
    /// Positive whole number of hours.
    min_hours: i32,
    /// Optional maximum; must be at least min_hours.
    max_hours: Option<i32>,
    /// Non-negative whole euro amount.
    price_euros: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QuoteUpdate {
    /// UUID of the quote to update.
    id: String,
    #[serde(flatten)]
    quote: QuoteWrite,
}

#[derive(Serialize)]
struct QuoteRead {
    id: String,
    #[serde(flatten)]
    quote: QuoteWrite,
}

#[tool_router]
impl QuoteMcp {
    fn new(auth: Auth) -> Self {
        Self { auth }
    }

    #[tool(
        description = "List all quotes with their IDs, recipients, visibility, totals and update dates.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn list_quotes(&self) -> Result<CallToolResult, McpError> {
        let quotes = quotes::admin_list(&self.auth).await.map_err(mcp_error)?;
        structured(&quotes)
    }

    #[tool(
        description = "Get one complete quote, including its ordered sections, by UUID.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn get_quote(
        &self,
        Parameters(QuoteId { id }): Parameters<QuoteId>,
    ) -> Result<CallToolResult, McpError> {
        match quotes::edit_form(&self.auth, id).await.map_err(mcp_error)? {
            Some(quote) => structured(&QuoteRead::try_from(quote)?),
            None => Ok(tool_error("Quote not found.")),
        }
    }

    #[tool(
        description = "Create a quote and return its generated UUID. The recipient is created if needed.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_quote(
        &self,
        Parameters(input): Parameters<QuoteWrite>,
    ) -> Result<CallToolResult, McpError> {
        match quotes::save(&self.auth, input.into_form(None))
            .await
            .map_err(mcp_error)?
        {
            Ok(id) => Ok(CallToolResult::structured(json!({ "id": id }))),
            Err(form) => Ok(tool_error(
                form.error.as_deref().unwrap_or("Invalid quote."),
            )),
        }
    }

    #[tool(
        description = "Update an existing quote and replace all of its sections.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn update_quote(
        &self,
        Parameters(input): Parameters<QuoteUpdate>,
    ) -> Result<CallToolResult, McpError> {
        let id = input.id;
        match quotes::save(&self.auth, input.quote.into_form(Some(id.clone())))
            .await
            .map_err(mcp_error)?
        {
            Ok(_) => Ok(CallToolResult::structured(json!({ "id": id }))),
            Err(form) => Ok(tool_error(
                form.error.as_deref().unwrap_or("Invalid quote."),
            )),
        }
    }
}

#[tool_handler]
impl ServerHandler for QuoteMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("aboutme-quotes", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Manage website quotes. Read with list_quotes/get_quote. Create or update only when explicitly requested; updates replace every section."
                    .to_owned(),
            )
    }
}

impl QuoteWrite {
    fn into_form(self, id: Option<String>) -> quotes::QuoteForm {
        quotes::QuoteForm {
            id,
            discord_id: self.discord_id,
            title: self.title,
            description: self.description,
            is_visible: self.is_visible,
            sections: self
                .sections
                .into_iter()
                .map(|section| quotes::QuoteFormSection {
                    title: section.title,
                    description: section.description,
                    min_hours: section.min_hours.to_string(),
                    max_hours: section
                        .max_hours
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    price_euros: section.price_euros.to_string(),
                })
                .collect(),
            error: None,
        }
    }
}

impl TryFrom<quotes::QuoteForm> for QuoteRead {
    type Error = McpError;

    fn try_from(form: quotes::QuoteForm) -> Result<Self, Self::Error> {
        let id = form
            .id
            .ok_or_else(|| McpError::internal_error("Quote ID is missing.", None))?;
        let sections = form
            .sections
            .into_iter()
            .map(|section| {
                Ok(QuoteSectionWrite {
                    title: section.title,
                    description: section.description,
                    min_hours: section.min_hours.parse().map_err(mcp_error)?,
                    max_hours: if section.max_hours.is_empty() {
                        None
                    } else {
                        Some(section.max_hours.parse().map_err(mcp_error)?)
                    },
                    price_euros: section.price_euros.parse().map_err(mcp_error)?,
                })
            })
            .collect::<Result<_, McpError>>()?;

        Ok(Self {
            id,
            quote: QuoteWrite {
                discord_id: form.discord_id,
                title: form.title,
                description: form.description,
                is_visible: form.is_visible,
                sections,
            },
        })
    }
}

pub(crate) fn router(auth: Auth, token: String) -> Router<Arc<AppState>> {
    let token_hash: Arc<[u8]> = Arc::from(Sha256::digest(token).as_slice());
    let service = StreamableHttpService::new(
        move || Ok(QuoteMcp::new(auth.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_allowed_hosts(["frig.dev"]).with_json_response(true),
    );

    Router::new()
        .nest_service("/mcp", service)
        .layer(from_fn_with_state(token_hash, authorize))
}

async fn authorize(State(expected): State<Arc<[u8]>>, request: Request, next: Next) -> Response {
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| Sha256::digest(token).as_slice() == expected.as_ref());

    if authorized {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, [(WWW_AUTHENTICATE, "Bearer")]).into_response()
    }
}

fn structured(value: &impl serde::Serialize) -> Result<CallToolResult, McpError> {
    serde_json::to_value(value)
        .map(CallToolResult::structured)
        .map_err(mcp_error)
}

fn tool_error(message: &str) -> CallToolResult {
    CallToolResult::structured_error(json!({ "error": message }))
}

fn mcp_error(error: impl std::fmt::Display) -> McpError {
    eprintln!("MCP request failed: {error}");
    McpError::internal_error("MCP request failed.", None)
}
