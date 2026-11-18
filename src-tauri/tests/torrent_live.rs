//! Live diagnostic for the torrent engine (librqbit).
//!
//! Defaults to the webtorrent.io Big Buck Bunny torrent, which is well seeded.
//! Run with:
//!   $env:RUST_LOG="librqbit=info"; cargo test --test torrent_live -- --nocapture
//! Override with FAAS_TORRENT / FAAS_OUT / FAAS_STATE / FAAS_MAX_SECS.

use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn torrent_reports_peers_and_progress() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let torrent_path =
        std::env::var("FAAS_TORRENT").unwrap_or_else(|_| r"C:\tmp\opencode\bbb.torrent".to_string());
    let torrent_path = std::path::Path::new(&torrent_path);
    if !torrent_path.exists() {
        eprintln!("SKIP: torrent file missing at {:?}", torrent_path);
        return;
    }
    let data = std::fs::read(torrent_path).expect("read torrent");

    let out = std::path::PathBuf::from(
        std::env::var("FAAS_OUT").unwrap_or_else(|_| r"C:\tmp\opencode\torrent_out".to_string()),
    );
    std::fs::create_dir_all(&out).expect("create out dir");

    let state_dir = std::path::PathBuf::from(
        std::env::var("FAAS_STATE").unwrap_or_else(|_| r"C:\tmp\opencode\torrent_state".to_string()),
    );
    std::fs::create_dir_all(&state_dir).expect("create state dir");

    let max_secs: u64 = std::env::var("FAAS_MAX_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(90);

    let session = librqbit::Session::new_with_opts(
        state_dir,
        librqbit::SessionOptions {
            listen: Some(librqbit::ListenerOptions {
                mode: librqbit::ListenerMode::TcpAndUtp,
                ipv4_only: true,
                ..Default::default()
            }),
            ipv4_only: true,
            peer_limit: Some(200),
            client_name_and_version: Some("Faast 0.1.0".to_string()),
            ..Default::default()
        },
    )
    .await
    .expect("session");

    let response = session
        .add_torrent(
            librqbit::AddTorrent::from_bytes(data),
            Some(librqbit::AddTorrentOptions {
                output_folder: Some(out.to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .expect("add torrent");
    let handle: Arc<librqbit::ManagedTorrent> = response.into_handle().expect("torrent handle");

    let started = Instant::now();
    let mut last_report = Instant::now();
    let mut best_peers = 0u32;
    let mut best_speed = 0u64;

    loop {
        let stats = handle.stats();
        let peers = stats
            .live
            .as_ref()
            .map(|l| l.snapshot.peer_stats.live)
            .unwrap_or(0);
        let speed = stats
            .live
            .as_ref()
            .map(|l| l.download_speed.as_bytes())
            .unwrap_or(0);
        best_peers = best_peers.max(peers);
        best_speed = best_speed.max(speed);

        if last_report.elapsed() >= Duration::from_secs(2) {
            println!(
                "t={:>3}s state={} name={:?} progress={:.2}% peers={} speed={:.1} KB/s downloaded={} size={} err={:?}",
                started.elapsed().as_secs(),
                stats.state,
                handle.name(),
                stats.progress_bytes as f64 / stats.total_bytes.max(1) as f64 * 100.0,
                peers,
                speed as f64 / 1024.0,
                stats.progress_bytes,
                stats.total_bytes,
                stats.error,
            );
            last_report = Instant::now();
        }

        if stats.finished || started.elapsed() > Duration::from_secs(max_secs) {
            println!(
                "FINAL after {:?}: finished={} downloaded={} / {} best_peers={} best_speed={:.1} KB/s",
                started.elapsed(),
                stats.finished,
                stats.progress_bytes,
                stats.total_bytes,
                best_peers,
                best_speed as f64 / 1024.0
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let stats = handle.stats();
    assert!(stats.total_bytes > 0, "torrent metadata was never resolved");
    assert!(best_peers > 0, "engine never established a live peer");
    assert!(
        stats.progress_bytes > 0,
        "engine reported peers but downloaded no bytes"
    );
}
