# Faast progress / recovery notes

## Current state
- Stack: Tauri v2 + Rust + vanilla JS Vite. Installed app at C:\Users\Minik\AppData\Local\Faast\faast.exe.
- Backend: librqbit torrents, segmented HTTP (8-32 connections by file size), 52 unit tests + integration suites + 62 E2E.
- Frontend: Files tab + glass folder cards, single source of truth for list/files view.

## Issues reported by user — all addressed in this pass

### 1. "39.9 GB need 109h to install" (slow large download)
- Big HTTP downloads now scale with size: 8 / 12 / 16 / **24 / 32** parallel
  Range connections (the old cap was 16). A 40 GB installer now gets 32
  connections, which is the main lever when a CDN throttles each connection.
- Range probe also accepts `Accept-Ranges: bytes` on a 200 response (some
  servers advertise support but answer the probe with 200).
- If a server pretends to support ranges and the segmented attempt writes zero
  bytes, the engine now falls back to a single stream instead of failing.
- Torrents: peer_limit 500 + UPnP port forwarding were already in place.
  Torrent speed still depends on the swarm — no code change can fix a dead torrent.
- Added `segment_count_scales_with_file_size` unit test.

### 2. White line in the logo (in-app + taskbar)
- ROOT CAUSE: `scripts/generate-icons.ps1` drew a "top sheen" with a rectangle
  only 55% of icon height. GDI+ left a hard 1px bright row at its bottom edge
  (row 560 of the 1024px master, scaled into every icon and icon.ico).
- Fixed the generator to use explicit blend stops on a full-height rectangle,
  so the sheen fades to transparent inside the shape with no seam.
- Repaired every shipped icon (all PNG sizes, icon.ico BMP entries, icon.icns
  PNG chunks) with `scripts/fix-icons.mjs`, then re-ran the fixed
  `generate-icons.ps1` + `build-icon.mjs` to confirm the generator now produces
  seamless icons with no repair step needed. `scripts/verify-icons.mjs` reports
  seam delta ~0 across PNG/ICO/ICNS; visually clean.

### 3. App laggy / slow to react
- Removed a second, dynamically-created "Files" view at the bottom of main.js
  that fought with render() every progress tick. Titlebar Library/Files tabs
  and the sidebar now share one `activeView` state via `setView()`.
- Files view: skip rebuilding the whole list (and reattaching listeners) on
  every 400ms progress event; rebuild only when the folder tree / statuses change.
- Patch rows in place without re-parsing status/action SVG unless the status
  actually changes (dataset markers).
- Collapsed five separate filter() scans + a second loop for counts into one
  `updateCountsAndStats()` pass.
- Removed a duplicate Escape key handler that called `window.close()` when no
  modal was found — pressing Escape could close the app.
- Styled the titlebar Library/Files tabs (they had no active state and were
  forced to 46px wide, clipping the labels).
- Review pass also found and fixed a pre-existing `folderOf()` bug: it searched
  for two consecutive backslashes, so Windows paths never split into folders in
  the Files view. Now single backslashes split correctly.

## Verification
- `npm run tauri build` succeeded (NSIS + MSI bundles); silent reinstall exit 0.
- Installed app launched (window title "Faast - Download Manager") and captured
  via PrintWindow: clean logo, styled Library/Files tabs, no visual regressions.
- `vite build` clean.
- `npm run e2e` — 62/62 pass.
- `cargo test --lib` — 52/52 pass.
- `cargo test --test engine_live` — 1/1 pass.
- `cargo test --test real_net` — 1/1 pass.
- `cargo test --test torrent_live` — flaky live-network diagnostic: failed once
  on a later run, passed on rerun (also passed 55s earlier). Not deterministic.
- `cargo check` clean.
- Browser checks: view switching + one files view (`verify-frontend.mjs`),
  mocked-IPC data test confirms folder grouping and file rows
  (`verify-data-views.mjs`), no console/page errors.

## FDM-inspired redesign (this pass)
- Full visual overhaul in `index.html` / `src/style.css` / `src/main.js`:
  FDM navy shell + cyan `#16a4fa` accent, card-based download rows, FDM-style
  20-block segmented progress bars, live speed sparkline, distinct torrent
  icon/tag, richer file icons, polished hover/focus/empty states.
- E2E color expectation for the primary button updated to the new accent.
- Design doc: `docs/superpowers/specs/2026-11-17-fdm-redesign-design.md`.

## Desktop icon cache
- The installed exe had the right icon but Windows showed the stale one:
  `IconCache.db` was from the previous day and Explorer's iconcache_48.db was
  older than the reinstall. Deleted all 16 icon-cache DBs and restarted
  Explorer; `SHGetFileInfo` confirms the desktop shortcut and exe now render
  the clean blue-arrow icon.
- User still reported a stale icon, so a hard refresh was run a second time:
  Explorer stopped, all 16 icon-cache DBs deleted again, both desktop and
  Start Menu `.lnk` files deleted and recreated with an explicit
  `IconLocation = "<exe>,0"`, `ie4uinit.exe -show` and `-ClearIconCache` run,
  then Explorer restarted. Both shortcut and exe resolve to the same system
  icon index (50). `scripts/refresh-desktop-icon.ps1` repeats this if needed.
- If a stale icon is still visible after this, only a sign-out/reboot clears
  the remaining shell icon cache layer.

## Not done (needs new services / out of scope)
- Browser download interception, site explorer, add-on store, remote downloads,
  per-torrent file priorities, per-download bandwidth cap. See the design doc.

## unslop + impeccable pass (last)
- Both skills installed (`heydamir/unslop`, `pbakaus/impeccable`). The
  impeccable launcher binary failed to download across three interrupted
  installs, so its documented manual fallback was used: read existing context,
  then `reference/craft-floor.md` immediately before editing.
- unslop audit → direction: exact, steady, workmanlike; ink-tinted neutrals;
  the one colored moment is the segmented progress blocks (signature of a
  segmented downloader). Nine-pass fixes applied:
  - 4px spacing grid; equal row/card padding; 240px sidebar.
  - Weights reduced to 400/500; sentence-case labels; no wide tracking.
  - Removed colored button glow, colored 3px active-row border, hover transforms,
            looping shimmer, animated width, decorative sparkline, gradient brand tile.
  - One icon system: lucide at 1.5 stroke; 16px icons; one aligned column.
  - Quiet surface steps for active states; hairline borders, not border+shadow.
  - Shorter copy/placeholders; ARIA labels on text inputs.
- impeccable craft/audit: currentColor focus rings, semantic ink tokens,
  32px icon targets, tokenized dark theme, verified contrast.
- E2E 62/62 after the pass; rebuilt + reinstalled.

## shadcn dark UI (last)
- Dark is now the default shell (`saved !== 'light'`); light remains opt-in.
- Dark tokens replaced with shadcn's zinc scale: `#09090b` surfaces, `#18181b`
  sidebar, `#27272a` hairlines, `#fafafa` text, `#a1a1aa` secondary text.
- Added `--primary` / `--primary-foreground` / `--primary-hover` tokens. In dark
  the primary button is near-white with dark text (shadcn + unslop guidance);
  blue stays only on the brand mark and the segmented progress moment.
- Neutralized hardcoded blue-tinted chrome text to zinc neutrals.
- E2E color expectations updated for the dark shell; 62/62 still pass.
- Rebuilt + reinstalled; installed app screenshot confirms the dark shell.

## Rollback
- No git available. Original (seamed) icons kept at `src-tauri/icons.bak`.
- `scripts/fix-icons.mjs` is idempotent and can be re-run any time.

## Public-readiness pass (2026-11-17)
- Wrote public README (reliability wedge, honest limitations), docs/BENCHMARKS.md, POSITIONING.md, INSTALL.md, CLI-DESIGN.md, TESTING.md.
- Added docs/launch/ drafts (show-hn, reddit, product-hunt) + packaging (scoop/winget manifests) + scripts/sign-installer.ps1.
- Exposed http_segment_count/split_ranges/download_segment as pub; added src-tauri/tests/benchmark.rs.
- Results: 1c=121MB/s, 8c=495MB/s, 16c=553MB/s, 32c=491MB/s (128MiB loopback, 4.6x max).
- Captured 3 UI frames (scripts/gif/*.png), encoded docs/assets/faast-demo.gif (3 frames, 1280x800) via gif-encoder-2 (devDep).
- Removed broken scripts/make-demo-gif.ps1; kept verify-gif.ps1. README embeds demo gif + static shot; fixed placeholder repo link.
- Verified: vite build clean, 52/52 unit, engine_live ok, cargo check --all-targets clean.
- git init'd repo, committed 8305c20 'v0.1.0: reliability-first download manager'. NOT yet pushed (needs public remote/URL).

## Push to GitHub (2026-11-17)
- Added remote: https://github.com/MinikLover67/Faast.git
- Merged unrelated histories (remote had Initial commit only), kept local LICENSE/README.md
- Pushed main branch to 0ea34ab
- All placeholder URLs now point to MinikLover67/Faast
- Next: create Release v0.1.0 with signed installer + assets, then update manifests with real SHA256.
