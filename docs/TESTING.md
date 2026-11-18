# Testing

| Layer | Command | Covers |
|---|---|---|
| UI E2E | `npm run build && npm run e2e` (with `vite preview` on :4173) | 62 Playwright checks: shell, sidebar, toolbar, modal, clipboard detection, escaping, computed styles |
| Rust unit | `cd src-tauri && cargo test --lib` | 52 tests: filename sanitizer, traversal, Range splitting, routing, magnet detection, segment metadata, percent clamping |
| HTTP engine | `cargo test --test engine_live` | Local Range server: full download and resume-from-partial with content verification |
| Internet smoke | `cargo test --test real_net` | Small live download; skips if network unavailable |
| Torrent diagnostic | `cargo test --test torrent_live -- --nocapture` | Live Big Buck Bunny torrent; network-dependent and occasionally flaky |
| Benchmark | `cargo test --test benchmark -- --nocapture` | 128 MiB loopback scaling at 1/8/16/32 connections |
| Installed app | `node scripts/smoke-installed.mjs` (needs the app running with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`) | Drives the real packaged app: modal, Escape, view switching, search, theme toggle, no console errors |
| Icon integrity | `node scripts/verify-icons.mjs` | Seam detection across PNG/ICO/ICNS |

`torrent_live` is a live-network diagnostic: one run may fail and a rerun pass. It is not a deterministic gate.
