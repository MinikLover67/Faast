# Show HN draft

**Title:**
Show HN: Faast – a local-first Windows download manager that resumes from part files

**Body:**

I built Faast because I got tired of three things: downloads that stall without an explanation, pause buttons that don't actually resume, and browser extensions that silently stop connecting to the desktop app.

Faast is a Windows desktop app (Tauri v2, Rust backend, vanilla JS frontend) that downloads HTTP files over parallel Range connections, writes each part to disk as it arrives, and resumes from those parts after a crash or pause. It also handles magnets/torrents with librqbit and video/audio links through yt-dlp.

The part I'm proudest of is boring: when a transfer fails, the row says what happened and offers a retry. There is no extension to go "Not Connected" — you paste a link or drop a .torrent on the window.

On a local 128 MiB benchmark, 16 connections are about 4.6× faster than one. I published the method and the caveats rather than a marketing number: https://github.com/MinikLover67/Faast/blob/main/docs/BENCHMARKS.md

It's early alpha and Windows-only. There's no browser integration yet, and the global speed limit currently applies only to yt-dlp downloads. I'd rather say that up front.

Repo (with screenshots): https://github.com/MinikLover67/Faast

Happy to answer questions about the segmented download engine, resume behaviour, or why I chose Tauri.
