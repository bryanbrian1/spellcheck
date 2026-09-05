# leaguechecker

A macOS and Windows app that detects the champion and role you lock into
during League champion select, then shows the optimal build path — items,
runes, summoners, skill order — for that pair.

## Start here

- **[docs/blueprint.html](docs/blueprint.html)** — the architecture blueprint.
  How the pieces connect, which rules are load-bearing, what is built and what
  is not, and where to start editing. Open it in a browser.
- **[CLAUDE.md](CLAUDE.md)** — the constraints themselves. This is the
  authority; the blueprint explains and connects these rules but never
  overrides them.

## Build

```sh
npm install
npm run dev          # run it
npm run app          # bundle a .app into src-tauri/target/release/bundle/macos/
npm run fake-client  # a stand-in League client, to develop without a game
cd src-tauri && cargo test
```

Rust must be on your `PATH` (`. "$HOME/.cargo/env"`).

`npm run app` builds for the machine you are on: a `.dmg` on macOS, an NSIS
`.exe` installer on Windows. There is no cross-compile — `ring` builds C, so
the Windows installer can only be produced on Windows. That is what the
Windows runner in `.github/workflows/release.yml` is for.

The crawler that fills `data/builds/` runs in CI, not in the app, and is
dependency-free — its tests need nothing installed:

```sh
npm run test:ingest
RIOT_API_KEY=RGAPI-... node scripts/ingest/index.mjs   # a real crawl
```


## Releases

Tagging `v*` builds both platforms and publishes them as a GitHub prerelease.
The tag must match the version in `package.json`, `src-tauri/tauri.conf.json`
and `src-tauri/Cargo.toml` — nothing checks this automatically. Run the
workflow manually (`workflow_dispatch`) to prove a change builds on Windows
without publishing anything.

## Installing a beta build

The betas are unsigned, so both systems will try to stop you once. This is
expected and not a sign of a bad download.

- **macOS** — the app is blocked on first open. System Settings → Privacy &
  Security → scroll down → **Open Anyway**.
- **Windows** — SmartScreen shows "Windows protected your PC". **More info**
  → **Run anyway**.

Signing removes both prompts: an Apple Developer account ($99/yr) for macOS,
and a certificate for Windows. Adding the `APPLE_*` secrets to the repo is
enough to turn macOS signing on — the release workflow already reads them.

If League is installed somewhere other than the default location — most
likely on Windows, where the installer lets you pick a drive — point the app
at the lockfile yourself:

```sh
LEAGUECHECKER_LOCKFILE='D:\Games\League of Legends\lockfile'
```
