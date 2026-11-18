# Faast — FDM deep research & reverse-engineering notes

> Purpose: document the feature set of Free Download Manager (FDM), how each maps onto Faast, and the clean-room re-implementation approach. Faast is an **independent re-implementation inspired by FDM** — we clone *behavior and QoL*, not source code. FDM is C++/Win32 (open source on SourceForge, mirrored on GitHub); Faast is Tauri v2 + Rust + vanilla JS. No FDM source is copied here.

## FDM feature inventory (from freedownloadmanager.org/features.htm)

| Category | FDM feature | Faast status | Notes / integration approach |
|---|---|---:|---|
| Core | Accelerated / segmented downloads | ✅ | `reqwest` multi-Range (8/12/16 connections), resume from `.faast-part*` segments |
| Core | Resume interrupted downloads | ✅ | `Range:` requests + part-file merge on disk |
| Core | BitTorrent support | ✅ | `librqbit` 9.1.0 (DHT, UDP/HTTP trackers, web seeds, TCP+uTP, PEX) |
| Web video | Add-ons to download from supported sites | ⬜ | Integrate **yt-dlp** (see below) — covers YouTube + 1000s of sites |
| Media | Preview audio/video before complete | ⬜ | Needs a player + partial-file reader; post-1.0 |
| Media | Convert file formats after download | ⬜ | Needs FFmpeg; post-1.0 |
| Scheduler | Start / pause downloads at set times | ⬜ | New `schedule_download` command (Phase 2) |
| Traffic | Speed / traffic-usage modes | ⬜ | New global + per-download byte throttle (Phase 2) |
| File mgmt | Organize by type into predefined folders | ⬜ | Category-based save subfolders (video/music/app/document) |
| Site | Site Explorer (browse site folder tree) | ⬜ | HTTP directory-listing crawler; post-1.0 |
| Site | HTML Spider (mirror whole pages/sites) | ⬜ | Recursive asset crawler; post-1.0 |
| Mirror | Multiple mirrors simultaneously | ⬜ | Same URL from several hosts, first-to-finish wins; post-1.0 |
| ZIP | Partial ZIP download | ⬜ | ZIP central-directory parsing; post-1.0 |
| UX | Remote control (Android) | ⬜ | HTTP/WebSocket API; post-1.0 |
| UX | Portable mode | ⬜ | Store state next to executable; Phase 4 |
| UX | Multilingual (24 languages) | ⬜ | i18n skeleton; Phase 4 |
| UX | Free, no ads, open source | ✅ | MIT license, Tauri + Rust |

## "Download everything" — YouTube & co.

The standard, maintained engine is **yt-dlp** (fork of youtube-dl, 200k+ stars, supports YouTube and thousands of sites, playlists, chapters, subtitles, throttling, cookies, authentication).

### How to integrate in a Tauri/Rust app (clean-room)

1. **Detect `yt-dlp`** on PATH at runtime (`which yt-dlp` / `where yt-dlp`).
2. If missing, surface a clear toast: *"yt-dlp not found — install it to download YouTube/video sites"* with a link to github.com/yt-dlp/yt-dlp. Optionally offer to download a portable `yt-dlp.exe` into the app data dir.
3. Shell out to `yt-dlp` (not a Rust wrapper crate) to keep the binary lightweight and avoid bundling a Python runtime. Commands:
   - `yt-dlp --new-session --print-json --progress <url>` → machine-readable progress lines parsed on stderr/stdout.
   - `--format bestvideo+bestaudio/best` needs **FFmpeg** to merge; document FFmpeg as optional (fall back to `best` single-file when FFmpeg absent).
   - `--cookies-from-browser` / `--cookies` for age-gated/private content.
4. Map yt-dlp progress events (`[download] 42.1% of 12.34MiB at 1.20MiB/s ETA 0:10`) into Faast's existing `download-progress` event so the frontend needs no new protocol.
5. Save into the user's chosen folder with the yt-dlp output template; category = video/music by extension.

### Why not the Rust `yt-dlp` crate?

The `boul2gom/yt-dlp` Rust wrapper downloads the yt-dlp binary at runtime and still shells out. Shelling out directly is simpler, easier to update (the user can `yt-dlp -U`), and keeps Faast's binary small — aligned with "make it more lightweight."

## Reverse-engineering FDM (clean-room, behavior-only)

FDM's open-source core (C++, Win32 API, NSIS installer) is public on SourceForge (`freedownload`) and GitHub mirrors. The parts worth studying for behavior (not copying):

- **Download engine:** how FDM splits files into sections, opens N HTTP connections with `Range:` headers, merges on completion, and retries on failure. → Faast already does this in `run_segmented_download` / `run_single_stream`.
- **Torrent engine:** FDM's built-in libtorrent-based client (DHT, trackers, encryption, selective file download). → Faast uses `librqbit` for the same.
- **Scheduler:** Windows timer jobs that start/pause downloads and connect/disconnect the internet at set times. → Phase 2 `schedule_download`.
- **Speed limit:** token-bucket / leaky-bucket throttling per connection and globally. → Phase 2 throttle.
- **Site Explorer / HTML Spider:** recursive HTTP directory listing and asset crawling. → post-1.0.
- **Browser integration:** "Send to FDM" context-menu extension that captures page media URLs. → post-1.0 (native messaging host).

**Rule:** we read FDM to understand *what* it does and *how the user experiences it*, then implement the equivalent in Rust/Tauri from scratch. Zero FDM source is included.

## QoL features to add beyond FDM (Faast differentiators)

- Dark mode + compact/dense view toggles.
- Keyboard-driven operation (Space pause/resume, Del remove, Ctrl+N add, `j`/`k` navigate).
- Bulk select + bulk pause/resume/remove/export.
- "Open containing folder" / "Open file" / "Retry failed" per row.
- Per-download speed cap + global bandwidth limit.
- Download scheduler (start at time, stop after time).
- Export queue to JSON/CSV.
- Portable mode (self-contained, no installer required).
- Toast + inline error hardening (already in place).

## Phase 2 - implemented (current pass)

### YouTube / video sites
- Backend routes `is_yt_dlp_url` hosts (YouTube, Vimeo, TikTok, SoundCloud, Bandcamp, Reddit, Instagram, Twitter/X, ...) to `yt-dlp`. Thousands more work because yt-dlp itself supports them.
- **Format picker** in the add dialog (shown only for media links):
  - Video: Best (MP4), 1080p, 720p, 480p (all merged to MP4 via FFmpeg)
  - Audio only: MP3, M4A, WAV (lossless), FLAC (lossless), Opus
- Progress is parsed from yt-dlp stderr and emitted on the same `download-progress` event.
- `yt-dlp` and `FFmpeg` are runtime dependencies (winget install), not bundled - keeps Faast's binary small.

### Folder routing rules
- New default root: `Documents\Faast-Downloads` (was the system Downloads folder).
- Ordered rules: `keyword` (case-insensitive substring on name/URL) and `extension`. First match wins. Built-in defaults route `.mp4` -> Video, `.mp3` -> Music, `.exe/.iso/.zip` -> Software, `.pdf/.html` -> Documents, and names containing "game" -> Games.
- Explicit folder choice always wins; routing is a default, not an override.
- Rules are persisted to `routing.json` in the app config dir and editable in a new **Folders** sidebar entry (flat rows, lucide icons - no glass/gradient slop).
- Routing is by name - a game whose name has no "game" word is not auto-sorted. Users add keywords to tune it.

### Where do files go? (html / exe / iso / games)
- A torrent is one folder on disk named after the torrent. The rule applies to that whole folder, so every file inside (HTML, EXE, ISO, ...) lands wherever the torrent's name routes - Games if the name contains "game", otherwise Documents (since .html defaults there) or the root.
- Loose files (HTTP downloads) route per-extension: `.html` -> Documents, `.exe/.msi/.iso/.zip` -> Software, `.mp4` -> Video, `.mp3/.flac/.wav` -> Music.
- "Games" is keyword-driven because a game has no single file extension. Add keywords to tune it.

### In-app file browser
- The Files tab is now a clean two-level list: folder rows on top, click to expand and see the files inside with Open / Open-containing-folder actions.
- Dropped the glass-card-with-glow design (the "AI slop" look). Flat rows, lucide icons, no gradients, no transforms.

### "Rewrite it in Rust like the React thingy?"
- There is NO React in Faast - the frontend is ~1000 lines of vanilla JS in a Tauri webview. Rewriting the UI in Rust means replacing the webview with a native toolkit, which is a months-long rewrite and loses HTML/CSS flexibility. Rust backend + web frontend is the standard Tauri architecture and is already the lightweight option.

### "What did I mean by Card Data Structure?"
- That phrase came from the previous (Cline) conversation, not from me. If it referred to anything concrete: the Files tab's `folderOf` groups downloads by their containing directory into a `Map<folder, DownloadItem[]>`. There is no separate "card" data model - cards were just DOM rendering of that grouping. The new file browser keeps the same grouping but renders it as rows.

### Logo horizontal line - fixed (not an icon bug)
- Every PNG (20 sizes) and all 7 frames inside icon.ico were pixel-scanned and are clean. The white line was a `1px solid var(--border)` border-bottom on `.titlebar`. Removed it.

### Bugs fixed in this pass (beyond the user's asks)
1. `src/icons.js` was emptied to a single blank line -> `vite build` failed outright. Restored from the last good bundle.
2. Stray `}` in the pause/resume button's `aria-label`.
3. Two missing icons (`external-link`, `rotate-ccw`) that silently fell back to a file icon.
4. e2e test strict-mode break from the new Files view's second `.empty-state`.
5. `vite preview` binds only IPv6 `::1`, so `npm run e2e` failed with ERR_CONNECTION_REFUSED. Default host -> localhost.
6. `--new-session` (invalid yt-dlp flag) -> `--newline`.
7. yt-dlp progress parser mishandled exact sizes, speeds, and binary units.

### Verification (current)
- `cargo test --lib`: 50/50 pass
- `npm run e2e`: 62/62 pass
- `scripts/check-format-picker.mjs`: 17/17 pass
- `scripts/check-routing-ui.mjs`: 18/18 pass
- `scripts/check-files-ui.mjs`: 6/6 pass
- `vite build`: clean

### Still open (honest)
- FDM features not yet implemented: scheduler, traffic modes, site explorer, HTML spider, ZIP partial download, remote control, portable mode, multilingual, browser integration, mirror downloads.
- Media preview inside the app and format conversion - need FFmpeg wiring + a player; post-1.0.
- yt-dlp and FFmpeg are external binaries the user installs; Faast detects them at runtime and fails with a clear toast if missing.
- Folder routing is keyword-based, so a game whose name has no "game" word is not auto-sorted. Add keywords to tune it.
