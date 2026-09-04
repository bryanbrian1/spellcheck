//! A minimal MCP client over Streamable HTTP.
//!
//! Only what a build lookup needs: `initialize`, the `notifications/initialized`
//! handshake, `tools/list`, and `tools/call`. A Streamable HTTP server may
//! answer a POST with either a JSON body or an SSE stream, so both are handled.
//!
//! Nothing here writes to disk. Responses live only as long as the call.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::build_data::error::ProviderError;

/// Protocol revision we negotiate. Servers that speak a different revision
/// answer with their own in the initialize result; we do not downgrade.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";

#[derive(Debug, Default)]
struct Session {
    id: Option<String>,
    initialized: bool,
}

pub struct McpClient {
    http: reqwest::Client,
    endpoint: String,
    provider: &'static str,
    client_name: String,
    client_version: String,
    session: Mutex<Session>,
    next_id: AtomicU64,
}

impl McpClient {
    pub fn new(
        endpoint: impl Into<String>,
        timeout: Duration,
        provider: &'static str,
    ) -> Result<McpClient, ProviderError> {
        let client_version = env!("CARGO_PKG_VERSION").to_string();
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(format!("leaguechecker/{client_version}"))
            .build()
            .map_err(|error| ProviderError::transport(provider, error))?;

        Ok(McpClient {
            http,
            endpoint: endpoint.into(),
            provider,
            client_name: "leaguechecker".to_string(),
            client_version,
            session: Mutex::new(Session::default()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Call a tool and return its payload.
    ///
    /// Retries once if the server has forgotten our session, which is normal
    /// after the app has sat idle between games.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, ProviderError> {
        match self.try_call_tool(name, &arguments).await {
            Err(ProviderError::Protocol { detail, .. }) if detail == SESSION_EXPIRED => {
                self.reset_session().await;
                self.try_call_tool(name, &arguments).await
            }
            other => other,
        }
    }

    /// The tool catalogue, including each tool's input schema. Useful for
    /// confirming argument names against the live server.
    pub async fn list_tools(&self) -> Result<Value, ProviderError> {
        self.ensure_session().await?;
        let result = self.request("tools/list", json!({})).await?;
        Ok(result)
    }

    async fn try_call_tool(&self, name: &str, arguments: &Value) -> Result<Value, ProviderError> {
        self.ensure_session().await?;

        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;

        // A tool that fails reports it inside a successful JSON-RPC result.
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(ProviderError::Upstream {
                provider: self.provider,
                detail: text_content(&result)
                    .unwrap_or_else(|| format!("tool {name} reported an error")),
            });
        }

        tool_payload(&result).ok_or_else(|| {
            ProviderError::protocol(
                self.provider,
                format!("tool {name} returned no readable content"),
            )
        })
    }

    async fn reset_session(&self) {
        let mut session = self.session.lock().await;
        *session = Session::default();
    }

    /// Initialize once, lazily. The lock is held across the handshake so
    /// concurrent lookups in champ select cannot open two sessions.
    async fn ensure_session(&self) -> Result<(), ProviderError> {
        let mut session = self.session.lock().await;
        if session.initialized {
            return Ok(());
        }

        let body = self.envelope(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": self.client_name, "version": self.client_version },
            }),
        );

        let (outcome, session_id) = self.post(&body, None).await?;
        match outcome {
            Outcome::Message(message) => {
                self.result_of(message)?;
            }
            Outcome::Accepted => {
                return Err(ProviderError::protocol(
                    self.provider,
                    "server accepted initialize without answering it",
                ))
            }
            Outcome::SessionExpired => {
                return Err(ProviderError::protocol(
                    self.provider,
                    "server rejected the session during initialize",
                ))
            }
        }

        session.id = session_id;

        // Fire-and-forget notification; the server answers 202 with no body.
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });
        self.post(&notification, session.id.as_deref()).await?;

        session.initialized = true;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, ProviderError> {
        let session_id = self.session.lock().await.id.clone();
        let body = self.envelope(method, params);

        let (outcome, _) = self.post(&body, session_id.as_deref()).await?;
        match outcome {
            Outcome::Message(message) => self.result_of(message),
            Outcome::Accepted => Err(ProviderError::protocol(
                self.provider,
                format!("server accepted {method} without answering it"),
            )),
            Outcome::SessionExpired => Err(ProviderError::protocol(self.provider, SESSION_EXPIRED)),
        }
    }

    fn envelope(&self, method: &str, params: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": self.next_id.fetch_add(1, Ordering::Relaxed),
            "method": method,
            "params": params,
        })
    }

    fn result_of(&self, message: Value) -> Result<Value, ProviderError> {
        if let Some(error) = message.get("error") {
            let detail = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            return Err(ProviderError::Upstream {
                provider: self.provider,
                detail,
            });
        }

        message.get("result").cloned().ok_or_else(|| {
            ProviderError::protocol(self.provider, "response carried neither result nor error")
        })
    }

    async fn post(
        &self,
        body: &Value,
        session_id: Option<&str>,
    ) -> Result<(Outcome, Option<String>), ProviderError> {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header(PROTOCOL_HEADER, PROTOCOL_VERSION)
            .json(body);

        if let Some(id) = session_id {
            request = request.header(SESSION_HEADER, id);
        }

        let response = request
            .send()
            .await
            .map_err(|error| ProviderError::transport(self.provider, error))?;

        let status = response.status();
        let returned_session = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();

        // The server dropped our session; the caller re-initializes and retries.
        if status == reqwest::StatusCode::NOT_FOUND && session_id.is_some() {
            return Ok((Outcome::SessionExpired, returned_session));
        }

        let text = response
            .text()
            .await
            .map_err(|error| ProviderError::transport(self.provider, error))?;

        if status == reqwest::StatusCode::ACCEPTED || text.trim().is_empty() {
            if status.is_success() {
                return Ok((Outcome::Accepted, returned_session));
            }
            return Err(ProviderError::protocol(
                self.provider,
                format!("HTTP {status} with an empty body"),
            ));
        }

        let message = if content_type.contains("text/event-stream") {
            first_rpc_message(&parse_sse(&text))
        } else {
            serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| first_rpc_message(&flatten_batch(value)))
        };

        match message {
            Some(message) => Ok((Outcome::Message(message), returned_session)),
            // A non-2xx with an unparseable body is the useful error to show.
            None if !status.is_success() => Err(ProviderError::protocol(
                self.provider,
                format!("HTTP {status}: {}", truncate(&text, 300)),
            )),
            None => Err(ProviderError::protocol(
                self.provider,
                format!("unreadable response: {}", truncate(&text, 300)),
            )),
        }
    }
}

const SESSION_EXPIRED: &str = "session expired";

enum Outcome {
    Message(Value),
    Accepted,
    SessionExpired,
}

/// A JSON-RPC body may be one message or a batch.
fn flatten_batch(value: Value) -> Vec<Value> {
    match value {
        Value::Array(messages) => messages,
        other => vec![other],
    }
}

/// The first message carrying a `result` or an `error`, ignoring the
/// notifications (progress, logging) a server may interleave.
fn first_rpc_message(messages: &[Value]) -> Option<Value> {
    messages
        .iter()
        .find(|message| message.get("result").is_some() || message.get("error").is_some())
        .cloned()
}

/// Pull the JSON payloads out of an SSE stream. Per the SSE grammar, an event
/// ends at a blank line and multiple `data:` lines are joined with newlines.
fn parse_sse(body: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut data = String::new();

    let mut flush = |data: &mut String| {
        if !data.trim().is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                messages.extend(flatten_batch(value));
            }
        }
        data.clear();
    };

    for line in body.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            flush(&mut data);
        } else if let Some(chunk) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(chunk.strip_prefix(' ').unwrap_or(chunk));
        }
        // Other SSE fields (event:, id:, retry:, comments) carry nothing we need.
    }
    flush(&mut data);

    messages
}

/// The payload of a `tools/call` result: structured output when the server
/// provides it, otherwise the text content parsed as JSON.
fn tool_payload(result: &Value) -> Option<Value> {
    if let Some(structured) = result.get("structuredContent") {
        if !structured.is_null() {
            return Some(structured.clone());
        }
    }

    let text = text_content(result)?;
    // Most servers return their JSON as a text block.
    Some(serde_json::from_str::<Value>(text.trim()).unwrap_or(Value::String(text)))
}

/// Concatenated text blocks from a tool result.
fn text_content(result: &Value) -> Option<String> {
    let blocks = result.get("content")?.as_array()?;
    let text: Vec<&str> = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();

    if text.is_empty() {
        None
    } else {
        Some(text.join("\n"))
    }
}

fn truncate(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_an_sse_stream() {
        let body = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\r\n\r\n";
        let messages = parse_sse(body);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["result"]["ok"], true);
    }

    #[test]
    fn joins_multiline_sse_data() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\n data: \"result\":{\"ok\":true}}\n\n";
        // Leading-space `data:` lines are still data lines.
        let messages = parse_sse(body.replace("\n data:", "\ndata:").as_str());
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn skips_notifications_before_the_result() {
        let body = concat!(
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[]}}\n\n"
        );
        let message = first_rpc_message(&parse_sse(body)).unwrap();
        assert_eq!(message["id"], 2);
    }

    #[test]
    fn ignores_malformed_sse_events() {
        let body = "data: not json\n\ndata: {\"id\":1,\"result\":{}}\n\n";
        assert_eq!(parse_sse(body).len(), 1);
    }

    #[test]
    fn prefers_structured_content() {
        let result = json!({
            "structuredContent": { "champion": "Ahri" },
            "content": [{ "type": "text", "text": "ignored" }]
        });
        assert_eq!(tool_payload(&result).unwrap()["champion"], "Ahri");
    }

    #[test]
    fn falls_back_to_json_in_a_text_block() {
        let result = json!({ "content": [{ "type": "text", "text": "{\"champion\":\"Ahri\"}" }] });
        assert_eq!(tool_payload(&result).unwrap()["champion"], "Ahri");
    }

    #[test]
    fn keeps_prose_as_a_string() {
        let result = json!({ "content": [{ "type": "text", "text": "no data" }] });
        assert_eq!(tool_payload(&result).unwrap(), json!("no data"));
    }

    #[test]
    fn truncates_long_error_bodies() {
        let long = "x".repeat(500);
        assert!(truncate(&long, 10).ends_with('…'));
        assert_eq!(truncate("short", 10), "short");
    }
}
