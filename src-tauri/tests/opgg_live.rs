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
        match service.build_for(key, position, None).await {
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
