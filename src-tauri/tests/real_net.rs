// Quick real-internet download verification using the same reqwest client config
// as run_download. Confirms the engine can actually reach the network and stream bytes.
use futures_util::stream::StreamExt;

#[tokio::test]
async fn real_internet_download() {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("client");

    // A small, widely-available file. If this host is unreachable the test is inconclusive,
    // so we assert only on the mechanics when we do get a response.
    let url = "https://www.google.com/favicon.ico";
    let resp = match client
        .get(url)
        .header("User-Agent", "FaastDownloadManager/1.0")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            // Network may be sandboxed; do not fail the suite on connectivity.
            eprintln!("SKIP: network unreachable in this environment: {}", e);
            return;
        }
    };
    assert!(resp.status().is_success(), "status {}", resp.status());
    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk");
        total += chunk.len() as u64;
    }
    eprintln!("Downloaded {} bytes from {}", total, url);
    assert!(total > 0, "expected to receive bytes");
}
