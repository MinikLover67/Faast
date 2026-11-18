# Faast — FDM-inspired redesign (approved approach: A + C, B where feasible)

## Intent
The user wants Faast to feel "100× better" and to take visual cues from Free
Download Manager (FDM). This is a frontend-first redesign of the existing,
working download engine — not a rewrite.

## What recon found
FDM 6.29's current design language (from freedownloadmanager.org):
- Deep navy navigation: `#102232` / `#1a3853`
- Bright cyan-blue accent: `#16a4fa` / `#0b84d6`
- Light canvas `#f4f7fb`, white cards, 16px radii, soft shadows
- Inter typography, muted text `#6a7885`, borders `#e6ecf2`
- Signature elements: **segmented progress blocks**, distinct torrent icons,
  prominent traffic/speed controls, dense but airy information layout

Faast already has segmented HTTP, torrents, scheduling, folder routing, speed
limits, and yt-dlp routing in the Rust backend. The gap is presentation and a
few convenience features.

## Goals
1. Replace the current light-gray Windows-11 shell with the FDM palette.
2. Rebuild download rows as richer cards with segmented progress blocks.
3. Add a live speed sparkline to the status bar.
4. Distinguish HTTP / torrent / magnet / yt-dlp rows with distinct icons.
5. Fold in "polish": hover states, focus rings, spacing, empty states, toasts.
6. Add feasible FDM-style conveniences: pause-all/resume-all already exist;
   extend with per-row priority (pause/resume), clearer speed-limit control,
   and a compact "active transfers" summary.

## Non-goals (and why)
- Real browser download interception: requires a Chrome/Firefox extension.
- Site explorer / add-on store: requires a new backend service.
- Remote downloads: requires a remote agent and accounts.
- Per-file torrent priorities: librqbit internals + risky in this pass.
These are recorded for future work; nothing here pretends to implement them.

## Constraints
- Vanilla JS + Vite + Tauri v2 WebView2; no framework, no new dependencies.
- Keep all existing IPC command names and the Rust engine untouched.
- All 62 E2E checks and 52 Rust unit tests must keep passing.

## Decisions (builder's calls)
- Scope to `index.html`, `src/style.css`, `src/main.js` only.
- Use CSS custom properties for the new palette so dark mode stays easy.
- Segmented progress: 20 blocks per row, patched in place on progress ticks.
- Sparkline: inline SVG polyline fed from a rolling array of recent speeds.
- Titlebar stays edge-to-edge (decorations:false); sidebar and topbar go navy.

## Delivered
- FDM palette + navy shell, cyan accent, light/dark themes via CSS variables.
- Download rows rebuilt as cards with FDM-style 20-block segmented progress.
- Live speed sparkline in the status bar.
- Distinct torrent/magnet icon and TORRENT tag; yt-dlp tag retained.
- Richer icons, hover/focus polish, card-style empty state.
- Pre-existing Windows `folderOf()` path bug fixed (Files view grouping).
- Desktop icon fixed by clearing Windows icon cache + restarting Explorer.

## Not delivered (explicitly out of scope)
- Browser download interception (needs a browser extension).
- Site explorer, add-on store, remote downloads (need new services).
- Per-torrent file priorities (librqbit internals, high risk).
- Per-download bandwidth cap UI (backend field exists but no IPC setter or
  engine-side limiter for HTTP/torrent; global cap only affects yt-dlp today).

## Verification
62/62 UI E2E, 52/52 Rust unit, engine_live/real_net/torrent_live integration,
vite build, tauri build (NSIS+MSI), silent reinstall, installed-app screenshot.
