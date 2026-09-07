# Changelog

Every released version, and what changed in it. Versions are the tags the
release workflow publishes; a tag is the only thing that ships a build, so
anything not listed here was never downloadable.

Dates are the day the tag was pushed. Numbers in brackets are pull requests.

## 0.1.1 — unreleased

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
