# spellcheck

Desktop app for macOS and Windows. Detects the champion and role you're
locked into during League champion select, then shows the optimal build path
(items, runes, summoners, skill order) for that champion-role pair.

## Hard constraints

- **RAM is the primary constraint.** Target under 120MB resident. Never
  suggest Electron. Never add a heavy UI framework.
- That budget was set against WKWebView. Windows renders in WebView2, which
  runs more processes, so the number has to be measured on Windows and not
  assumed to carry over. Measure before promising it.
- Frontend: vanilla TypeScript + plain CSS. No React, no Tailwind, no
  component library. If a dependency isn't strictly necessary, don't add it.
- Backend: Rust (Tauri v2).
- Build data is static JSON fetched per-champion on demand. Never load the
  full dataset into memory.
- No Riot API key ever ships in the app binary. Keys live only in GitHub
  Actions secrets.
- Never scrape any third-party site. Data comes from documented APIs only:
  Riot's official API, Data Dragon, and the OP.GG MCP endpoint.
- OP.GG data is per-request only. Never cache it into the repo or
  redistribute it. Only Riot-sourced data is distributable.

## Architecture

- `src-tauri/` — Rust. Lockfile parsing, LCU HTTP client, WebSocket listener.
- `src/` — UI. Vanilla TS, no framework.
- `scripts/ingest/` — Node crawler. Runs in CI only, never in the app.
- `data/builds/{Champion}/{role}.json` — generated build data, committed by CI.
- `.github/workflows/ingest.yml` — scheduled crawl.

## Data providers

All build data goes through a `BuildDataProvider` interface so sources are
swappable. Two implementations:

- `OpggProvider` — live queries to https://mcp-api.op.gg/mcp, tool
  `lol_get_champion_analysis`. Per-request only. NEVER cache OP.GG data into
  the repo or redistribute it — that's republishing their dataset.
- `RiotProvider` — reads our own crawled data/builds/*.json. This is the
  distributable path.

Start on OpggProvider, migrate to RiotProvider as our data accumulates.
The UI must not know which one is active.

## Recommendation engine — three independent checks

Each check returns zero or more suggestions. A suggestion is
{ itemId, priority, reason, source } where source is "stat" or "rule".
Never present a rule-based suggestion as a statistic.

1. **Enemy threat** — enemy comp tags → what to build against
   (armor/MR split, antiheal + timing, tenacity, defensive actives)
2. **Team gaps** — own team tags → what to build for
   (missing frontline, all-AD comp, nobody has antiheal, no engage)
3. **Game state** — behind/even/ahead vs lane opponent, from Live Client
   Data allgamedata (items, level, KDA for all players)
   - behind: components over spikes, defensive, waveclear
   - ahead: snowball items, damage spikes
   Only available in phase 3. Checks 1 and 2 run in champ select.

## Tag files

- data/meta/champions.json — damageType (ad/ap/mixed/true), sustain,
  hardCC, dive, poke, frontline, engage
- data/meta/items.json — answers (armor, mr, antiheal, tenacity,
  anti-shield, anti-crit), cost, buildsInto, isComponent

## LCU integration notes

- Lockfile, macOS: `/Applications/League of Legends.app/Contents/LoL/lockfile`
- Lockfile, Windows: `C:\Riot Games\League of Legends\lockfile` is only the
  installer's default — it takes a drive, and regional builds differ. The
  watcher reads the installer's own records first
  (`%ProgramData%\Riot Games\RiotClientInstalls.json` and
  `Metadata\league_of_legends.*\*.product_settings.yaml`), then the default,
  then the same path on D:–F:. `lockfile` in `providers.json` is the
  user-facing override; `SPELLCHECK_LOCKFILE` wins over it and is for
  development
- When the client cannot be found, the offline event carries every path
  searched and the page shows them. Keep it that way: an unknown install and
  a closed client are otherwise indistinguishable on screen
- Format: `ProcessName:PID:Port:Password:Protocol`
- Auth: HTTP Basic, username `riot`, password from lockfile
- Base URL: `https://127.0.0.1:{port}` — self-signed cert, verification must
  be disabled for localhost only
- **Use the WebSocket, never a polling loop.** Connect to
  `wss://127.0.0.1:{port}` and subscribe with
  `[5, "OnJsonApiEvent_lol-champ-select_v1_session"]`. Polling burns CPU.
- Champion + role come from `myTeam[]` entries: `championId` and
  `assignedPosition` (top/jungle/middle/bottom/utility)

## Rules for you

- Handle the League-client-not-running case in every code path. It's the
  default state, not an edge case.
- Item set and rune page imports must be user-initiated button presses.
  Never automatic.
- Explain any new dependency before adding it.
- Commit in small logical units with clear messages.
