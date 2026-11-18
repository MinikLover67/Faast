# Reddit drafts

General: follow each sub's 90/10 rule (mostly helpful contributions, occasionally your own project). Do not ask for upvotes. Repost only when there is genuinely new to show.

## r/software

**Title:** I built a small open-source download manager that resumes from part files after a crash

**Body:**
I've been working on Faast, a free Windows download manager, because the ones I used kept failing in small ways — stalled transfers with no error, pause/resume that lost progress, extensions that disconnected.

It uses parallel Range connections for HTTP files, writes each part to disk, and resumes from the parts after a crash. Torrents and yt-dlp links work too. No account, no ads, no telemetry.

It's v0.1, Windows-only, no browser extension yet. Repo and screenshots: <url>. What reliability issue annoys you most about current managers?

## r/Windows11

**Title:** Free, open-source download manager with a native Win11-style dark UI

**Body:**
Been building this as a native Tauri app; the goal was a download manager that looks like it belongs on Windows 11 instead of 2009. Has segmented HTTP downloads with crash-safe resume, torrents, scheduled starts and folder routing.

v0.1, Windows only, unsigned installer for now (SmartScreen will warn). <url>. Feedback on the UI especially welcome.

## r/opensource

**Title:** Faast – MIT-licensed Windows download manager, looking for first contributors

**Body:**
Faast is a Tauri v2 + Rust download manager I started because I wanted something local-first and transparent. 52 unit tests, a Playwright E2E suite, and a published local benchmark with its method.

Areas where help would matter most: code-signing guidance, a browser-extension design that doesn't break, and Linux packaging. <url>
