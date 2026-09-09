# spellcheck

A macOS and Windows app that detects the champion and role you lock into
during League champion select, then shows the optimal build path — items,
runes, summoners, skill order — for that pair.

## Latest release — 0.1.3

Download: **[macOS (universal)](https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev/spellcheck_0.1.3_universal.dmg)**
· **[Windows (x64)](https://pub-7d8d63aa0eec43f5a16598403866eed1.r2.dev/spellcheck_0.1.3_x64-setup.exe)**

Windows installs its own updates now. macOS still does not, and will not
until the app is code signed.

- The update button downloads, installs and restarts into the new version on
  Windows, instead of opening a browser and leaving the rest to you. It says
  which of the two things it does, and disables itself while installing.
- On macOS it stays a notice: an unsigned bundle cannot replace itself inside
  `/Applications` without destroying itself.
- Every GitHub release now carries its own changelog entry rather than the
  same install warnings, generated from `CHANGELOG.md` at build time.

You will not see the Windows install button working until 0.1.4 — an
installed copy uses whatever updater it was built with, so 0.1.3 has to be
installed by hand once more.

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
one, offers a bar with a button. Nothing is ever installed without that press
— the same rule `CLAUDE.md` sets for item set and rune imports, for the same
reason.

What the press does depends on the platform, and the app decides rather than
the page. **On Windows** it downloads the update, installs it and restarts
into it; SmartScreen fires each time, because the build is unsigned. **On
macOS** it opens the installer in a browser and you install it by hand: an
unsigned bundle cannot replace itself inside `/Applications` — App Management
has no code signature to check, so the new copy lands beside the old one under
a collision name and the original is deleted. Code signing is the only thing
that changes this.

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

## About

I'm a hardstuck gold mid. I built this for myself, and I'm not qualified to
tell anyone what to build — so the app doesn't. It shows you what the data
says, and when it's reasoning rather than measuring, it tells you that too.

It's a side project and it's early. If it gets something wrong in your game,
that's the thing I want to hear about.

Whatever else changes, a few decisions are not up for negotiation, because
they are the reasons this exists rather than preferences about how to build it:

- **It stays small.** Rust and Tauri, a hard budget of under 120MB resident,
  a frontend of vanilla TypeScript and plain CSS. Nothing that runs beside a
  game should cost more than the game does.
- **It never scrapes.** Documented APIs only — Riot's own, Data Dragon, and
  OP.GG's published endpoint.
- **It never acts on its own.** Item set and rune page imports happen on a
  button press or not at all. An app that rewrites your runes mid-champ-select
  has taken the decision away from you.
- **A statistic and a rule never look alike.** A win rate is a measurement; a
  reason to build antiheal is an argument. The interface will not let the
  second borrow the authority of the first.

Bug reports and feedback are welcome in
[issues](https://github.com/bryanbrian1/spellcheck/issues) — what broke is more
useful than what worked.

## Licence

MIT. See [LICENSE](LICENSE).

The build data is not ours to relicense and is not covered by it. Riot-sourced
data — Data Dragon and our own crawl under `data/` — is redistributable and
ships with the app. OP.GG data is fetched per request, never cached into this
repository and never redistributed, which is why `OpggProvider` holds nothing
between calls.

spellcheck isn't endorsed by Riot Games and doesn't reflect the views or
opinions of Riot Games or anyone officially involved in producing or managing
Riot Games properties. Riot Games and all associated properties are trademarks
or registered trademarks of Riot Games, Inc.

## Installing a beta build

The betas are unsigned, so both systems try to stop you. macOS does it once
per install; Windows does it on every update, because the updater runs the
installer again each time. Neither is a sign of a bad download.

**macOS**

1. Open the `.dmg` and drag spellcheck out of it.
2. Launch it. It gets blocked — do this anyway, because the refused launch is
   what puts the app in the queue for step 3.
3. System Settings → Privacy & Security, scroll down to Security. There is a
   line naming spellcheck and an **Open Anyway** button. Click it and
   authenticate.
4. Launch again. It opens, and keeps opening from then on.

Right-clicking the app and choosing **Open** used to be the shortcut for this
and no longer works on current macOS. System Settings is the only route.

**Keep betas out of `/Applications`.** An unsigned bundle cannot replace
itself there: the updater leaves the new copy beside the old one under a
collision name and deletes the original, so the relaunch finds nothing. Use
`~/Applications` or anywhere else until the app is signed.

**Windows**

1. Run the `-setup.exe`. SmartScreen shows "Windows protected your PC", with
   only a **Don't run** button.
2. Click **More info**. That reveals a **Run anyway** button.
3. Click **Run anyway**.

Expect this on every update, not just the first install.

**Why none of this is signed: I'm broke.** That is the whole answer. Neither
warning means the download was tampered with or that something is wrong with
the build — they mean nobody has paid to vouch for who made it, and the thing
being bought is identity verification, not safety. Apple wants $99 a year for
the Developer ID certificate. A Windows certificate authority wants somewhere
around $200–600 a year, or roughly $10 a month through Azure Trusted Signing
if you qualify for it. That is a real bill for a beta with a handful of
testers, so it is not one I am paying yet. The unsigned installer and the
unsigned updater both come from the same missing line item.

It is not permanent, and nothing needs rewriting when it changes. The release
workflow already reads the `APPLE_*` secrets, so macOS signing switches on the
day there is a certificate to hand it; Windows has no equivalent step written
yet. Apple is the one worth doing first — signing there is also what makes the
macOS updater safe, which is a working feature rather than a quieter dialog.

If League is installed somewhere other than the default location — most
likely on Windows, where the installer lets you pick a drive — point the app
at the lockfile yourself:

```sh
SPELLCHECK_LOCKFILE='D:\Games\League of Legends\lockfile'
```
