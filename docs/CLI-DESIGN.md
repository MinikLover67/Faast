# CLI parity — design (deferred)

## Why deferred

A CLI that shares the engine with the desktop app is attractive to the HN audience, but the engine is currently coupled to Tauri: every download function takes `&tauri::AppHandle` (for event emission) and `&State<Mutex<AppState>>` (for state). Shipping a half-working CLI by duplicating that logic would create two engines that drift. This document records the design so it can be implemented cleanly.

## Proposed shape

```
faast add <url> [-o DIR] [-s SEGMENTS] [--schedule ISO8601]
faast list [--json]
faast pause <id>
faast resume <id>
faast retry <id>
faast remove <id>
faast status <id>
```

`faast list --json` is the highest-value command: it makes the CLI scriptable and is trivial once state is decoupled.

## Required refactor

1. Introduce a `DownloadHost` trait in `faast_lib`:
   - `fn emit_progress(&self, progress: DownloadProgress)`
   - `fn state(&self) -> &Mutex<AppState>`
2. Implement it for Tauri (`TauriHost` wrapping `AppHandle`) and for a headless CLI host (`CliHost` writing progress to stdout / a JSON file).
3. Change engine functions from `&tauri::AppHandle` + `&State<...>` to `host: &impl DownloadHost`.
4. Add a `src-tauri/src/bin/faast-cli.rs` binary that parses args with `clap` and calls the same `faast_lib` functions through `CliHost`.
5. Persist the download list to disk in the headless host so `faast list` works across processes (today state is in memory only).

## Risks

- The Tauri event path must keep emitting exactly as before; regression-test the UI after the refactor.
- Torrent sessions need a non-GUI state directory in headless mode.
- `yt-dlp` progress parsing writes to child-process stdout; the host abstraction must not swallow it.

## Effort

Roughly a focused multi-day task, not a single pass. It is ranked third in [POSITIONING.md](POSITIONING.md), after the trust layer and reliability proof.
