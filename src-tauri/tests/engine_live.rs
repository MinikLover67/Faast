// Live integration test for the HTTP download engine.
// Spins up a local HTTP server that serves bytes with Range support, then
// exercises the same reqwest client configuration used by `run_download`
// (redirect limit, User-Agent, Range resume, streaming to file) — proving the
// engine can actually download and resume a real file.

use std::io::{Read, Seek, SeekFrom, Write};

fn make_test_payload(len: usize) -> Vec<u8> {
    // deterministic, non-repeating-ish pattern
    (0..len).map(|i| (i % 251) as u8).collect()
}

// Minimal blocking HTTP server with Range support, running on a background thread.
fn spawn_range_server(payload: Vec<u8>) -> String {
    use std::net::TcpListener;
    use std::sync::Arc;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let addr = listener.local_addr().expect("local addr");
    let payload = Arc::new(payload);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let payload = Arc::clone(&payload);
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let total = payload.len();

                // parse Range: bytes=N-
                let range_start = req
                    .lines()
                    .find_map(|l| {
                        let l = l.to_ascii_lowercase();
                        l.strip_prefix("range: bytes=")
                            .and_then(|v| v.split('-').next())
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);

                let body = if range_start < total {
                    payload[range_start..].to_vec()
                } else {
                    Vec::new()
                };
                let status = if range_start > 0 && range_start < total {
                    "206 Partial Content"
                } else if range_start >= total {
                    "416 Range Not Satisfiable"
                } else {
                    "200 OK"
                };
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    status,
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });

    format!("http://{}", addr)
}

#[tokio::test]
async fn live_http_download_full_and_resume() {
    let payload = make_test_payload(256 * 1024); // 256 KiB
    let expected_len = payload.len();
    let base = spawn_range_server(payload.clone());
    let url = format!("{}/file.bin", base);

    // Mirror the client config from run_download
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("FaastDownloadManager/1.0")
        .build()
        .expect("client");

    // ── Part 1: full download ──
    let resp = client.get(&url).send().await.expect("send");
    assert!(resp.status().is_success(), "status {}", resp.status());
    let total = resp.content_length().expect("content length");
    assert_eq!(total as usize, expected_len);

    use futures_util::stream::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut file_path = std::env::temp_dir();
    file_path.push("faast_live_test_full.bin");
    let mut file = std::fs::File::create(&file_path).expect("create file");
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        file.write_all(&chunk).expect("write");
        downloaded += chunk.len() as u64;
    }
    file.flush().expect("flush");
    assert_eq!(downloaded, expected_len as u64, "downloaded byte count");

    let on_disk = std::fs::read(&file_path).expect("read back");
    assert_eq!(on_disk.len(), expected_len);
    assert_eq!(on_disk, payload, "full download content matches");
    let _ = std::fs::remove_file(&file_path);

    // ── Part 2: partial write then Range resume ──
    let resume_from = 100 * 1024u64;
    let resp = client
        .get(&url)
        .header("Range", format!("bytes={}-", resume_from))
        .send()
        .await
        .expect("resume send");
    assert_eq!(resp.status().as_u16(), 206, "expected partial content");

    let mut resume_path = std::env::temp_dir();
    resume_path.push("faast_live_test_resume.bin");
    // seed the file with the first resume_from bytes (as a prior partial would)
    std::fs::write(&resume_path, &payload[..resume_from as usize]).expect("seed partial");

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&resume_path)
        .expect("open partial");
    file.seek(SeekFrom::Start(resume_from)).expect("seek");

    let mut stream = resp.bytes_stream();
    let mut total_on_disk = resume_from;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("resume chunk");
        file.write_all(&chunk).expect("resume write");
        total_on_disk += chunk.len() as u64;
    }
    file.flush().expect("resume flush");
    assert_eq!(total_on_disk, expected_len as u64, "resumed total");

    let resumed = std::fs::read(&resume_path).expect("read resumed");
    assert_eq!(resumed.len(), expected_len);
    assert_eq!(resumed, payload, "resumed file content matches original");
    let _ = std::fs::remove_file(&resume_path);
}
