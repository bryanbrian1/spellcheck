# leaguechecker

macOS desktop app. Detects the champion and role you're locked into during
League champion select, then shows the optimal build path (items, runes,
summoners, skill order) for that champion-role pair.

## Hard constraints

- **RAM is the primary constraint.** Target under 120MB resident. Never
  suggest Electron. Never add a heavy UI framework.
- Frontend: vanilla TypeScript + plain CSS. No React, no Tailwind, no
  component library. If a dependency isn't strictly necessary, don't add it.
- Backend: Rust (Tauri v2).
- Build data is static JSON fetched per-champion on demand. Never load the
  full dataset into memory.
- No Riot API key ever ships in the app binary. Keys live only in GitHub
  Actions secrets.
- Never scrape u.gg, op.gg, or any third-party site. Riot's official API and
  Data Dragon only.

## Architecture

- `src-tauri/` — Rust. Lockfile parsing, LCU HTTP client, WebSocket listener.
- `src/` — UI. Vanilla TS, no framework.
- `scripts/ingest/` — Node crawler. Runs in CI only, never in the app.
- `data/builds/{Champion}/{role}.json` — generated build data, committed by CI.
- `.github/workflows/ingest.yml` — scheduled crawl.

## LCU integration notes

- Lockfile: `/Applications/League of Legends.app/Contents/LoL/lockfile`
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
