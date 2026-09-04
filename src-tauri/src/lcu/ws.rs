//! The champ select event socket.
//!
//! This is the piece the blueprint is emphatic about: the client pushes, we
//! do not poll. A loop asking "is anything different yet?" wakes the CPU on
//! every tick whether or not anything changed, and it does that while the
//! machine is running a game. The socket costs nothing while nobody speaks.
//!
//! The protocol is the client's own: a JSON array per frame, `[opcode, ...]`.
//! Subscribing is `[5, "<event name>"]`; every subsequent change arrives as
//! `[8, "<event name>", { uri, eventType, data }]`.

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use super::error::LcuError;
use super::lockfile::Lockfile;

/// The one event we subscribe to. Subscribing to `OnJsonApiEvent` instead
/// would deliver every change in the client — thousands of frames a minute
/// during a game — and we would throw almost all of them away.
pub const CHAMP_SELECT_EVENT: &str = "OnJsonApiEvent_lol-champ-select_v1_session";

/// The client's opcodes. Only these two are ever sent or read.
const OP_SUBSCRIBE: u64 = 5;
const OP_EVENT: u64 = 8;

/// One change to one resource.
#[derive(Debug, Clone, PartialEq)]
pub struct LcuJsonEvent {
    /// The resource path, e.g. `/lol-champ-select/v1/session`.
    pub uri: String,
    /// `Create`, `Update` or `Delete`. A `Delete` on the session is how champ
    /// select ends — dodged, declined, or started.
    pub event_type: String,
    pub data: Value,
}

impl LcuJsonEvent {
    pub fn is_delete(&self) -> bool {
        self.event_type.eq_ignore_ascii_case("delete")
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// A live subscription to the client's event stream.
pub struct LcuEventStream {
    socket: Socket,
}

impl LcuEventStream {
    /// Open the socket and subscribe to champ select.
    ///
    /// Failure here is ordinary — the player can quit the client between our
    /// reading the lockfile and this call — so the error is retryable and the
    /// caller goes back to waiting.
    pub async fn connect(lockfile: &Lockfile) -> Result<LcuEventStream, LcuError> {
        let url = lockfile.websocket_url();
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|error| LcuError::Socket(format!("{url}: {error}")))?;

        let header = HeaderValue::from_str(&lockfile.authorization_header())
            .map_err(|error| LcuError::Socket(format!("authorization header: {error}")))?;
        request.headers_mut().insert(AUTHORIZATION, header);

        let (socket, _) = tokio_tungstenite::connect_async_tls_with_config(
            request,
            None,
            false,
            Some(loopback_connector()?),
        )
        .await
        .map_err(|error| LcuError::Socket(format!("{url}: {error}")))?;

        let mut stream = LcuEventStream { socket };
        stream.subscribe(CHAMP_SELECT_EVENT).await?;
        Ok(stream)
    }

    pub async fn subscribe(&mut self, event: &str) -> Result<(), LcuError> {
        let frame = Value::Array(vec![OP_SUBSCRIBE.into(), event.into()]).to_string();
        self.socket
            .send(Message::Text(frame.into()))
            .await
            .map_err(|error| LcuError::Socket(format!("subscribing to {event}: {error}")))
    }

    /// Wait for the next champ select change.
    ///
    /// `Ok(None)` means the client closed the socket, which is what quitting
    /// the client looks like from here. Frames that are not events — the
    /// empty acknowledgement the client sends on subscribe, keepalives — are
    /// skipped rather than surfaced.
    pub async fn next_event(&mut self) -> Result<Option<LcuJsonEvent>, LcuError> {
        while let Some(message) = self.socket.next().await {
            let message = message.map_err(|error| LcuError::Socket(error.to_string()))?;
            match message {
                Message::Text(text) => {
                    if let Some(event) = parse_frame(&text)? {
                        return Ok(Some(event));
                    }
                }
                Message::Close(_) => return Ok(None),
                // Ping/Pong are answered by the library; binary frames are
                // not part of this protocol.
                _ => continue,
            }
        }
        Ok(None)
    }

    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

/// Read one text frame.
///
/// `Ok(None)` covers every frame that is not an event, which includes the
/// empty string the client sends to acknowledge a subscription. Kept free of
/// the socket so the wire format can be tested without a running client.
pub fn parse_frame(text: &str) -> Result<Option<LcuJsonEvent>, LcuError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let frame: Value = serde_json::from_str(trimmed)
        .map_err(|error| LcuError::unexpected("an event frame", error))?;

    let Some(array) = frame.as_array() else {
        return Ok(None);
    };

    if array.first().and_then(Value::as_u64) != Some(OP_EVENT) {
        return Ok(None);
    }

    let payload = array
        .get(2)
        .ok_or_else(|| LcuError::unexpected("an event frame", "no payload"))?;

    Ok(Some(LcuJsonEvent {
        uri: payload
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        event_type: payload
            .get("eventType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        data: payload.get("data").cloned().unwrap_or(Value::Null),
    }))
}

/// TLS that trusts the client's self-signed certificate.
///
/// Same justification as [`LcuClient`](super::client::LcuClient), and the
/// same narrow scope: this connector is only ever handed a `wss://127.0.0.1`
/// URL built from the lockfile port. The crypto provider is named explicitly
/// rather than taken from process state, so nothing else in the app can
/// change what this handshake uses.
fn loopback_connector() -> Result<Connector, LcuError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| LcuError::Socket(format!("TLS setup: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(LoopbackCertificates { provider }))
        .with_no_client_auth();

    Ok(Connector::Rustls(Arc::new(config)))
}

/// Accepts any certificate, because the only server it will ever meet is the
/// one on this machine that issued its own. Signatures are still verified
/// normally — this asserts nothing about the handshake itself, only that we
/// have no chain to check the certificate against.
#[derive(Debug)]
struct LoopbackCertificates {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for LoopbackCertificates {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_champ_select_update() {
        let frame = json!([
            8,
            CHAMP_SELECT_EVENT,
            {
                "data": { "localPlayerCellId": 2, "myTeam": [] },
                "eventType": "Update",
                "uri": "/lol-champ-select/v1/session"
            }
        ])
        .to_string();

        let event = parse_frame(&frame).unwrap().unwrap();
        assert_eq!(event.uri, "/lol-champ-select/v1/session");
        assert_eq!(event.event_type, "Update");
        assert!(!event.is_delete());
        assert_eq!(event.data["localPlayerCellId"], 2);
    }

    #[test]
    fn recognises_the_end_of_champ_select() {
        let frame = json!([8, CHAMP_SELECT_EVENT, {
            "data": null, "eventType": "Delete", "uri": "/lol-champ-select/v1/session"
        }])
        .to_string();

        assert!(parse_frame(&frame).unwrap().unwrap().is_delete());
    }

    #[test]
    fn skips_frames_that_are_not_events() {
        // The acknowledgement the client sends when a subscription lands.
        assert!(parse_frame("").unwrap().is_none());
        assert!(parse_frame("   ").unwrap().is_none());
        // A subscribe echoed back, not an event.
        assert!(parse_frame(&json!([5, CHAMP_SELECT_EVENT]).to_string())
            .unwrap()
            .is_none());
        // An object rather than a frame array.
        assert!(parse_frame(&json!({ "hello": true }).to_string())
            .unwrap()
            .is_none());
    }

    #[test]
    fn junk_on_the_wire_is_an_error_not_a_panic() {
        assert!(parse_frame("[8, this is not json").is_err());
    }

    #[test]
    fn tolerates_an_event_missing_its_optional_fields() {
        let event = parse_frame(&json!([8, CHAMP_SELECT_EVENT, {}]).to_string())
            .unwrap()
            .unwrap();
        assert_eq!(event.uri, "");
        assert_eq!(event.data, Value::Null);
    }
}
