# leaguechecker

A macOS app that detects the champion and role you lock into during League
champion select, then shows the optimal build path — items, runes, summoners,
skill order — for that pair.

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
cd src-tauri && cargo test
```

Rust must be on your `PATH` (`. "$HOME/.cargo/env"`).
