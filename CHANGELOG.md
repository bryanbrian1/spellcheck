# Changelog

Every released version, and what changed in it. Versions are the tags the
release workflow publishes; a tag is the only thing that ships a build, so
anything not listed here was never downloadable.

Dates are the day the tag was pushed. Numbers in brackets are pull requests.

## 0.1.3 — 2026-09-09

Windows installs its own updates now. macOS still does not, and will not
until the app is code signed — that is a certificate problem rather than a
code one, and the code for it is already written and waiting.

**You will not see this working until the release after this one.** The
button in an installed copy is whatever that copy was built with, so a
tester on 0.1.2 still gets "Download" when this appears. Install 0.1.3 the
manual way once more, and 0.1.4 is the one that arrives on a single press.

### Changed

- **The update button installs on Windows** instead of opening a browser
  and leaving the rest to you. It downloads, installs and restarts into the
  new version. The in-place installer had been switched off for both
  platforms in 0.1.2 for a reason that only ever applied to macOS, so
  Windows testers were doing manual reinstalls to work around a restriction
  their OS does not have. SmartScreen still fires during the install,
  because the build is unsigned. (#35)
- The button says which of the two things it does — "Install" on Windows,
  "Download" on macOS — and disables itself while an install is running, so
  a long download behind an unchanged button cannot be pressed twice into
  two downloads. (#35)

### Repository

Nothing here changes the app, but both were wrong in a way that was costing
somebody the answer to "what is in this version".

- Every GitHub release description is its own changelog entry now, rather
  than the same eleven lines of install warnings on all of them, and the
  workflow assembles it from `CHANGELOG.md` at build time instead of a
  literal that had to be kept in sync by hand. (#37)
- The README opens with the current release, what changed in it, and the
  download links. (#36)

### Known

- macOS remains notify-only. An unsigned bundle cannot replace itself inside
  `/Applications` — App Management has no code signature to check, so the
  new copy lands beside the old one under a collision name and the original
  is deleted. Code signing is the only thing that changes this.
- Builds are unsigned on both platforms, so macOS blocks the first open and
  Windows shows SmartScreen. On Windows that now happens on every update
  rather than only on first install.
- The download address is an `r2.dev` bucket, which Cloudflare documents as
  development-only and rate limits. Moving off it later costs every
  installed copy one manual reinstall.
- Item set and rune page imports are still disabled everywhere: nothing in
  the LCU layer can write a rune page yet.

## 0.1.2 — 2026-09-09

The first build that can tell you a newer one exists, and the first that
anyone outside the repository can actually download. Both of those are new,
and the second is a correction: the source repository is private, GitHub
does not serve a private repo's release assets to anonymous clients, and an
installed app is an anonymous client — so the 0.1.1 installers were never
downloadable by anybody who was not signed in with access to this repo.

**This one is still a manual install for everybody.** 0.1.1 has no update
check in it, so nothing already on a tester's machine can be told about
0.1.2. The bar starts working for the release after this one.

### Added

- An installed copy checks once at launch whether a newer version exists,
  and shows a bar naming it. It never installs on its own — the button
  opens that version's installer and you install it the ordinary way, which
  is the same rule the item set and rune imports follow. (#32, #33)
- Installers and the update manifest are published to a Cloudflare R2
  bucket that answers anonymously, and the GitHub prerelease stays as our
  own record of what was built. Shipping the app a token that reads a
  private repository was the alternative, and it would have put that
  credential inside a binary anybody can run `strings` on. (#32)

### Fixed

- **The in-game standing was wrong roughly a third of the time, and biased
  toward whoever was losing.** Inventories were priced from the live API's
  `price` field, which is the combine cost rather than the total, so every
  component already consumed into a finished item went uncounted — a
  finished Rabadon's counted 1100 against a real 3500, and finished boots
  counted zero. Measured over 81 samples from one real match: spend counted
  at 46% of its true value, the standing wrong in 30 of them, and the app
  silent while genuinely behind in 15. Behind is the only footing that
  produces advice, so the check went quiet in precisely the window it
  exists to serve. Pricing now comes from the item table, which carries
  `gold.total` for all 868 items. (#26)
- Boots and situational items are a menu you choose between, and now render
  as one: each option named at a readable size, with its own win rate and
  its own sample, sorted by how often it is actually built. They were six
  anonymous 32px squares under a header reading "6 options", with the win
  rate in a tooltip nobody hovers mid-game. (#26)
- A matchup lookup that failed or had too few games took the entire build
  down with it, including the general build the app would otherwise have
  shown. It falls back now. This only ever hit the in-game route, because
  champ select cannot name an opponent in any queue that hides the
  draft. (#26)
- The header says "Game over" when a game has ended, instead of falling
  back to describing the client and leaving a finished game looking
  live. (#26)
- Stat shards draw their art and their names instead of rendering as
  "#5005", "#5008" and "#5001" in three empty-looking boxes. Data Dragon
  does serve the art; the file that indexes runes just omits it. (#26)
- Rune labels no longer print through the art that replaced them. Item art
  is an opaque square and rune art is a transparent symbol, so the label
  underneath showed through the gaps in the glyph. It stands down once art
  has actually loaded, and still stays put when art fails. (#28)
- A rune page renders as the three things it is — primary, secondary,
  shards — each row led by its tree name. Which tree to open for your
  second pair was previously stated nowhere on the screen. (#29)
- The crawler's key check tells a fresh 401 to wait before it tells you to
  re-paste the key. A brand-new Riot key can be rejected until it reaches
  their edge, which is exactly when a 401 is most likely and exactly when
  the old advice was wrong. (#31)

### Known

- Builds are still unsigned, so macOS blocks the first open and Windows
  shows SmartScreen. On Windows this now fires on every update rather than
  only on first install.
- The update bar reports and does not install. An unsigned app cannot
  replace its own bundle inside `/Applications` — it lands beside the old
  one under a collision name and the original is deleted — so the in-place
  install is written, tested and deliberately not wired to anything until
  the app is code signed.
- The download address is an `r2.dev` bucket, which Cloudflare documents as
  development-only and rate limits. Moving off it later costs every
  installed copy one manual reinstall, which is a fair price for a beta and
  would not be for a release.
- Item set and rune page imports are still disabled everywhere: nothing in
  the LCU layer can write a rune page yet.

## 0.1.1 — 2026-09-06

The app is called spellcheck now, and the companion window was redesigned.
Both are visible the moment you open it, so this is not a drop-in
replacement for 0.1.0 — it installs beside the old build rather than over
it, because the bundle identifier changed with the name.

### Renamed

- Everything user-facing carries the new name: product name, window title,
  the label the footer prints as attribution, and the docs. (#19)
- Everything internal follows it: crate, library and binary names, the
  bundle identifier (`com.leaguechecker.desktop` →
  `com.spellcheck.desktop`), both environment variables
  (`SPELLCHECK_LOCKFILE`, `SPELLCHECK_PROVIDER`), the user agents the LCU,
  Live Client and OP.GG clients send, and the log prefix. (#19)
- **The identifier change moves the config directory.** A `providers.json`
  written by 0.1.0 is not read by 0.1.1 and has to be copied across.
- The `gh secret` help in the crawler's key check points at the repo's new
  name. (#22)

### Redesigned

- One flat surface. Suggestions and build items are rows separated by
  space, not boxes nested inside rails inside cards. (#21)
- Item names are readable while you play — the 9px grey captions that
  broke mid-word became 11px in 70px columns. This was the complaint that
  started the redesign. (#21)
- Provenance fires once instead of four times. Amber still means "this is
  a rule, not a statistic" and still never appears on a number, but it is
  carried by the rail and the item name alone. Statistical rails paint
  nothing — a win rate already reads as measured. (#21)
- Every number is monospace, so columns line up. (#21)
- Type scale cut from six sizes to four; the gold deficit is context now
  rather than the largest thing in the window. (#21)
- Mode strip and roles became text and underline tabs.
- Every label at or below 13px clears 4.5:1 contrast; several were between
  2.9:1 and 4.2:1. (#21)
- The disabled primary button no longer paints itself accent. Both import
  buttons are disabled until a press can actually import something, and an
  accent fill nobody can click was the loudest thing on screen. (#21)
- Dropped the Archivo webfont for the native UI stack, so the window draws
  without a network round trip. (#21)

### Known

- No self-update. An installed copy cannot fetch a new version; every
  release is a manual download.
- Builds are unsigned, so macOS blocks the first open and Windows shows
  SmartScreen.
- Item set and rune page imports are still disabled everywhere: nothing in
  the LCU layer can write a rune page yet.

## 0.1.0 — 2026-09-05

First build anyone could download — macOS and Windows, both unsigned
prereleases.

- Detects the champion and role you lock in during champ select, over the
  LCU WebSocket rather than a polling loop, and shows the build for that
  pair.
- Follows one game across champ select and the match, so locking in and
  playing are one screen rather than two.
- Three recommendation checks: what the enemy composition forces, what your
  own team leaves uncovered, and — once a game is running — whether you are
  behind, even or ahead of the person you are actually laning against.
- Search works with no League client running at all, by the champion name a
  player would type rather than the key a provider files it under.
- Build data from the OP.GG provider, with our own crawled Riot data
  accumulating behind the same interface.
