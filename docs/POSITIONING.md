# Positioning

## The decision

Faast will not win by having more features than AB Download Manager (19k stars, browser extensions, four platforms) or by undercutting IDM on price (FDM is already free). The wedge is **reliability and trust**:

> **The download manager that keeps working when the link gets difficult.**

Every major competitor has a public complaint that fits this: FDM's pause/resume is called unreliable; AB DM's browser extension reports "Not Connected"; JDownloader's installer is remembered for adware; IDM is a paid nag. Faast has no extension to break, resumes from part files after a crash, writes honest errors instead of a stuck bar, and ships no ads, sign-up or telemetry.

## What we can say, with evidence

- "Segmented HTTP with resumable part files" — implemented and integration-tested.
- "4.6× throughput on our local 128 MiB benchmark" — see [BENCHMARKS.md](BENCHMARKS.md), with the full method.
- "No account, no ads, no telemetry" — true of the code today.
- "Free and open source under MIT" — true.
- "Modern dark interface" — subjective but visible in the screenshot.

## What we cannot say yet

- "Cross-platform" — Windows only.
- "Automatically captures browser downloads" — no extension.
- "10× faster" — no comparative benchmark.
- "Most reliable" — no independent reliability study.

## Competitor snapshot (research, 2026)

| Tool | Strength | Recurring complaint |
|---|---|---|
| IDM | Speed consistency, browser capture | Paid, nag screen, Windows-only, dated UI |
| FDM | Name recognition, all-in-one | "Horrible interface", unreliable pause/resume, bloat |
| JDownloader | 300+ plugins, power features | Adware installer, dated UI, cumbersome |
| AB DM | Modern UI, extensions, multiplatform | Extension connectivity errors |
| Gopeed | Very clean, simple UI | Younger ecosystem |
| Mister DM | Published benchmarks, 86 languages | Young project, many open issues |

## Messaging hierarchy

1. **Lead with the pain:** "Downloads that stall, pause buttons that don't resume, extensions that stop connecting."
2. **Then the product:** "Faast is a local-first Windows download manager that resumes from part files and tells you what went wrong."
3. **Then proof:** the benchmark table and the screenshot.
4. **Then honesty:** the limitations section in the README.

## Roadmap, in priority order

1. Trust layer: signed installer, winget + Scoop packaging, public repo with a real README. (Prepared in this repo; publishing needs credentials.)
2. Reliability proof: crash-resume test, better error messages, a public status page for known issues.
3. Wedge feature: CLI parity with the desktop engine — see [CLI-DESIGN.md](CLI-DESIGN.md).
4. Browser interception, cross-platform, per-torrent priorities — later, only after 1–3.

## Anti-goals

- No MCP control server in the product.
- No feature parity chase with AB DM before the trust layer exists.
- No paid promotion before the repo and first-run experience are polished.
