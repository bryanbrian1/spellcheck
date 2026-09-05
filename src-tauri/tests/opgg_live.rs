//! Live smoke test against the OP.GG endpoint.
//!
//! Ignored by default: it needs the network, it depends on a third party
//! being up, and `cargo test` must stay offline and instant. Run it
//! deliberately, when something about the provider is in doubt:
//!
//! ```text
//! cargo test --test opgg_live -- --ignored --nocapture
//! ```
//!
//! It exists because the whole class of bug it catches is silent. Every
//! argument this provider sends is checked by a server that answers a wrong
//! one with a successful call and no data — indistinguishable, from inside
//! the app, from "OP.GG has nothing for that pair". The unit tests pin what we
//! send and how we read the answer; only this pins that the two still meet.

use leaguechecker::{BuildLookup, BuildService, ProviderConfig};

/// The exact call champ select makes, for champions whose Data Dragon keys
/// exercise the awkward corners of the endpoint's UPPER_SNAKE_CASE spelling.
#[tokio::test]
#[ignore = "hits the live OP.GG endpoint"]
async fn returns_real_builds() {
    let service = BuildService::from_config(&ProviderConfig::default()).unwrap();
    let mut failures = Vec::new();

    for (key, position) in [
        ("Ahri", "middle"),
        ("MonkeyKing", "jungle"),
        ("JarvanIV", "jungle"),
        ("KSante", "top"),
        ("KogMaw", "bottom"),
        ("Thresh", "utility"),
    ] {
        match service.build_for(key, position, None).await.map(|r| r.lookup) {
            Ok(BuildLookup::Found(build)) => {
                let core = build
                    .items
                    .core
                    .first()
                    .map(|group| {
                        group
                            .items
                            .iter()
                            .map(|item| item.name.clone().unwrap_or_else(|| format!("#{}", item.id)))
                            .collect::<Vec<_>>()
                            .join(" > ")
                    })
                    .unwrap_or_default();

                println!(
                    "{key:>11} {position:<8} patch {:<6} {} games — {core}",
                    build.source.patch.as_deref().unwrap_or("?"),
                    build.stats.and_then(|stats| stats.games).unwrap_or(0),
                );

                // Item names are what proves the ids and names were zipped
                // rather than one of them being dropped on the floor.
                if core.contains('#') || core.is_empty() {
                    failures.push(format!("{key} {position}: no item names in the core build"));
                }
                if build.runes.is_empty() || build.summoners.is_empty() {
                    failures.push(format!("{key} {position}: runes or summoners missing"));
                }
                if build.skills.order.is_empty() {
                    failures.push(format!("{key} {position}: no skill order"));
                }
            }
            // Not a failure of ours, but worth seeing: a champion-role pair
            // the source genuinely has nothing for.
            Ok(BuildLookup::NoData(no_data)) => {
                failures.push(format!("{key} {position}: no data — {}", no_data.detail))
            }
            Err(error) => failures.push(format!("{key} {position}: {error}")),
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The lane fallback, against the live summary.
///
/// This is the half that cannot be unit tested at all: the tool's own schema
/// advertises `all` and `none` for `position` and the server rejects both, so
/// the only way to know this question is still answerable the way we ask it
/// is to ask it. A champion played in one lane and one played in three are
/// both here, because the second is where picking the wrong entry would show.
#[tokio::test]
#[ignore = "hits the live OP.GG endpoint"]
async fn names_the_lane_a_champion_is_actually_played_in() {
    let service = BuildService::from_config(&ProviderConfig::default()).unwrap();
    let mut failures = Vec::new();

    // Yasuo is mid first and top second; Thresh is support and nothing else;
    // Teemo is top over jungle. All three are stable enough to assert on.
    for (key, expected) in [("Yasuo", "middle"), ("Thresh", "utility"), ("Teemo", "top")] {
        // The empty position is exactly what Practice Tool and customs give.
        match service.build_for(key, "", None).await {
            Ok(resolved) => {
                assert!(resolved.inferred_role, "{key}: reported as an assigned lane");
                let role = match &resolved.lookup {
                    BuildLookup::Found(build) => build.role,
                    BuildLookup::NoData(no_data) => no_data.role,
                };
                if role.as_str() != expected {
                    failures.push(format!("{key}: chose {role}, expected {expected}"));
                }
            }
            Err(error) => failures.push(format!("{key}: {error}")),
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
