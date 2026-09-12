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
use super::install;

/// Where the macOS client keeps it. The path is inside the installed app
/// bundle, so it is the same on every Mac with a default install.
#[cfg(target_os = "macos")]
pub const DEFAULT_LOCKFILE_PATH: &str =
    "/Applications/League of Legends.app/Contents/LoL/lockfile";

/// Where the Windows client keeps it, with a weaker guarantee than the macOS
/// path above. The Windows installer lets you choose a drive, and regional
/// builds land somewhere else again, so this is the common default rather
/// than the only answer. [`LockfileSearch`] exists because of that
/// difference: on Windows it asks the installer's own records first and
/// treats this constant as one guess among several.
#[cfg(target_os = "windows")]
pub const DEFAULT_LOCKFILE_PATH: &str = r"C:\Riot Games\League of Legends\lockfile";

/// The default install directory on the drives people most often pick when
/// they do not take `C:`. Checked after the installer's records, which is
/// where a non-default install is normally found; these are for a machine
/// whose records are missing or unreadable. A stat of a path that does not
/// exist is what each one costs.
#[cfg(target_os = "windows")]
const OTHER_DRIVE_LOCKFILE_PATHS: [&str; 3] = [
    r"D:\Riot Games\League of Legends\lockfile",
    r"E:\Riot Games\League of Legends\lockfile",
    r"F:\Riot Games\League of Legends\lockfile",
];

/// Anywhere else there is no client to find. A path that cannot exist is the
/// honest default: every lookup reports the state the app is designed to
/// spend most of its life in, the client is closed, and nothing above here
/// needs to know the platform is unsupported. The override still works, which
/// is what keeps the fake client usable on any machine.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const DEFAULT_LOCKFILE_PATH: &str = "/nonexistent/league-client/lockfile";

/// Points the app at a lockfile somewhere else — a non-default install, or a
/// captured file for development on a machine with no client. Wins over the
/// config file's `lockfile` setting, which is the user-facing way to say the
/// same thing.
pub const LOCKFILE_ENV_VAR: &str = "SPELLCHECK_LOCKFILE";

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

    /// The first lockfile found at any of `paths`, in order.
    ///
    /// `Ok(None)` means none of them exists, which is the client being
    /// closed. A path that exists but cannot be read is remembered and
    /// reported only if nothing later in the list succeeds: the client can
    /// be found through one candidate while another is stale, and finding
    /// it is the point.
    pub fn read_first(paths: &[PathBuf]) -> Result<Option<Lockfile>, LcuError> {
        let mut first_error = None;
        for path in paths {
            match Lockfile::read(path) {
                Ok(Some(lockfile)) => return Ok(Some(lockfile)),
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(None),
        }
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

/// Where to look for the lockfile.
///
/// The watcher asks this every time it checks whether the client is running,
/// so a League installed after the app was opened is found on the next
/// check rather than the next launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockfileSearch {
    /// Exactly these paths and nothing else. For tests, which must not find
    /// a real client running on the developer's machine.
    Only(Vec<PathBuf>),
    /// Everywhere the client could be, most deliberate first: the
    /// environment override, then the config file's `lockfile` setting, then
    /// wherever this platform's installer says it put the client, then the
    /// platform's default path.
    Discover {
        /// The config file's `lockfile` setting, if there is one.
        configured: Option<PathBuf>,
    },
}

impl Default for LockfileSearch {
    fn default() -> Self {
        LockfileSearch::Discover { configured: None }
    }
}

impl LockfileSearch {
    /// The paths to try, in order, with repeats removed.
    pub fn candidates(&self) -> Vec<PathBuf> {
        match self {
            LockfileSearch::Only(paths) => paths.clone(),
            LockfileSearch::Discover { configured } => {
                let mut paths = Vec::new();
                paths.extend(env_override());
                paths.extend(configured.clone());
                paths.extend(platform_paths());
                install::dedup(paths)
            }
        }
    }
}

/// `SPELLCHECK_LOCKFILE`, if set to something. Blank is not a choice: it
/// falls through rather than pointing the watcher at the current directory.
fn env_override() -> Option<PathBuf> {
    match std::env::var(LOCKFILE_ENV_VAR) {
        Ok(raw) if !raw.trim().is_empty() => Some(PathBuf::from(raw.trim())),
        _ => None,
    }
}

/// Every place this platform's client is known to live, best-informed first.
#[cfg(target_os = "windows")]
fn platform_paths() -> Vec<PathBuf> {
    let mut paths = install::lockfiles_in(&install::recorded_install_dirs());
    paths.push(PathBuf::from(DEFAULT_LOCKFILE_PATH));
    paths.extend(OTHER_DRIVE_LOCKFILE_PATHS.iter().map(PathBuf::from));
    paths
}

/// The macOS client lives inside its app bundle, so there is one place.
#[cfg(not(target_os = "windows"))]
fn platform_paths() -> Vec<PathBuf> {
    vec![PathBuf::from(DEFAULT_LOCKFILE_PATH)]
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
    fn the_default_path_points_at_this_platform_s_client() {
        let path = PathBuf::from(DEFAULT_LOCKFILE_PATH);
        assert_eq!(path.file_name().unwrap(), "lockfile");

        #[cfg(target_os = "macos")]
        assert!(
            path.starts_with("/Applications/League of Legends.app"),
            "{path:?}"
        );

        // The Windows path is backslash-separated, which `Path` does not split
        // on when the tests are cross-checked from another OS, so match the
        // string rather than the components.
        #[cfg(target_os = "windows")]
        assert!(
            DEFAULT_LOCKFILE_PATH.contains(r"Riot Games\League of Legends"),
            "{DEFAULT_LOCKFILE_PATH}"
        );
    }

    #[test]
    fn the_override_is_searched_before_everything_else() {
        // Windows installs land wherever the installer was pointed, so this
        // override is the supported way out and not only a test seam.
        std::env::set_var(LOCKFILE_ENV_VAR, "/tmp/somewhere-else/lockfile");
        let candidates = LockfileSearch::default().candidates();
        assert_eq!(candidates[0], PathBuf::from("/tmp/somewhere-else/lockfile"));
        assert!(candidates.contains(&PathBuf::from(DEFAULT_LOCKFILE_PATH)));

        // Blank is not a choice — it falls back rather than pointing the
        // watcher at the current directory.
        std::env::set_var(LOCKFILE_ENV_VAR, "   ");
        assert_eq!(
            LockfileSearch::default().candidates()[0],
            PathBuf::from(DEFAULT_LOCKFILE_PATH)
        );

        std::env::remove_var(LOCKFILE_ENV_VAR);
        assert_eq!(
            LockfileSearch::default().candidates()[0],
            PathBuf::from(DEFAULT_LOCKFILE_PATH)
        );
    }

    #[test]
    fn the_configured_path_comes_before_the_platform_s_guesses() {
        std::env::remove_var(LOCKFILE_ENV_VAR);
        let search = LockfileSearch::Discover {
            configured: Some(PathBuf::from("/tmp/configured/lockfile")),
        };
        let candidates = search.candidates();
        assert_eq!(candidates[0], PathBuf::from("/tmp/configured/lockfile"));
        assert!(candidates.contains(&PathBuf::from(DEFAULT_LOCKFILE_PATH)));
    }

    #[test]
    fn a_configured_path_that_is_also_the_default_is_listed_once() {
        std::env::remove_var(LOCKFILE_ENV_VAR);
        let search = LockfileSearch::Discover {
            configured: Some(PathBuf::from(DEFAULT_LOCKFILE_PATH)),
        };
        let candidates = search.candidates();
        assert_eq!(
            candidates.iter().filter(|p| p.as_path() == Path::new(DEFAULT_LOCKFILE_PATH)).count(),
            1,
            "{candidates:?}"
        );
    }

    #[test]
    fn only_means_only() {
        let search = LockfileSearch::Only(vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]);
        assert_eq!(search.candidates(), vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]);
    }

    #[test]
    fn the_first_readable_candidate_wins() {
        let dir = std::env::temp_dir().join("spellcheck-read-first-test");
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("lockfile");
        std::fs::write(&real, "LeagueClient:1:2:pw:https").unwrap();

        let found = Lockfile::read_first(&[
            PathBuf::from("/nonexistent/first/lockfile"),
            real.clone(),
            PathBuf::from("/nonexistent/third/lockfile"),
        ])
        .unwrap()
        .expect("the middle candidate exists");
        assert_eq!(found.port, 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_candidate_existing_is_the_client_being_closed() {
        let found = Lockfile::read_first(&[
            PathBuf::from("/nonexistent/first/lockfile"),
            PathBuf::from("/nonexistent/second/lockfile"),
        ])
        .unwrap();
        assert!(found.is_none());
        assert!(Lockfile::read_first(&[]).unwrap().is_none());
    }

    #[test]
    fn a_bad_candidate_is_reported_only_when_no_good_one_follows() {
        let dir = std::env::temp_dir().join("spellcheck-read-first-bad-test");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad-lockfile");
        std::fs::write(&bad, "this is not a lockfile").unwrap();
        let good = dir.join("lockfile");
        std::fs::write(&good, "LeagueClient:1:2:pw:https").unwrap();

        // Bad then good: found.
        assert!(Lockfile::read_first(&[bad.clone(), good.clone()]).unwrap().is_some());
        // Bad then nothing: the error surfaces, so it can be shown.
        assert!(Lockfile::read_first(&[bad.clone(), PathBuf::from("/nonexistent/lockfile")]).is_err());

        std::fs::remove_dir_all(&dir).ok();
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
