# Contributing to Faast

Thanks for your interest! Faast is an early-alpha Windows download manager built with Tauri v2, Rust, and vanilla JS.

## Ways to contribute

- **Bug reports** — use the bug template; include OS, steps, logs, and a minimal reproducer
- **Feature requests** — open a discussion first (see roadmap in `docs/POSITIONING.md`)
- **Code** — PRs welcome for fixes, docs, tests, and scoped features aligned with the reliability wedge

## Development setup

```bash
# prerequisites
# - Node 20+ (for Vite + Tauri CLI)
# - Rust stable (for backend)
# - Windows 10/11 x64 (target platform)

git clone https://github.com/MinikLover67/Faast
cd Faast
npm install
npm run dev          # Vite on :1420 + Tauri dev window
```

## Test layers

| Layer | Command | What it covers |
|-------|---------|----------------|
| UI E2E | `npm run build && npm run e2e` | 62 Playwright checks (needs `vite preview` on :4173) |
| Rust unit | `cd src-tauri && cargo test --lib` | 52 tests: routing, Range splitting, percent clamping… |
| HTTP engine | `cargo test --test engine_live` | Local Range server: full download + resume |
| Benchmark | `cargo test --test benchmark -- --nocapture` | 128 MiB loopback scaling 1/8/16/32 conns |
| Installed app | `node scripts/smoke-installed.mjs` | Drives the real packaged app via CDP |

Run `cargo test --lib && cargo test --test engine_live` before pushing.

## Code style

- **Rust**: `cargo fmt --all` + `cargo clippy --all-targets -- -D warnings`
- **JS**: no formatter configured yet; match existing vanilla style (4-space, semantic naming)
- **CSS**: custom properties for theming; 4px spacing grid; `currentColor` focus rings

## Architecture notes

- Frontend: single `src/main.js` (view switching, row rendering, folder grouping, theme)
- Backend: `src-tauri/src/lib.rs` (segmented HTTP, librqbit torrents, yt-dlp routing)
- IPC: Tauri `invoke` commands (`add_download`, `pause_download`, `list_downloads`, …)
- State: in-memory `AppState` (downloads, handles, torrent session, yt-dlp children)

## Commit / PR hygiene

- Conventional commits encouraged (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`)
- Keep PRs small and focused; link related issues
- Update docs (`README.md`, `docs/*.md`) when user-facing behavior changes

## What *not* to PR (yet)

- Browser extension — design not settled (see `docs/POSITIONING.md`)
- Cross-platform — Windows-first until v0.2
- Feature parity with AB DM — not the wedge
- MCP / AI integrations — explicitly out of scope

## Questions?

Open a GitHub Discussion or ping the maintainer in an issue.