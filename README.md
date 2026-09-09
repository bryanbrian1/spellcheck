# spellcheck

A macOS and Windows app that detects the champion and role you lock into
during League champion select, then shows the optimal build path — items,
runes, summoners, skill order — for that pair.

## Latest release — 0.1.2

Download: **[macOS (universal)](https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev/spellcheck_0.1.2_universal.dmg)**
· **[Windows (x64)](https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev/spellcheck_0.1.2_x64-setup.exe)**

The first build that can tell you a newer one exists, and the first that
anyone outside this repository can actually download.

- An installed copy now checks once at launch whether a newer version is out
  and shows a bar naming it. It never installs on its own.
- The in-game standing was wrong roughly a third of the time and biased
  toward whoever was losing — inventories were priced from a field that
  reports combine cost rather than total, so a finished Rabadon's counted
  1100 against a real 3500. Now priced from the item table.
- Boots and situational items render as a menu you choose between, each
  option named, with its own win rate and sample, sorted by pick rate.
- Rune pages draw as three rows with their tree names, and stat shards have
  their art instead of rendering as three empty boxes.
- A matchup lookup that fails no longer takes the whole build down with it.

These builds are unsigned, so both systems will warn you once — see
[Installing a beta build](#installing-a-beta-build).

**[CHANGELOG.md](CHANGELOG.md)** carries every version and the full detail,
and each [GitHub release](https://github.com/bryanbrian1/spellcheck/releases)
repeats its own entry.

## Start here

- **[docs/blueprint.html](docs/blueprint.html)** — the architecture blueprint.
  How the pieces connect, which rules are load-bearing, what is built and what
  is not, and where to start editing. Open it in a browser.
- **[docs/roadmap.html](docs/roadmap.html)** — what is left to do. The defects
  the first real game turned up, how thin the crawled data still is, and what
  stands between a beta and something you would hand a stranger. A dated
  snapshot rather than a standing description, so check its claims before
  building on them.
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

[CHANGELOG.md](CHANGELOG.md) records every released version and what changed
in it. A tag is the only thing that ships a build, so the changelog entry and
the version bump belong in the same run-up to a tag as the tag itself.

Two other places repeat that entry and go stale silently if they are not
moved with it: the **Latest release** section at the top of this file, and
the GitHub release description, which the workflow fills with install notes
rather than a changelog. Update the first by hand and the second with
`gh release edit vX.Y.Z --notes-file <file>`.

Tagging `v*` builds both platforms and publishes them as a GitHub prerelease.
The tag must match the version in `package.json`, `src-tauri/tauri.conf.json`
and `src-tauri/Cargo.toml` — nothing checks this automatically. Run the
workflow manually (`workflow_dispatch`) to prove a change builds on Windows
without publishing anything.

## Updates

An installed copy checks for a new version once at launch and, if there is
one, offers a bar with a button. It never installs on its own — the same rule
`CLAUDE.md` sets for item set and rune imports, for the same reason.

The source repository is private, and GitHub does not serve a private repo's
release assets to anonymous clients. An installed app is an anonymous client,
and the alternative — shipping it a token — would put a credential that reads
this repository inside a binary anyone can run `strings` on. So the installers
and the update manifest live in a Cloudflare R2 bucket instead, and the
GitHub prerelease stays as our own record of what was built.

Two signatures are involved and they are not substitutes for each other:

- **The updater signature** is a minisign keypair. The plugin verifies every
  download against the public key baked into `src-tauri/tauri.conf.json`, the
  check cannot be disabled, and it is the only reason serving these files
  from a public bucket is safe. Generate one with
  `npx tauri signer generate -w ~/.tauri/spellcheck.key`. **Lose the private
  key and no installed copy can ever be updated again** — there is no
  recovery, only a new key and a new manual install for everybody.
- **OS code signing** is separate, still absent, and is what Gatekeeper and
  SmartScreen care about.

The release workflow needs these set:

| Name | Kind | What it is |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | secret | contents of `~/.tauri/spellcheck.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secret | empty unless you set one |
| `R2_ACCOUNT_ID` | secret | Cloudflare account id |
| `R2_ACCESS_KEY_ID` | secret | R2 API token |
| `R2_SECRET_ACCESS_KEY` | secret | R2 API token |
| `R2_BUCKET` | secret | bucket name |
| `R2_PUBLIC_URL` | variable | public base URL, no trailing slash |

`R2_PUBLIC_URL` is a repository *variable* rather than a secret because it is
public by definition — it is compiled into every binary. It must match the
`endpoints` entry in `src-tauri/tauri.conf.json`, and **that URL cannot be
changed for copies already installed**: a shipped binary only ever looks where
it was built to look. Changing it strands every existing install on the old
address.

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
SPELLCHECK_LOCKFILE='D:\Games\League of Legends\lockfile'
```
