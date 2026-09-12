//! Where Riot's own installer says League is.
//!
//! On Windows the install directory is whatever the player typed into the
//! Riot installer, so a fixed path is a guess. The installer keeps its own
//! records, though, and those are the answer: under `%ProgramData%\Riot
//! Games` it writes `RiotClientInstalls.json`, which maps every installed
//! game to the Riot Client that manages it, and a per-product
//! `Metadata\<product>\<product>.product_settings.yaml`, which names that
//! product's install directory outright. Both are plain files the installer
//! maintains for itself; reading them is how this app finds a League that was
//! put on `D:`.
//!
//! The parsers are platform-neutral on purpose. Only the directory they read
//! from is a Windows fact, and keeping them apart is what lets the parsing be
//! tested on the machine this app is developed on, which is not a Windows
//! machine.

use std::path::{Path, PathBuf};

/// The installer's registry of games, relative to `%ProgramData%`.
const INSTALLS_FILE: &str = r"Riot Games\RiotClientInstalls.json";

/// The per-product metadata directory, relative to `%ProgramData%`. Each
/// product has a subdirectory named after it — `league_of_legends.live`,
/// `league_of_legends.pbe` — holding `<product>.product_settings.yaml`.
const METADATA_DIR: &str = r"Riot Games\Metadata";

/// The product prefix the metadata directories carry. Anything else under
/// `Metadata` is another Riot game, which never writes a lockfile we can use.
const LEAGUE_PRODUCT_PREFIX: &str = "league_of_legends.";

/// The key in `product_settings.yaml` that names the install directory.
const INSTALL_PATH_KEY: &str = "product_install_full_path:";

/// Every League install directory the Riot installer has on record.
///
/// Empty when there is no record — a machine without League, or one that is
/// not Windows. Nothing here is an error: a missing record is exactly what
/// most machines look like, and the caller has other places to look.
pub fn recorded_install_dirs() -> Vec<PathBuf> {
    let Some(program_data) = program_data_dir() else {
        return Vec::new();
    };
    let mut dirs = Vec::new();

    if let Ok(text) = std::fs::read_to_string(program_data.join(INSTALLS_FILE)) {
        dirs.extend(install_dirs_from_installs_json(&text));
    }
    if let Ok(entries) = std::fs::read_dir(program_data.join(METADATA_DIR)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(LEAGUE_PRODUCT_PREFIX) {
                continue;
            }
            let settings = entry.path().join(format!("{name}.product_settings.yaml"));
            if let Ok(text) = std::fs::read_to_string(settings) {
                dirs.extend(install_dir_from_product_settings(&text));
            }
        }
    }

    dedup(dirs)
}

/// `%ProgramData%`, which Windows sets for every process. Absent anywhere
/// else, which is what makes the lookup above a no-op on macOS.
fn program_data_dir() -> Option<PathBuf> {
    std::env::var_os("ProgramData")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Pull League's install directories out of `RiotClientInstalls.json`.
///
/// The file's `associated_client` object is keyed by game install directory,
/// one key per installed game. League's is the one with "League of Legends"
/// in it; the others are VALORANT and the rest of Riot's catalogue.
pub fn install_dirs_from_installs_json(text: &str) -> Vec<PathBuf> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(clients) = value.get("associated_client").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    clients
        .keys()
        .filter(|dir| dir.to_ascii_lowercase().contains("league of legends"))
        .map(|dir| PathBuf::from(dir.as_str()))
        .collect()
}

/// Pull the install directory out of one `product_settings.yaml`.
///
/// The file is YAML, but the one line we want is `key: "value"` at the top
/// level and a full YAML parser would be a dependency spent on one string.
/// The value is quoted and uses forward slashes, which Windows accepts as
/// path separators, so it is used as written.
pub fn install_dir_from_product_settings(text: &str) -> Option<PathBuf> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(INSTALL_PATH_KEY))
        .map(|value| value.trim().trim_matches(|c| c == '"' || c == '\''))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The lockfile each install directory would hold, in the order given.
pub fn lockfiles_in(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.iter().map(|dir| dir.join("lockfile")).collect()
}

/// Drop repeats while keeping first-seen order. Two records naming the same
/// install are common: the JSON and the YAML both describe the live client.
pub fn dedup(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::with_capacity(paths.len());
    for path in paths {
        if !seen.iter().any(|kept| same_path(kept, &path)) {
            seen.push(path);
        }
    }
    seen
}

/// Windows paths compare case-insensitively and the installer writes them
/// with forward slashes while everything else uses backslashes, so a textual
/// comparison after normalising both is the honest equality here.
fn same_path(a: &Path, b: &Path) -> bool {
    normalise(a) == normalise(b)
}

fn normalise(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INSTALLS_JSON: &str = r#"{
        "associated_client": {
            "C:/Riot Games/League of Legends/": "C:/Riot Games/Riot Client/RiotClientServices.exe",
            "D:/Games/VALORANT/live/": "C:/Riot Games/Riot Client/RiotClientServices.exe"
        },
        "patchlines": { "league_of_legends": "live", "valorant": "live" },
        "rc_default": "C:/Riot Games/Riot Client/RiotClientServices.exe",
        "rc_live": "C:/Riot Games/Riot Client/RiotClientServices.exe"
    }"#;

    const PRODUCT_SETTINGS: &str = r#"locale_data:
  available_locales:
    - en_US
  default_locale: en_US
patching_policy: 1
product_install_full_path: "D:/Games/Riot Games/League of Legends/"
product_install_root: "D:/Games/Riot Games/"
settings:
  create_shortcut: true
  create_uninstall_key: true
  locale: en_US
shortcut_name: League of Legends
"#;

    #[test]
    fn finds_league_and_only_league_in_the_installs_registry() {
        assert_eq!(
            install_dirs_from_installs_json(INSTALLS_JSON),
            vec![PathBuf::from("C:/Riot Games/League of Legends/")]
        );
    }

    #[test]
    fn a_registry_without_league_yields_nothing() {
        let text = r#"{ "associated_client": { "D:/Games/VALORANT/live/": "x" } }"#;
        assert!(install_dirs_from_installs_json(text).is_empty());
        assert!(install_dirs_from_installs_json("not json").is_empty());
        assert!(install_dirs_from_installs_json("{}").is_empty());
    }

    #[test]
    fn reads_the_install_path_out_of_product_settings() {
        assert_eq!(
            install_dir_from_product_settings(PRODUCT_SETTINGS),
            Some(PathBuf::from("D:/Games/Riot Games/League of Legends/"))
        );
    }

    #[test]
    fn product_settings_without_the_key_yield_nothing() {
        assert_eq!(install_dir_from_product_settings("shortcut_name: League\n"), None);
        assert_eq!(install_dir_from_product_settings("product_install_full_path: \"\"\n"), None);
        assert_eq!(install_dir_from_product_settings(""), None);
    }

    #[test]
    fn the_lockfile_sits_in_the_install_directory() {
        let dirs = vec![PathBuf::from("D:/Games/Riot Games/League of Legends/")];
        let lockfiles = lockfiles_in(&dirs);
        assert_eq!(lockfiles.len(), 1);
        assert_eq!(lockfiles[0].file_name().unwrap(), "lockfile");
        assert!(lockfiles[0].starts_with(&dirs[0]));
    }

    #[test]
    fn the_same_install_written_two_ways_is_one_candidate() {
        // The JSON writes forward slashes and a trailing separator; the
        // default constant writes backslashes and neither. Same directory.
        let paths = vec![
            PathBuf::from("C:/Riot Games/League of Legends/"),
            PathBuf::from(r"C:\Riot Games\League of Legends"),
            PathBuf::from(r"c:\riot games\league of legends\"),
            PathBuf::from(r"D:\Riot Games\League of Legends"),
        ];
        let kept = dedup(paths);
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert_eq!(kept[0], PathBuf::from("C:/Riot Games/League of Legends/"));
    }

    #[test]
    fn no_installer_record_is_not_an_error() {
        // On the machine this runs on there is no %ProgramData%, or there is
        // one with no Riot Games directory in it. Either way: nothing found,
        // nothing raised.
        let _ = recorded_install_dirs();
    }
}
