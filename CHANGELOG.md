# Changelog

All notable changes to Faast. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- Static brand accent color (`--brand`: `#2563eb` light / `#3b82f6` dark) derived from logo
- Logo on README (`docs/assets/faast-logo.png`)
- GitHub repo scaffolding: CI workflow, issue templates, PR template, CONTRIBUTING, SECURITY, CODEOWNERS, FUNDING

### Changed
- README: added logo, fixed repo URLs to `https://github.com/MinikLover67/Faast`

## [0.1.0] — 2026-11-17

### Added
- **Segmented HTTP downloads** — 8–32 parallel Range connections by file size, single-stream fallback
- **Crash-safe resume** — part files on disk are the source of truth; pause/crash doesn't lose progress
- **Torrents & magnets** — `librqbit` engine (DHT, UDP/HTTP trackers, web seeds, TCP+uTP, PEX, UPnP, 500 peer limit)
- **Video/audio links** — yt-dlp routing with format selection (1080p/720p/480p, MP3/M4A/WAV/FLAC/OPUS)
- **Queue controls** — pause, resume, retry, remove, filter by status/category, search, schedule start time
- **Folder routing** — rules by extension or keyword send finished files into subfolders
- **Bandwidth control** — global speed limit (KB/s) for yt-dlp downloads
- **Dark-first UI** — shadcn zinc palette, segmented progress bars (20 blocks), toast feedback, native titlebar
- **Light theme** — FDM-inspired navy/cyan, opt-in via theme toggle
- **Testing layers** — 52 Rust unit tests, Playwright E2E (62 checks), `engine_live` HTTP integration, `real_net` internet smoke, `torrent_live` diagnostic, local benchmark (128 MiB, 1/8/16/32 conns)
- **Packaging** — NSIS + MSI installers, Scoop + winget manifests, code-signing script
- **Launch kit** — Show HN, Reddit, Product Hunt drafts with honest positioning

### Fixed (during v0.1 development)
- Icon seam in all PNG/ICO/ICNS (root cause: `generate-icons.ps1` sheen gradient)
- Laggy UI (duplicate Files view, signature-based re-render, in-place row patching)
- Slow large downloads (segment count now scales to 32; better Range probe; single-stream fallback)
- Windows folder grouping (`folderOf` searched for `\\\\` instead of `\\`)

### Known limitations
- Windows only (no macOS/Linux)
- No browser extension (add downloads by paste/drop)
- Global speed limit applies only to yt-dlp downloads
- No per-torrent file priorities, RSS, remote web UI
- Installer unsigned (SmartScreen warns)
- `torrent_live` test flaky (live network)

[Unreleased]: https://github.com/MinikLover67/Faast/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/MinikLover67/Faast/releases/tag/v0.1.0