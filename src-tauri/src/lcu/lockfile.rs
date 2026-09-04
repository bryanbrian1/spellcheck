//! The lockfile: how a program on this machine learns to talk to the client.
//!
//! The League launcher writes one line when it starts and deletes it when it
//! quits, which makes the file's *existence* the cheapest available answer to
//! "is the client running". Both facts we need — the port and the password —
//! come from that line, and both change every launch, so nothing here may be
//! remembered across sessions.

use std::fmt;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use super::error::LcuError;

/// Where the macOS client keeps it. The path is inside the installed app
/// bundle, so it is the same on every Mac with a default install.
pub const DEFAULT_LOCKFILE_PATH: &str =
    "/Applications/League of Legends.app/Contents/LoL/lockfile";

/// Points the app at a lockfile somewhere else — a non-default install, or a
/// captured file for development on a machine with no client.
pub const LOCKFILE_ENV_VAR: &str = "LEAGUECHECKER_LOCKFILE";

/// The LCU authenticates every caller as this user; the password is the
/// per-launch token from the lockfile.
pub const LCU_USERNAME: &str = "riot";

/// Every field of `ProcessName:PID:Port:Password:Protocol`.
#[derive(Clone, PartialEq, Eq)]
pub struct Lockfile {
    pub process: String,
    pub pid: u32,
    pub port: u16,
    /// Per-launch credential. Never logged, never persisted, never sent
    /// anywhere but 127.0.0.1.
    password: String,
    pub protocol: String,
}

impl Lockfile {
    /// Parse one lockfile line.
    ///
    /// The client writes the file in one go, but a reader can still catch it
    /// mid-write and see a truncated line, so a bad parse is a retryable
    /// error rather than a reason to stop watching.
    pub fn parse(raw: &str, path: &Path) -> Result<Lockfile, LcuError> {
        let malformed = |detail: String| LcuError::LockfileMalformed {
            path: path.display().to_string(),
            detail,
        };

        let line = raw.trim();
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() != 5 {
            return Err(malformed(format!(
                "expected 5 colon-separated fields, found {}",
                fields.len()
            )));
        }

        let pid = fields[1]
            .parse::<u32>()
            .map_err(|error| malformed(format!("PID {:?}: {error}", fields[1])))?;
        let port = fields[2]
            .parse::<u16>()
            .map_err(|error| malformed(format!("port {:?}: {error}", fields[2])))?;

        if fields[3].is_empty() {
            return Err(malformed("the password field is empty".to_string()));
        }

        Ok(Lockfile {
            process: fields[0].to_string(),
            pid,
            port,
            password: fields[3].to_string(),
            protocol: fields[4].to_string(),
        })
    }

    /// Read the lockfile if the client is running.
    ///
    /// `Ok(None)` means no file, which means no client. That is the machine's
    /// usual state and every caller treats it as an answer, not a fault.
    pub fn read(path: &Path) -> Result<Option<Lockfile>, LcuError> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Lockfile::parse(&raw, path).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(LcuError::LockfileUnreadable {
                path: path.display().to_string(),
                detail: error.to_string(),
            }),
        }
    }

    /// The lockfile this machine should be watching.
    pub fn discover() -> Result<Option<Lockfile>, LcuError> {
        Lockfile::read(&default_lockfile_path())
    }

    /// Base URL for the REST API. Always loopback: the port in the lockfile
    /// is bound to 127.0.0.1 and the certificate is self-signed for it.
    pub fn http_base(&self) -> String {
        format!("{}://127.0.0.1:{}", self.http_scheme(), self.port)
    }

    /// The same server, upgraded. This is the endpoint that removes the need
    /// to poll anything.
    pub fn websocket_url(&self) -> String {
        let scheme = if self.http_scheme() == "https" { "wss" } else { "ws" };
        format!("{scheme}://127.0.0.1:{}/", self.port)
    }

    /// `Basic base64("riot:password")`, built in one place so the password has
    /// exactly one route out of this struct.
    pub fn authorization_header(&self) -> String {
        format!(
            "Basic {}",
            BASE64.encode(format!("{LCU_USERNAME}:{}", self.password))
        )
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    fn http_scheme(&self) -> &str {
        if self.protocol.eq_ignore_ascii_case("http") {
            "http"
        } else {
            "https"
        }
    }
}

/// Redacted on purpose. This struct ends up in log lines and error contexts,
/// and a credential that reaches a log has left the machine's control.
impl fmt::Debug for Lockfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lockfile")
            .field("process", &self.process)
            .field("pid", &self.pid)
            .field("port", &self.port)
            .field("password", &"<redacted>")
            .field("protocol", &self.protocol)
            .finish()
    }
}

pub fn default_lockfile_path() -> PathBuf {
    match std::env::var(LOCKFILE_ENV_VAR) {
        Ok(raw) if !raw.trim().is_empty() => PathBuf::from(raw.trim()),
        _ => PathBuf::from(DEFAULT_LOCKFILE_PATH),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        PathBuf::from("/tmp/lockfile")
    }

    fn sample() -> Lockfile {
        Lockfile::parse("LeagueClient:4242:52519:8kBpBnLYaQpVJfE0Ck2Aqg:https", &path()).unwrap()
    }

    #[test]
    fn parses_the_documented_format() {
        let lockfile = sample();
        assert_eq!(lockfile.process, "LeagueClient");
        assert_eq!(lockfile.pid, 4242);
        assert_eq!(lockfile.port, 52519);
        assert_eq!(lockfile.password(), "8kBpBnLYaQpVJfE0Ck2Aqg");
        assert_eq!(lockfile.protocol, "https");
    }

    #[test]
    fn tolerates_a_trailing_newline() {
        let lockfile =
            Lockfile::parse("LeagueClient:1:2:pw:https\n", &path()).unwrap();
        assert_eq!(lockfile.port, 2);
    }

    #[test]
    fn a_half_written_line_is_retryable_not_fatal() {
        let error = Lockfile::parse("LeagueClient:4242:525", &path()).unwrap_err();
        assert!(error.is_retryable(), "{error}");
    }

    #[test]
    fn rejects_junk_fields() {
        for line in [
            "LeagueClient:notapid:52519:pw:https",
            "LeagueClient:4242:notaport:pw:https",
            "LeagueClient:4242:52519::https",
            "",
        ] {
            assert!(Lockfile::parse(line, &path()).is_err(), "accepted {line:?}");
        }
    }

    #[test]
    fn a_missing_lockfile_means_the_client_is_closed() {
        let missing = PathBuf::from("/nonexistent/League of Legends.app/lockfile");
        assert!(Lockfile::read(&missing).unwrap().is_none());
    }

    #[test]
    fn builds_loopback_urls() {
        let lockfile = sample();
        assert_eq!(lockfile.http_base(), "https://127.0.0.1:52519");
        assert_eq!(lockfile.websocket_url(), "wss://127.0.0.1:52519/");
    }

    #[test]
    fn authorizes_as_riot_with_the_lockfile_password() {
        // base64("riot:8kBpBnLYaQpVJfE0Ck2Aqg")
        assert_eq!(
            sample().authorization_header(),
            "Basic cmlvdDo4a0JwQm5MWWFRcFZKZkUwQ2syQXFn"
        );
    }

    #[test]
    fn debug_output_never_carries_the_password() {
        let rendered = format!("{:?}", sample());
        assert!(!rendered.contains("8kBpBnLYaQpVJfE0Ck2Aqg"), "{rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
    }
}
