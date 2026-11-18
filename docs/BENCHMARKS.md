# Benchmarks

## What this measures

Faast's HTTP engine splits a file into contiguous byte ranges and downloads them in parallel. This benchmark calls the **real engine helpers** (`split_ranges` and `download_segment` from `faast_lib`) against a local HTTP server that generates a synthetic payload on demand and honours byte ranges. It measures the engine's own scaling behaviour — not your internet connection.

**Source:** `src-tauri/tests/benchmark.rs`
**Reproduce:** `cd src-tauri && cargo test --test benchmark -- --nocapture`

## Results

Environment: local loopback, 128 MiB synthetic payload, debug build, 8 worker threads.

| Connections | Elapsed | Throughput | Relative to 1 |
|---:|---:|---:|---:|
| 1 | 1.058 s | 121 MB/s | 1.0× |
| 8 | 0.259 s | 495 MB/s | 4.1× |
| 16 | 0.231 s | 553 MB/s | 4.6× |
| 32 | 0.261 s | 491 MB/s | 4.1× |

## How to read this

- **Segmentation works.** Eight connections deliver roughly four times the throughput of one, even on loopback where there is no network bottleneck — the gain comes from pipelining generation, streaming, and disk writes.
- **16 is the local sweet spot.** 32 connections add scheduling and file overhead and come out slightly slower. `http_segment_count` therefore scales 8 → 12 → 16 → 24 → 32 by file size; the 24/32 tiers exist for CDNs that throttle *each connection* to a fixed rate, where more sockets still win.
- **Real-world numbers will differ.** Server rate limits, latency, disk speed and peers dominate outside localhost. Your result depends on the host.

## What we deliberately do not claim

- No "10× faster than IDM" headline. We benchmarked our engine, not a competitor, on different hardware and a different server.
- No claim that more connections is always better — the table above shows the opposite at 32 on loopback.
- No claim that segmentation defeats a server-side global cap. It cannot.

## Future work

- Publish a repeatable internet-facing benchmark against a public test file with a documented server.
- Measure resume-after-crash overhead and merge time for very large files (40 GB+).
- Add a CI job that runs this benchmark and tracks regressions over time.
