# Product Hunt draft (optional)

**Tagline (≤60 chars):**
Local-first download manager that resumes from part files

**Description:**
Faast is a free, open-source Windows download manager built around reliability. It splits HTTP files across parallel connections, writes each part to disk as it arrives, and resumes exactly where it stopped after a crash or pause. Torrents, magnets and yt-dlp links are supported too. No account, no ads, no telemetry, no browser extension to disconnect. Early alpha, Windows only.

**First maker comment:**
We built Faast after one too many stalled downloads with no explanation. The interesting part is the resume path: part files on disk are the source of truth, so a crash doesn't lose progress. We published the benchmark method too, including where 32 connections performs worse than 16. Happy to answer anything about the engine or the tradeoffs.

**Gallery notes:**
1. Dark UI on a populated download list (the screenshot in the README).
2. Segmented progress bars close-up — the one distinctive visual.
3. Benchmark table with the honest caveat line.
4. Add-download dialog with a magnet link.

**Do not:** ask for upvotes, use "10× faster", or claim cross-platform support.
