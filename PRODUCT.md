# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

The app is Tauri v2 + vanilla TypeScript + plain CSS (no framework, by hard
constraint). The website follows the same rule: static HTML/CSS/JS in
`site/`, no build step, deployed to Cloudflare Pages from this repository.
Confirmed by the user on 2026-09-13.

## Users

League of Legends players on macOS or Windows who want to know what to build
for the champion and role they just locked in, without alt-tabbing to a
website during champion select. The primary situation is the ~90 seconds of
champ select and the loading screen, with the app open beside the League
client. The website's audience is the general League-playing public arriving
cold (public launch, confirmed 2026-09-13); the job on the site is to
understand what spellcheck does and download it.

## Product Purpose

spellcheck is a desktop companion that detects the champion and role you lock
in during champion select (via the League client's own local API) and shows
the build for that pair: runes, summoners, starting items, core build,
situational items, with the win rate and sample size behind each. Success is
a player who never has to leave the client to look up a build. The website's
success is a download.

## Positioning

- Reads champion **and role** from champ select itself; nothing to type.
- Every percentage carries the number of games it was measured over.
- A statistic and a rule never look alike: win rates are measurements
  (blue); "build antiheal because they have Soraka" is an argument (amber).
  The interface never lets the second borrow the authority of the first.
- Small on purpose: Rust + Tauri, hard budget under 120 MB resident. "Nothing
  that runs beside a game should cost more than the game does."
- Never scrapes; documented APIs only (Riot API, Data Dragon, OP.GG's
  published MCP endpoint).
- Never acts on its own: rune/item imports are a button press or nothing.

## Operating Context

- Runs beside the League client; connects over the LCU WebSocket, no
  polling. Handles "League isn't running" as the default state.
- Current release: 0.1.4 (2026-09-12). Beta. Builds are **unsigned** on both
  platforms: macOS blocks the first open (Privacy & Security → Open Anyway),
  Windows shows SmartScreen on install and on every update.
- macOS updater is notify-only until the app is code signed; Windows
  self-updates on a button press.
- Installers and `latest.json` update manifest are served from a Cloudflare
  R2 public bucket: `https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev/`.
  Files: `spellcheck_{version}_universal.dmg`,
  `spellcheck_{version}_x64-setup.exe`. That URL is compiled into shipped
  binaries and must keep working.
- Source: https://github.com/bryanbrian1/spellcheck (MIT). Issues are the
  feedback channel.
- Intended domain: spellcheck.pro (not yet confirmed purchased as of
  2026-09-13; treat as a single constant).

## Capabilities and Constraints

- Site must be static, framework-free, and light — same ethos as the app.
- Download buttons should reflect the current version; the version and file
  names are derivable from `latest.json` on the R2 bucket (which sets CORS
  or not — verify before relying on client-side fetch; fall back to
  build-time values).
- No Riot API key anywhere in the site. No scraped or OP.GG-derived data on
  the site.
- Riot legal boilerplate required: "spellcheck isn't endorsed by Riot Games
  and doesn't reflect the views or opinions of Riot Games…" (verbatim in
  README).
- Riot Developer Portal verification token lives at `/riot.txt` on the R2
  bucket and `docs/riot.txt`; the site should also serve `riot.txt`.
- Undecided: whether Home and Download are one page or two; changelog,
  install guide, roadmap pages are out of scope for this build (user
  selected Home + Download only).

## Brand Commitments

- Name is lowercase `spellcheck`, always.
- Voice (confirmed binding 2026-09-13): first person, plain, honest. The
  author is "a hardstuck gold mid" who built it for himself and "isn't
  qualified to tell anyone what to build — so the app doesn't." Unsigned
  because "I'm broke." No marketing-speak, no invented claims, no
  testimonials. The README is the voice reference.
- App icon: `src-tauri/icons/icon.png` (and sizes).
- The app's own visual system (in `src/styles.css`): Inter (bundled latin
  subset at `src/fonts/Inter-latin.woff2`), cool near-black neutral ramp,
  teal accent `#2fd4c4`, blue `#5b9cf8` = measured/statistic, amber
  `#f0b429` = rule/heuristic (never a statistic), red, green. Dark only.
  The site is an extension of this world, not a new one.

## Evidence on Hand

- Screenshots: `docs/screenshots/in-use.png` (app beside the League client,
  build filled in for a locked champion) and `docs/screenshots/build.png`
  (Jayce mid: runes, summoners, starting items with win rates and sample
  sizes, e.g. "a win rate over 1,893 games").
- `CHANGELOG.md` — every release with dated, honest notes.
- `docs/blueprint.html`, `docs/roadmap.html` — architecture and roadmap.
- No testimonials, user counts, press, or benchmarks exist. Do not fabricate.
  The Windows memory budget has not been measured; do not claim it.

## Product Principles

1. Honest first. Say what it is (early, unsigned, one person) and what the
   data is (measured vs argued). Never dress a rule up as a statistic.
2. Small is the feature. Weight, dependencies and page bytes are product
   claims, and the site must live up to them.
3. Show, don't assert. Real screenshots, real numbers from the app, no
   invented social proof.
4. The player decides. The app suggests and waits for a press; the site
   informs and offers a download, and pressures nobody.
5. Riot's rules and OP.GG's terms are constraints, not fine print.

## Accessibility & Inclusion

No product-specific standard established. Baseline: keyboard-reachable
downloads, real contrast on the dark palette, reduced-motion respected.
