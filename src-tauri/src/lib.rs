use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String,
    pub url: String,
    pub file_name: String,
    pub save_path: String,
    pub status: String,
    pub percent: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bps: Option<f64>,
    pub error: Option<String>,
    pub created_at: f64,
    pub category: String,
    /// Engine that owns this download: "http" | "torrent" | "magnet" | "ytdlp".
    pub source_kind: String,
    /// If set, the download waits until this epoch-ms before starting (scheduler).
    pub start_at: Option<f64>,
    /// Per-download byte-rate cap (bytes/sec). `None` = unlimited.
    pub speed_limit_bps: Option<f64>,
    /// Auto-detected content type for folder organization.
    pub content_type: String,
    /// User-chosen output format for yt-dlp downloads ("best", "1080p",
    /// "720p", "480p", "mp3", "wav", "m4a", "flac", "opus"). Ignored by the
    /// HTTP and torrent engines.
    pub media_format: String,
}

impl DownloadItem {
    fn new(
        id: String,
        url: String,
        file_name: String,
        save_path: String,
        created_at: f64,
    ) -> Self {
        Self {
            id,
            url,
            file_name,
            save_path,
            status: "downloading".to_string(),
            percent: 0,
            downloaded_bytes: 0,
            total_bytes: None,
            speed_bps: None,
            error: None,
            created_at,
            category: "document".to_string(),
            source_kind: "http".to_string(),
            start_at: None,
            speed_limit_bps: None,
            content_type: "other".to_string(),
            media_format: "best".to_string(),
        }
    }
}

pub struct AppState {
    downloads: Mutex<HashMap<String, DownloadItem>>,
    handles: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
    /// One shared librqbit session (DHT / trackers / listener) for every torrent.
    torrent_session: Mutex<Option<Arc<librqbit::Session>>>,
    /// Serializes first-time session creation so two quick adds can't race.
    torrent_init: Arc<tokio::sync::Mutex<()>>,
    /// Live librqbit handles, kept so pause/resume/delete reach the real torrent.
    torrent_handles: Mutex<HashMap<String, Arc<librqbit::ManagedTorrent>>>,
    /// Global byte-rate cap (bytes/sec) applied to HTTP + yt-dlp downloads.
    speed_limit_global: Mutex<Option<f64>>,
    /// Live yt-dlp child processes so pause/resume can kill them.
    yt_dlp_children: Mutex<HashMap<String, Arc<YtDlpSlot>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            downloads: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            torrent_session: Mutex::new(None),
            torrent_init: Arc::new(tokio::sync::Mutex::new(())),
            torrent_handles: Mutex::new(HashMap::new()),
            speed_limit_global: Mutex::new(None),
            yt_dlp_children: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub status: String,
    pub percent: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bps: Option<f64>,
    pub error: Option<String>,
    /// Filled in once torrent metadata reveals the real name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

fn make_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Milliseconds alone collide when several downloads are added quickly
    // (multi-file drag-drop, paste + Enter), which overwrote rows.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("dl-{ms}-{seq}")
}

fn make_created_at() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64
}

fn sanitize_file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Windows silently refuses/redirects device names such as CON, NUL or COM1.
    let stem = clean.split('.').next().unwrap_or("").to_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    // Windows also strips trailing dots/spaces, which would desync the path we
    // recorded from the path the OS actually creates.
    let clean = clean.trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() || clean == "." || clean == ".." || clean == "/" || reserved {
        "download.bin".to_string()
    } else {
        clean
    }
}

fn file_name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // `My%20File.zip` should land as `My_File.zip`, not `My_20File.zip`.
    let name = if url.trim().to_lowercase().starts_with("http") {
        percent_decode(name)
    } else {
        name.to_string()
    };
    sanitize_file_name(&name)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Best-effort display name from a magnet's `dn` parameter.
fn magnet_display_name(uri: &str) -> Option<String> {
    let query = uri.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("dn=") {
            let name = percent_decode(value);
            // In a query string `+` encodes a space.
            let name = name.replace('+', " ");
            if let Some(name) = clean_display_name(&name) {
                return Some(name);
            }
        }
    }
    None
}

/// Torrent names are attacker-controlled: drop control characters so they
/// can't break out of a label or inject extra rows in the UI.
fn clean_display_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// A name safe to append to a directory while keeping it readable (spaces are
/// preserved, unlike `sanitize_file_name`, so the row shows the real name).
fn safe_path_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    let trimmed = out.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        "torrent".to_string()
    } else {
        trimmed.to_string()
    }
}

fn category_for(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let category = match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg"
        | "m2ts" | "ts" => "video",
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "aiff" => "music",
        "exe" | "msi" | "msix" | "appx" | "zip" | "7z" | "rar" | "tar" | "gz" | "iso" | "apk"
        | "dmg" | "pkg" | "deb" | "rpm" => "app",
        _ => "document",
    };
    category.to_string()
}

/// One user-configurable routing rule. Rules are evaluated in order and the
/// first enabled match wins, so more specific rules go above generic ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRule {
    /// "extension" matches the file extension, "keyword" matches anywhere in
    /// the name. Both are case-insensitive.
    pub kind: String,
    pub pattern: String,
    /// Subfolder created under the root. Empty = leave in the root.
    pub folder: String,
    pub enabled: bool,
}

/// Where downloads land when the user has not explicitly picked a folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingConfig {
    /// Master switch. When off, everything goes straight to the root folder.
    pub enabled: bool,
    /// Absolute root. Empty = `<Documents>\Faast-Downloads`.
    pub root: String,
    pub rules: Vec<RouteRule>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        // Ordered: specific before generic. Torrents resolve against the
        // torrent's display name, so keyword rules are how a game gets sorted
        // into Games without needing a file extension.
        let rule = |kind: &str, pattern: &str, folder: &str| RouteRule {
            kind: kind.to_string(),
            pattern: pattern.to_string(),
            folder: folder.to_string(),
            enabled: true,
        };
        Self {
            enabled: true,
            root: String::new(),
            rules: vec![
                rule("keyword", "game", "Games"),
                rule("extension", "iso", "Software"),
                rule("extension", "exe", "Software"),
                rule("extension", "msi", "Software"),
                rule("extension", "zip", "Software"),
                rule("extension", "7z", "Software"),
                rule("extension", "rar", "Software"),
                rule("extension", "mp4", "Video"),
                rule("extension", "mkv", "Video"),
                rule("extension", "avi", "Video"),
                rule("extension", "mov", "Video"),
                rule("extension", "webm", "Video"),
                rule("extension", "mp3", "Music"),
                rule("extension", "flac", "Music"),
                rule("extension", "wav", "Music"),
                rule("extension", "m4a", "Music"),
                rule("extension", "opus", "Music"),
                rule("extension", "pdf", "Documents"),
                rule("extension", "epub", "Documents"),
                rule("extension", "docx", "Documents"),
                rule("extension", "html", "Documents"),
            ],
        }
    }
}

const ROUTING_FILE: &str = "routing.json";

fn routing_file_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(ROUTING_FILE))
}

fn load_routing(app: &tauri::AppHandle) -> RoutingConfig {
    let Some(path) = routing_file_path(app) else {
        return RoutingConfig::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<RoutingConfig>(&raw).ok())
        .unwrap_or_default()
}

fn save_routing(app: &tauri::AppHandle, cfg: &RoutingConfig) -> Result<(), String> {
    let path = routing_file_path(app).ok_or("Could not resolve the config folder")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create the config folder: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(cfg)
        .map_err(|e| format!("Could not serialise routing rules: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("Could not save routing rules: {e}"))
}

/// Picks the subfolder for a name by walking the rules in order.
/// Pure so it can be unit-tested without a tauri handle.
fn route_subfolder(cfg: &RoutingConfig, name: &str) -> String {
    if !cfg.enabled {
        return String::new();
    }
    let lower = name.to_lowercase();
    // A name with no dot (typical for a torrent folder name) has no extension,
    // so the whole-name fallback would mis-compare against the last segment.
    let ext = match lower.rfind('.') {
        Some(i) if i + 1 < lower.len() => lower[i + 1..].to_string(),
        _ => String::new(),
    };
    for r in &cfg.rules {
        if !r.enabled || r.pattern.trim().is_empty() {
            continue;
        }
        let pattern = r.pattern.trim().to_lowercase();
        let hit = match r.kind.as_str() {
            "extension" => !ext.is_empty() && ext == pattern,
            "keyword" => lower.contains(&pattern),
            _ => false,
        };
        if hit {
            let folder = r.folder.trim();
            if !folder.is_empty() {
                return safe_path_component(folder);
            }
            return String::new();
        }
    }
    String::new()
}

/// Root folder for downloads when the user has not chosen one explicitly.
/// Defaults to `<Documents>\Faast-Downloads` so downloads stay in one
/// predictable place instead of scattering through the system Downloads dir.
fn default_root_dir(app: &tauri::AppHandle, cfg: &RoutingConfig) -> String {
    if !cfg.root.trim().is_empty() {
        return cfg.root.trim().trim_end_matches(['/', '\\']).to_string();
    }
    app.path()
        .document_dir()
        .map(|d| d.join("Faast-Downloads").to_string_lossy().to_string())
        .unwrap_or_else(|_| "Faast-Downloads".to_string())
}

/// Resolves the directory a download is written into. An explicit choice from
/// the "Changeâ€¦" button always wins; routing is a default, not an override.
fn resolve_save_dir(
    app: &tauri::AppHandle,
    cfg: &RoutingConfig,
    explicit: Option<&str>,
    name: &str,
) -> String {
    if let Some(dir) = explicit.map(str::trim).filter(|d| !d.is_empty()) {
        return dir.trim_end_matches(['/', '\\']).to_string();
    }
    let root = default_root_dir(app, cfg);
    match route_subfolder(cfg, name) {
        sub if sub.is_empty() => root,
        sub => format!("{root}\\{sub}"),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SourceKind {
    Magnet,
    RemoteTorrent,
    LocalTorrent,
    Http,
    YtDlp,
}

/// Video / audio sites that yt-dlp can handle. When the user pastes one of
/// these, Faast routes it to yt-dlp instead of a raw HTTP GET (which would
/// just save an HTML page). yt-dlp supports thousands more sites; this list
/// covers the common ones and can be extended.
const YT_DLP_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "www.youtu.be",
    "vimeo.com",
    "www.vimeo.com",
    "dailymotion.com",
    "www.dailymotion.com",
    "facebook.com",
    "www.facebook.com",
    "watch.facebook.com",
    "instagram.com",
    "www.instagram.com",
    "tiktok.com",
    "www.tiktok.com",
    "twitch.tv",
    "www.twitch.tv",
    "soundcloud.com",
    "www.soundcloud.com",
    "bandcamp.com",
    "www.bandcamp.com",
    "bilibili.com",
    "www.bilibili.com",
    "reddit.com",
    "www.reddit.com",
    "streamable.com",
    "www.streamable.com",
    "imgur.com",
    "www.imgur.com",
];

/// True when `url` points at a known video/audio site that yt-dlp supports.
fn is_yt_dlp_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    // Extract the host from the URL. Handles http://, https://, and
    // protocol-relative //host forms. Falls back to a substring check for
    // anything that doesn't parse cleanly.
    let host = lower
        .split("://")
        .nth(1)
        .unwrap_or(&lower)
        .split('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches("www.");
    if host.is_empty() {
        return false;
    }
    // Exact host match, or a subdomain of a known host (e.g. m.youtube.com).
    YT_DLP_HOSTS.iter().any(|h| {
        let h = h.trim_start_matches("www.");
        host == h || host.ends_with(&format!(".{}", h))
    })
}

/// Classifies and validates a user-supplied download source up front so the UI
/// can show a clear error instead of a failed row after a pointless request.
fn classify_source(url: &str) -> Result<SourceKind, String> {
    let lower = url.trim().to_lowercase();
    if lower.starts_with("magnet:") {
        return Ok(SourceKind::Magnet);
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let path = lower.split(['?', '#']).next().unwrap_or(&lower);
        if path.ends_with(".torrent") {
            return Ok(SourceKind::RemoteTorrent);
        }
        if is_yt_dlp_url(url) {
            return Ok(SourceKind::YtDlp);
        }
        return Ok(SourceKind::Http);
    }
    if lower.starts_with("ftp://") {
        return Err(
            "FTP links aren't supported yet â€” use an HTTP(S) link, magnet, or .torrent file."
                .to_string(),
        );
    }
    if lower.ends_with(".torrent") {
        return Ok(SourceKind::LocalTorrent);
    }
    Err("Unsupported link. Use an HTTP(S) URL, magnet link, or .torrent file.".to_string())
}

fn compute_percent(downloaded: u64, total: Option<u64>) -> u32 {
    match total {
        Some(t) if t > 0 => ((downloaded as f64 / t as f64) * 100.0).clamp(0.0, 100.0) as u32,
        _ => 0,
    }
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse::<u64>().ok()
}

#[derive(Debug, PartialEq)]
struct ResumePlan {
    start_at: u64,
    total: Option<u64>,
    truncate: bool,
}

/// Decides where to write when resuming. If we asked for a range but the server
/// sent the whole file (200), the partial bytes are useless â€” restart at zero.
fn plan_resume(
    resume_from: u64,
    status: reqwest::StatusCode,
    content_len: Option<u64>,
    content_range: Option<&str>,
) -> ResumePlan {
    let partial = resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if partial {
        let total = content_range
            .and_then(parse_content_range_total)
            .or_else(|| content_len.map(|len| resume_from + len));
        ResumePlan {
            start_at: resume_from,
            total,
            truncate: false,
        }
    } else {
        ResumePlan {
            start_at: 0,
            total: content_len,
            truncate: true,
        }
    }
}

/// How many parallel Range connections a big HTTP download opens. A single
/// TCP stream is often capped by the server or path (~150 KB/s on many CDNs);
/// spreading the file over several connections uses the whole pipe. Larger
/// files get more connections so we don't open 16 sockets for a 1 MB file.
pub fn http_segment_count(total: u64) -> usize {
    if total >= 20 << 30 {
        // 20+ GB (e.g. a 40 GB game installer): many CDNs throttle each
        // connection to ~100 KB/s. 32 parallel ranges is the difference between
        // an all-night download and one that finishes in minutes.
        32
    } else if total >= 8 << 30 {
        24
    } else if total >= 1 << 32 {
        16
    } else if total >= 1 << 30 {
        12
    } else {
        8
    }
}
/// Below this size the extra requests cost more than they gain.
const HTTP_SEGMENT_MIN_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive.
    pub start: u64,
    /// Inclusive.
    pub end: u64,
}

impl ByteRange {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Splits `total` bytes into at most `count` contiguous, inclusive ranges.
pub fn split_ranges(total: u64, count: usize) -> Vec<ByteRange> {
    let count = count.max(1) as u64;
    if total == 0 {
        return Vec::new();
    }
    let chunk = total.div_ceil(count);
    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < total {
        let end = start.saturating_add(chunk - 1).min(total - 1);
        ranges.push(ByteRange { start, end });
        start = end + 1;
    }
    ranges
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentMeta {
    total: u64,
    count: usize,
}

fn segment_meta_path(save_path: &str) -> String {
    format!("{}.faast-meta", save_path)
}

fn segment_part_path(save_path: &str, index: usize) -> String {
    format!("{}.faast-part{}", save_path, index)
}

fn read_segment_meta(save_path: &str) -> Option<SegmentMeta> {
    let raw = std::fs::read_to_string(segment_meta_path(save_path)).ok()?;
    let meta: SegmentMeta = serde_json::from_str(&raw).ok()?;
    if meta.count == 0 || meta.total == 0 {
        return None;
    }
    Some(meta)
}

fn write_segment_meta(save_path: &str, meta: SegmentMeta) -> std::io::Result<()> {
    let raw = serde_json::to_string(&meta).unwrap_or_default();
    std::fs::write(segment_meta_path(save_path), raw)
}

async fn remove_segment_artifacts(save_path: &str) {
    let _ = tokio::fs::remove_file(segment_meta_path(save_path)).await;
    // The metadata may already be gone; sweep a generous, fixed range too so
    // leftovers from an interrupted earlier attempt can't survive a remove.
    for i in 0..128 {
        let _ = tokio::fs::remove_file(segment_part_path(save_path, i)).await;
    }
}

fn patch_item(state: &tauri::State<'_, Mutex<AppState>>, id: &str, patch: impl FnOnce(&mut DownloadItem)) {
    let app_state = state.lock().unwrap();
    let mut map = app_state.downloads.lock().unwrap();
    if let Some(item) = map.get_mut(id) {
        patch(item);
    }
}

#[derive(Debug, Clone)]
enum TorrentSource {
    Magnet(String),
    TorrentFile(String),
    TorrentUrl(String),
}

/// Marks a download failed, reports it to the UI, and forgets its task handle.
fn fail_download(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, Mutex<AppState>>,
    id: &str,
    error: String,
    downloaded: u64,
    total: Option<u64>,
) {
    let percent = compute_percent(downloaded, total);
    patch_item(state, id, |it| {
        it.status = "failed".to_string();
        it.percent = percent;
        it.downloaded_bytes = downloaded;
        it.total_bytes = total;
        it.speed_bps = None;
        it.error = Some(error.clone());
    });
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.to_string(),
            status: "failed".to_string(),
            percent,
            downloaded_bytes: downloaded,
            total_bytes: total,
            speed_bps: None,
            error: Some(error),
            file_name: None,
        },
    );
    forget_handle(state, id);
}

fn forget_handle(state: &tauri::State<'_, Mutex<AppState>>, id: &str) {
    let app_state = state.lock().unwrap();
    let mut handles = app_state.handles.lock().unwrap();
    handles.remove(id);
}

/// True when the row was paused or removed while the task was still running.
/// Without this, an in-flight progress emit could resurrect a paused row.
fn should_stop(state: &tauri::State<'_, Mutex<AppState>>, id: &str) -> bool {
    let app_state = state.lock().unwrap();
    let map = app_state.downloads.lock().unwrap();
    match map.get(id) {
        Some(item) => item.status == "paused" || item.status == "failed" || item.status == "stopped",
        None => true,
    }
}


#[tauri::command]
fn list_downloads(state: State<'_, Mutex<AppState>>) -> Vec<DownloadItem> {
    let app_state = state.lock().unwrap();
    let map = app_state.downloads.lock().unwrap();
    let mut items: Vec<DownloadItem> = map.values().cloned().collect();
    items.sort_by(|a, b| {
        a.created_at
            .partial_cmp(&b.created_at)
            .unwrap()
            .then(a.id.cmp(&b.id))
    });
    items
}

#[tauri::command]
fn get_download(id: String, state: State<'_, Mutex<AppState>>) -> Option<DownloadItem> {
    let app_state = state.lock().unwrap();
    let map = app_state.downloads.lock().unwrap();
    map.get(&id).cloned()
}

/// Reads clipboard text natively. `navigator.clipboard.readText()` in WebView2
/// silently rejects whenever the document isn't focused, which made the
/// "copy link -> Add download" flow unreliable.
#[tauri::command]
fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, OpenClipboard,
        };
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

        // CF_UNICODETEXT
        const CF_UNICODETEXT: u32 = 13;
        const MAX_CHARS: usize = 1 << 20;

        unsafe {
            if OpenClipboard(std::ptr::null_mut()) == 0 {
                return None;
            }
            let handle = GetClipboardData(CF_UNICODETEXT);
            let mut text = None;
            if !handle.is_null() {
                let ptr = GlobalLock(handle) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0usize;
                    while len < MAX_CHARS && *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    text = Some(String::from_utf16_lossy(slice));
                    GlobalUnlock(handle);
                }
            }
            CloseClipboard();
            text
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[tauri::command]
async fn pause_download(
    app: tauri::AppHandle,
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    // Never hold a std::sync guard across an await: clone what we need, then
    // drop every lock before touching the torrent session.
    let (task, torrent, session, slot, item) = {
        let app_state = state.lock().unwrap();
        {
            let mut map = app_state.downloads.lock().unwrap();
            if let Some(item) = map.get_mut(&id) {
                if item.status == "downloading" {
                    item.status = "paused".to_string();
                    item.speed_bps = Some(0.0);
                }
            }
        }
        let task = app_state.handles.lock().unwrap().remove(&id);
        let torrent = app_state.torrent_handles.lock().unwrap().get(&id).cloned();
        let session = app_state.torrent_session.lock().unwrap().clone();
        let slot = app_state.yt_dlp_children.lock().unwrap().get(&id).cloned();
        let item = {
            let map = app_state.downloads.lock().unwrap();
            map.get(&id).cloned()
        };
        (task, torrent, session, slot, item)
    };
    if let Some(task) = task {
        task.abort();
        // Wait for the task to unwind so its open part-file handles are closed
        // before any later write (resume) or delete (remove) touches them.
        let _ = task.await;
    }
    // yt-dlp downloads own a live child process: ask it to stop, the task will
    // kill the process on its next progress tick.
    if let Some(slot) = slot {
        slot.abort.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let (Some(session), Some(torrent)) = (session, torrent) {
        // Stops all peer traffic; the partial data stays on disk for resume.
        let _ = session.pause(&torrent).await;
    }
    // Guarantee the frontend's last event for this id reflects the paused state,
    // even if the aborted progress reporter emitted a late "downloading" tick.
    if let Some(item) = item {
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                id: item.id.clone(),
                status: "paused".to_string(),
                percent: item.percent,
                downloaded_bytes: item.downloaded_bytes,
                total_bytes: item.total_bytes,
                speed_bps: Some(0.0),
                error: None,
                file_name: Some(item.file_name.clone()),
            },
        );
    }
    Ok(())
}

#[tauri::command]
async fn resume_download(
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<DownloadItem, String> {
    let item = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id).cloned().ok_or("Download not found")?
    };
    if item.status != "paused" && item.status != "failed" && item.status != "stopped" {
        return Ok(item);
    }

    let kind = classify_source(&item.url)?;

    // Trust the bytes actually on disk (an aborted write may be shorter than the
    // last reported progress), falling back to the recorded count.
    let disk_len = std::fs::metadata(&item.save_path).ok().map(|m| m.len());
    let resume_from = if kind == SourceKind::Http {
        disk_len.unwrap_or(item.downloaded_bytes)
    } else {
        item.downloaded_bytes
    };

    // Already fully on disk: asking for `Range: bytes=<total>-` returns 416 and
    // used to leave the row failed even though the file was complete.
    if kind == SourceKind::Http {
        if let (Some(total), Some(len)) = (item.total_bytes, disk_len) {
            if total > 0 && len >= total {
                patch_item(&state, &id, |it| {
                    it.status = "completed".to_string();
                    it.percent = 100;
                    it.downloaded_bytes = len;
                    it.total_bytes = Some(total);
                    it.speed_bps = Some(0.0);
                    it.error = None;
                });
                let _ = app.emit(
                    "download-progress",
                    DownloadProgress {
                        id: id.clone(),
                        status: "completed".to_string(),
                        percent: 100,
                        downloaded_bytes: len,
                        total_bytes: Some(total),
                        speed_bps: Some(0.0),
                        error: None,
                        file_name: None,
                    },
                );
                let done = {
                    let app_state = state.lock().unwrap();
                    let map = app_state.downloads.lock().unwrap();
                    map.get(&id).cloned()
                };
                return done.ok_or_else(|| "Download not found".to_string());
            }
        }
    }

    // A paused torrent is still registered in the shared librqbit session.
    // Unpause that instance instead of adding a second copy of the torrent.
    if kind != SourceKind::Http {
        let existing = {
            let app_state = state.lock().unwrap();
            let torrent = app_state.torrent_handles.lock().unwrap().get(&id).cloned();
            let session = app_state.torrent_session.lock().unwrap().clone();
            torrent.zip(session)
        };
        if let Some((torrent, session)) = existing {
            let _ = session.unpause(&torrent).await;
            {
                let app_state = state.lock().unwrap();
                let mut map = app_state.downloads.lock().unwrap();
                if let Some(it) = map.get_mut(&id) {
                    it.status = "downloading".to_string();
                    it.error = None;
                    it.speed_bps = None;
                }
            }
            let task = tauri::async_runtime::spawn(poll_torrent(
                app.clone(),
                id.clone(),
                torrent,
                session,
                save_dir_of(&item.save_path),
            ));
            {
                let app_state = state.lock().unwrap();
                app_state.handles.lock().unwrap().insert(id.clone(), task);
            }
            let updated = {
                let app_state = state.lock().unwrap();
                let map = app_state.downloads.lock().unwrap();
                map.get(&id).cloned()
            };
            return updated.ok_or_else(|| "Download not found".to_string());
        }
    }

    {
        let app_state = state.lock().unwrap();
        let mut map = app_state.downloads.lock().unwrap();
        if let Some(it) = map.get_mut(&id) {
            it.status = "downloading".to_string();
            it.downloaded_bytes = resume_from;
            it.error = None;
            it.speed_bps = None;
        }
    }

    let url = item.url.clone();
    let save_path = item.save_path.clone();
    let total_bytes = item.total_bytes;

    let handle = match kind {
        SourceKind::Magnet => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::Magnet(url),
            save_path.clone(),
        )),
        SourceKind::LocalTorrent => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::TorrentFile(url),
            save_path.clone(),
        )),
        SourceKind::RemoteTorrent => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::TorrentUrl(url),
            save_path.clone(),
        )),
        SourceKind::Http => tauri::async_runtime::spawn(run_download(
            app.clone(),
            id.clone(),
            url,
            save_path,
            resume_from,
            total_bytes,
        )),
        SourceKind::YtDlp => tauri::async_runtime::spawn(yt_dlp_download(
            app.clone(),
            id.clone(),
            url,
            save_path,
        )),
    };
    {
        let app_state = state.lock().unwrap();
        let mut handles = app_state.handles.lock().unwrap();
        handles.insert(id.clone(), handle);
    }
    let updated = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id).cloned()
    };
    updated.ok_or_else(|| "Download not found".to_string())
}

fn save_dir_of(save_path: &str) -> String {
    std::path::Path::new(save_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| save_path.to_string())
}

/// Picks a save path that doesn't clobber an existing file or another queued
/// download â€” e.g. two links whose URL ends in `/` both became `download.bin`.
fn unique_save_path(dir: &str, file_name: &str, taken: &[String]) -> String {
    let candidate = format!("{}/{}", dir, file_name);
    let free = |p: &str| !std::path::Path::new(p).exists() && !taken.iter().any(|t| t == p);
    if free(&candidate) {
        return candidate;
    }
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), format!(".{}", e)),
        _ => (file_name.to_string(), String::new()),
    };
    for n in 1..10_000u32 {
        let candidate = format!("{}/{} ({}){}", dir, stem, n, ext);
        if free(&candidate) {
            return candidate;
        }
    }
    candidate
}


#[tauri::command]
async fn remove_download(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let (item, task, torrent, session, slot) = {
        let app_state = state.lock().unwrap();
        let item = app_state.downloads.lock().unwrap().remove(&id);
        let task = app_state.handles.lock().unwrap().remove(&id);
        let torrent = app_state.torrent_handles.lock().unwrap().remove(&id);
        let session = app_state.torrent_session.lock().unwrap().clone();
        let slot = app_state.yt_dlp_children.lock().unwrap().remove(&id);
        (item, task, torrent, session, slot)
    };
    if let Some(task) = task {
        task.abort();
        let _ = task.await;
    }
    // Kill any live yt-dlp child so its temp file isn't left being written.
    if let Some(slot) = slot {
        slot.abort.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let (Some(session), Some(torrent)) = (session, torrent) {
        // `false` keeps the downloaded files; Remove only drops the list row.
        let _ = session
            .delete(
                librqbit::api::TorrentIdOrHash::Id(torrent.id()),
                false,
            )
            .await;
    }
    if let Some(item) = item {
        remove_segment_artifacts(&item.save_path).await;
    }
    Ok(())
}

#[tauri::command]
async fn add_download(
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
    url: String,
    save_dir: Option<String>,
    media_format: Option<String>,
) -> Result<DownloadItem, String> {
    let url_clean = url.trim().to_string();
    if url_clean.is_empty() {
        return Err("Please enter a link or choose a .torrent file.".to_string());
    }
    let kind = classify_source(&url_clean)?;
    let file_name = match kind {
        SourceKind::Magnet => magnet_display_name(&url_clean)
            .map(|name| safe_path_component(&name))
            .unwrap_or_else(|| "magnet-download".to_string()),
        SourceKind::LocalTorrent | SourceKind::RemoteTorrent => {
            let base = file_name_from_url(&url_clean);
            let stem = std::path::Path::new(&base)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(base);
            let stem = if stem.is_empty() {
                "torrent-download".to_string()
            } else {
                stem
            };
            safe_path_component(&stem)
        }
        SourceKind::YtDlp => {
            // YouTube's watch URL (`/watch?v=ID`) yields the useless stem
            // "watch"; pull the video id out of the query string so the row
            // and the file on disk get a real, unique name instead.
            let base = file_name_from_url(&url_clean);
            let stem = if base == "watch" || base.is_empty() {
                url_clean
                    .split('?')
                    .nth(1)
                    .and_then(|q| {
                        q.split('&')
                            .find_map(|p| p.strip_prefix("v=").map(|v| v.to_string()))
                    })
                    .map(|id| format!("youtube-{}", id))
                    .unwrap_or_else(|| "video".to_string())
            } else {
                base
            };
            safe_path_component(&stem)
        }
        _ => file_name_from_url(&url_clean),
    };
    let category = category_for(&file_name);

    let routing = load_routing(&app);
    let dir = resolve_save_dir(&app, &routing, save_dir.as_deref(), &file_name);

    let taken: Vec<String> = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.values().map(|d| d.save_path.clone()).collect()
    };
    // Torrents are unpacked into their own folder, so the "file name" is
    // really a folder name here; unique_save_path still guards against an
    // existing file/folder with the same name (the usual cause of the
    // "The file exists" I/O error from the engine).
    let save_path = unique_save_path(&dir, &file_name, &taken);
    let id = make_id();
    let mut item = DownloadItem::new(
        id.clone(),
        url_clean.clone(),
        file_name,
        save_path.clone(),
        make_created_at(),
    );
    item.category = category;
    item.media_format = normalize_media_format(media_format.as_deref());
    {
        let app_state = state.lock().unwrap();
        let mut map = app_state.downloads.lock().unwrap();
        map.insert(id.clone(), item.clone());
    }

    let handle = match kind {
        SourceKind::Magnet => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::Magnet(url_clean),
            save_path.clone(),
        )),
        SourceKind::LocalTorrent => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::TorrentFile(url_clean),
            save_path.clone(),
        )),
        SourceKind::RemoteTorrent => tauri::async_runtime::spawn(run_torrent_download(
            app.clone(),
            id.clone(),
            TorrentSource::TorrentUrl(url_clean),
            save_path.clone(),
        )),
        SourceKind::Http => tauri::async_runtime::spawn(run_download(
            app.clone(),
            id.clone(),
            url_clean,
            save_path,
            0,
            None,
        )),
        SourceKind::YtDlp => {
            // Fetch the real media title in the background so "Add download"
            // returns immediately (no 5-8s hang) and the row is renamed once
            // the title is available.
            let app_bg = app.clone();
            let id_bg = id.clone();
            let url_bg = url_clean.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(title) = fetch_ytdlp_title(&url_bg).await {
                    let title = title.trim().to_string();
                    if !title.is_empty() {
                        let state = app_bg.state::<Mutex<AppState>>();
                        patch_item(&state, &id_bg, |it| {
                            it.file_name = title.clone();
                        });
                        // Notify the frontend so the row updates immediately.
                        if let Some(it) = {
                            let app_state = state.lock().unwrap();
                            let map = app_state.downloads.lock().unwrap();
                            map.get(&id_bg).cloned()
                        } {
                            let _ = app_bg.emit("download-progress", DownloadProgress {
                                id: it.id.clone(),
                                status: it.status.clone(),
                                percent: it.percent,
                                downloaded_bytes: it.downloaded_bytes,
                                total_bytes: it.total_bytes,
                                speed_bps: it.speed_bps,
                                error: None,
                                file_name: Some(it.file_name.clone()),
                            });
                        }
                    }
                }
            });
            tauri::async_runtime::spawn(yt_dlp_download(
                app.clone(),
                id.clone(),
                url_clean,
                save_path,
            ))
        }
    };
    {
        let app_state = state.lock().unwrap();
        let mut handles = app_state.handles.lock().unwrap();
        handles.insert(id.clone(), handle);
    }
    Ok(item)
}

async fn run_download(
    app: tauri::AppHandle,
    id: String,
    url: String,
    save_path: String,
    resume_from: u64,
    total_bytes: Option<u64>,
) {
    let state = app.state::<Mutex<AppState>>();

    let item = {
        let map = state.lock().unwrap();
        let map = map.downloads.lock().unwrap();
        map.get(&id).cloned()
    };
    if let Some(item) = item {
        if should_stop(&state, &id) {
            return;
        }
        // Scheduler: wait until start_at if set.
        if let Some(start_at) = item.start_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as f64;
            if start_at > now {
                tokio::time::sleep(Duration::from_secs_f64((start_at - now) / 1000.0)).await;
            }
        }
        if should_stop(&state, &id) {
            return;
        }
    }

    if let Some(parent) = std::path::Path::new(&save_path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                fail_download(
                    &app,
                    &state,
                    &id,
                    format!("Could not create save folder: {}", e),
                    resume_from,
                    total_bytes,
                );
                return;
            }
        }
    }

    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            fail_download(
                &app,
                &state,
                &id,
                format!("Could not create HTTP client: {}", e),
                resume_from,
                total_bytes,
            );
            return;
        }
    };

    // Resuming a segmented download: the plan left on disk is authoritative.
    if let Some(meta) = read_segment_meta(&save_path) {
        let result = run_segmented_download(
            &app, &id, &url, &save_path, &client, meta.total, meta.count, true,
        )
        .await;
        match result {
            Ok(_) => finish_http_download(&app, &state, &id, meta.total),
            Err(e) => {
                let done = segment_progress_on_disk(&save_path, meta).await;
                fail_download(&app, &state, &id, e, done, Some(meta.total));
            }
        }
        return;
    }

    // A partial file from a single-stream attempt: continue it directly.
    if resume_from > 0 {
        run_single_stream(
            &app,
            &state,
            &id,
            &url,
            &save_path,
            &client,
            resume_from,
            total_bytes,
        )
        .await;
        return;
    }

    // Probe cheaply for Range support before committing to a plan.
    let plan = match probe_http_plan(&client, &url).await {
        Ok(plan) => plan,
        Err(e) => {
            fail_download(&app, &state, &id, e, resume_from, total_bytes);
            return;
        }
    };

    if let HttpPlan::Segmented(total) = plan {
        let segments = http_segment_count(total);
        let result = run_segmented_download(
            &app,
            &id,
            &url,
            &save_path,
            &client,
            total,
            segments,
            false,
        )
        .await;
        match result {
            Ok(_) => finish_http_download(&app, &state, &id, total),
            Err(e) => {
                let meta = SegmentMeta {
                    total,
                    count: segments,
                };
                let done = segment_progress_on_disk(&save_path, meta).await;
                if done == 0 {
                    // The server advertised ranges but didn't honour them (or
                    // rejected every parallel request). Nothing was written,
                    // so a plain single stream can still complete the job.
                    let _ = remove_segment_artifacts(&save_path).await;
                    run_single_stream(
                        &app, &state, &id, &url, &save_path, &client, 0, total_bytes,
                    )
                    .await;
                    return;
                }
                fail_download(&app, &state, &id, e, done, Some(total));
            }
        }
        return;
    }

    run_single_stream(
        &app, &state, &id, &url, &save_path, &client, 0, total_bytes,
    )
    .await;
}

#[derive(Debug, PartialEq)]
enum HttpPlan {
    Segmented(u64),
    Single,
}

/// One tiny request tells us whether the server supports byte ranges and how
/// big the file is, without downloading any of it.
async fn probe_http_plan(client: &reqwest::Client, url: &str) -> Result<HttpPlan, String> {
    let resp = client
        .get(url)
        .header("User-Agent", "FaastDownloadManager/1.0")
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    let status = resp.status();
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range_total)
        .or_else(|| resp.content_length());
    // Most servers answer a Range probe with 206. A few answer 200 but still
    // advertise Accept-Ranges and honour real Range requests, so use that as a
    // hint too. If they don't actually honour it, the segmented attempt fails
    // before writing anything and the caller falls back to a single stream.
    let accepts_ranges = status == reqwest::StatusCode::PARTIAL_CONTENT
        || resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
    // Never keep reading the probe body: dropping the response also releases
    // the connection for the real requests.
    drop(resp);

    // An empty file makes `bytes=0-0` unsatisfiable (416); treat that as a
    // normal single-stream download rather than a hard error.
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Ok(HttpPlan::Single);
    }
    if !status.is_success() {
        return Err(format!("HTTP status {}", status));
    }
    if accepts_ranges {
        if let Some(total) = total.filter(|t| *t >= HTTP_SEGMENT_MIN_BYTES) {
            return Ok(HttpPlan::Segmented(total));
        }
    }
    Ok(HttpPlan::Single)
}

/// One sequential stream, with the usual resume / early-close handling.
async fn run_single_stream(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, Mutex<AppState>>,
    id: &str,
    url: &str,
    save_path: &str,
    client: &reqwest::Client,
    resume_from: u64,
    total_bytes: Option<u64>,
) {
    let mut req = client
        .get(url)
        .header("User-Agent", "FaastDownloadManager/1.0");
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={}", resume_from));
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            fail_download(
                app,
                state,
                id,
                format!("Network error: {}", e),
                resume_from,
                total_bytes,
            );
            return;
        }
    };

    let status_code = resp.status();
    if !status_code.is_success() {
        fail_download(
            app,
            state,
            id,
            format!("HTTP status {}", status_code),
            resume_from,
            total_bytes,
        );
        return;
    }

    let content_range = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok());
    let plan = plan_resume(resume_from, status_code, resp.content_length(), content_range);
    let total = plan.total.or(total_bytes);

    let mut file = match tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(plan.truncate)
        .open(save_path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            fail_download(
                app,
                state,
                id,
                format!("Could not open file: {}", e),
                plan.start_at,
                total,
            );
            return;
        }
    };
    if plan.start_at > 0 {
        use tokio::io::AsyncSeekExt;
        if let Err(e) = file.seek(std::io::SeekFrom::Start(plan.start_at)).await {
            fail_download(
                app,
                state,
                id,
                format!("Could not resume file: {}", e),
                plan.start_at,
                total,
            );
            return;
        }
    }

    let mut downloaded: u64 = plan.start_at;
    let mut last_emit = Instant::now();
    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded;
    let mut stream = resp.bytes_stream();

    // Report the size and starting point as soon as headers arrive.
    patch_item(state, id, |it| {
        it.status = "downloading".to_string();
        it.percent = compute_percent(downloaded, total);
        it.downloaded_bytes = downloaded;
        it.total_bytes = total;
        it.error = None;
    });
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.to_string(),
            status: "downloading".to_string(),
            percent: compute_percent(downloaded, total),
            downloaded_bytes: downloaded,
            total_bytes: total,
            speed_bps: None,
            error: None,
            file_name: None,
        },
    );

    loop {
        let chunk = match stream.next().await {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                fail_download(
                    app,
                    state,
                    id,
                    format!("Download error: {}", e),
                    downloaded,
                    total,
                );
                return;
            }
            None => break,
        };
        if chunk.is_empty() {
            continue;
        }
        {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = file.write_all(&chunk).await {
                fail_download(
                    app,
                    state,
                    id,
                    format!("Write error: {}", e),
                    downloaded,
                    total,
                );
                return;
            }
        }
        downloaded += chunk.len() as u64;

        // The user may have paused or removed this row while bytes were
        // arriving; stop writing and leave the partial file for resume.
        if should_stop(state, id) {
            return;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(last_speed_mark);
        let speed = if elapsed.as_nanos() > 0 {
            (downloaded - last_speed_bytes) as f64 / elapsed.as_secs_f64().max(0.0001)
        } else {
            0.0
        };

        let percent = match total {
            Some(t) if t > 0 => ((downloaded as f64) / (t as f64) * 100.0).min(100.0) as u32,
            _ => 0,
        };

        if now.duration_since(last_emit) >= Duration::from_millis(400) {
            last_emit = now;
            patch_item(state, id, |it| {
                it.status = "downloading".to_string();
                it.percent = percent;
                it.downloaded_bytes = downloaded;
                it.total_bytes = total;
                it.speed_bps = if speed > 0.0 { Some(speed) } else { None };
            });
            let _ = app.emit(
                "download-progress",
                DownloadProgress {
                    id: id.to_string(),
                    status: "downloading".to_string(),
                    percent,
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                    speed_bps: if speed > 0.0 { Some(speed) } else { None },
                    error: None,
                    file_name: None,
                },
            );
            last_speed_mark = now;
            last_speed_bytes = downloaded;
        }
    }

    // A connection can end cleanly without delivering every byte (truncated
    // proxy/connection reset treated as EOF). Never report a short file as done.
    if let Some(expected) = total {
        if downloaded < expected {
            use tokio::io::AsyncWriteExt;
            let _ = file.flush().await;
            fail_download(
                app,
                state,
                id,
                format!(
                    "Connection closed early â€” {} of {} bytes received",
                    downloaded, expected
                ),
                downloaded,
                Some(expected),
            );
            return;
        }
    }

    // Make sure the bytes are actually on disk before we report success, and
    // release the OS file handle so the user can open/rename the file right away.
    {
        use tokio::io::AsyncWriteExt;
        if let Err(e) = file.flush().await {
            fail_download(
                app,
                state,
                id,
                format!("Could not finish writing file: {}", e),
                downloaded,
                total,
            );
            return;
        }
    }
    drop(file);

    // If the server never told us a size, adopt what actually landed on disk.
    let total = total.or(Some(downloaded));
    finish_http_download(app, state, id, downloaded.min(total.unwrap_or(downloaded)));
}

fn finish_http_download(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, Mutex<AppState>>,
    id: &str,
    total: u64,
) {
    patch_item(state, id, |it| {
        it.status = "completed".to_string();
        it.percent = 100;
        it.downloaded_bytes = total;
        it.total_bytes = Some(total);
        it.speed_bps = Some(0.0);
        it.error = None;
    });
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.to_string(),
            status: "completed".to_string(),
            percent: 100,
            downloaded_bytes: total,
            total_bytes: Some(total),
            speed_bps: Some(0.0),
            error: None,
            file_name: None,
        },
    );
    forget_handle(state, id);
}

async fn segment_progress_on_disk(save_path: &str, meta: SegmentMeta) -> u64 {
    let mut done = 0u64;
    for (i, range) in split_ranges(meta.total, meta.count).iter().enumerate() {
        let len = tokio::fs::metadata(segment_part_path(save_path, i))
            .await
            .map(|m| m.len())
            .unwrap_or(0)
            .min(range.len());
        done += len;
    }
    done
}

async fn emit_http_progress(
    app: tauri::AppHandle,
    id: String,
    counter: Arc<AtomicU64>,
    total: u64,
    done: Arc<std::sync::atomic::AtomicBool>,
) {
    let state = app.state::<Mutex<AppState>>();
    let mut last = Instant::now();
    let mut last_bytes = counter.load(Ordering::Relaxed);
    while !done.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(400)).await;
        // Pausing (or removing) aborts the download task, but this reporter is
        // a separate task: stop on its own so it can't resurrect the row.
        let still_running = {
            let app_state = state.lock().unwrap();
            let map = app_state.downloads.lock().unwrap();
            match map.get(&id) {
                Some(item) if item.status == "downloading" => true,
                _ => false,
            }
        };
        if !still_running {
            break;
        }
        let now = Instant::now();
        let downloaded = counter.load(Ordering::Relaxed).min(total);
        let elapsed = now.duration_since(last).as_secs_f64().max(0.0001);
        let speed = downloaded.saturating_sub(last_bytes) as f64 / elapsed;
        last = now;
        last_bytes = downloaded;
        let percent = compute_percent(downloaded, Some(total));
        // Re-check under the same lock so a pause that landed between the
        // first check and here can't be overwritten.
        let patched = {
            let app_state = state.lock().unwrap();
            let mut map = app_state.downloads.lock().unwrap();
            match map.get_mut(&id) {
                Some(item) if item.status == "downloading" => {
                    item.percent = percent;
                    item.downloaded_bytes = downloaded;
                    item.total_bytes = Some(total);
                    item.speed_bps = if speed > 0.0 { Some(speed) } else { None };
                    true
                }
                _ => false,
            }
        };
        if !patched {
            break;
        }
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                id: id.clone(),
                status: "downloading".to_string(),
                percent,
                downloaded_bytes: downloaded,
                total_bytes: Some(total),
                speed_bps: if speed > 0.0 { Some(speed) } else { None },
                error: None,
                file_name: None,
            },
        );
    }
}

/// Downloads one inclusive byte range into its own part file, appending to
/// whatever a previous attempt already wrote so pause/resume is lossless.
pub async fn download_segment(
    client: &reqwest::Client,
    url: &str,
    part_path: &str,
    range: ByteRange,
    have: u64,
    counter: &AtomicU64,
) -> Result<(), String> {
    let expected = range.len();
    let mut have = have.min(expected);
    if have == expected {
        return Ok(());
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .await
        .map_err(|e| format!("Could not open temporary file: {}", e))?;
    let actual = file
        .metadata()
        .await
        .map(|m| m.len())
        .unwrap_or(0)
        .min(expected);
    if actual != have {
        // The on-disk tail is the truth after an abrupt stop.
        have = actual;
    }
    let from = range.start + have;
    if from > range.end {
        return Ok(());
    }

    let resp = client
        .get(url)
        .header("User-Agent", "FaastDownloadManager/1.0")
        .header(
            reqwest::header::RANGE,
            format!("bytes={}-{}", from, range.end),
        )
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "Server stopped supporting parallel downloads (HTTP {})",
            resp.status()
        ));
    }

    let mut stream = resp.bytes_stream();
    let mut written = have;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;
        if chunk.is_empty() {
            continue;
        }
        let remaining = expected.saturating_sub(written);
        if remaining == 0 {
            break;
        }
        let take = (chunk.len() as u64).min(remaining) as usize;
        {
            use tokio::io::AsyncWriteExt;
            file.write_all(&chunk[..take])
                .await
                .map_err(|e| format!("Write error: {}", e))?;
        }
        written += take as u64;
        counter.fetch_add(take as u64, Ordering::Relaxed);
    }

    {
        use tokio::io::AsyncWriteExt;
        file.flush()
            .await
            .map_err(|e| format!("Write error: {}", e))?;
    }
    if written < expected {
        return Err(format!(
            "Connection closed early â€” {} of {} bytes received",
            written, expected
        ));
    }
    Ok(())
}

/// Joins the part files into the real destination in order.
async fn merge_segments(save_path: &str, ranges: &[ByteRange]) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut dest = tokio::fs::File::create(save_path)
        .await
        .map_err(|e| format!("Could not create file: {}", e))?;
    let mut buf = vec![0u8; 128 * 1024];
    for (i, range) in ranges.iter().enumerate() {
        let part_path = segment_part_path(save_path, i);
        let mut part = tokio::fs::File::open(&part_path)
            .await
            .map_err(|e| format!("Could not read downloaded part {}: {}", i + 1, e))?;
        let len = part.metadata().await.map(|m| m.len()).unwrap_or(0);
        if len != range.len() {
            return Err(format!(
                "Downloaded part {} is incomplete ({} of {} bytes)",
                i + 1,
                len,
                range.len()
            ));
        }
        loop {
            let n = part
                .read(&mut buf)
                .await
                .map_err(|e| format!("Could not read downloaded part: {}", e))?;
            if n == 0 {
                break;
            }
            dest.write_all(&buf[..n])
                .await
                .map_err(|e| format!("Could not write file: {}", e))?;
        }
    }
    dest.flush()
        .await
        .map_err(|e| format!("Could not finish writing file: {}", e))?;
    dest.sync_all()
        .await
        .map_err(|e| format!("Could not finish writing file: {}", e))?;
    Ok(())
}

/// Downloads `total` bytes over several parallel Range connections and merges
/// the parts when the last one finishes.
async fn run_segmented_download(
    app: &tauri::AppHandle,
    id: &str,
    url: &str,
    save_path: &str,
    client: &reqwest::Client,
    total: u64,
    count: usize,
    resume: bool,
) -> Result<u64, String> {
    let ranges = split_ranges(total, count);
    if ranges.is_empty() {
        return Ok(0);
    }
    let meta = SegmentMeta {
        total,
        count: ranges.len(),
    };
    // Rewriting the plan keeps resume working even if the sidecar was removed.
    let _ = resume;
    write_segment_meta(save_path, meta)
        .map_err(|e| format!("Could not create download state: {}", e))?;

    // Count what is already on disk so paused parts don't restart.
    let mut existing = Vec::with_capacity(ranges.len());
    let mut downloaded = 0u64;
    for (i, range) in ranges.iter().enumerate() {
        let len = tokio::fs::metadata(segment_part_path(save_path, i))
            .await
            .map(|m| m.len())
            .unwrap_or(0)
            .min(range.len());
        existing.push(len);
        downloaded += len;
    }

    let state = app.state::<Mutex<AppState>>();
    patch_item(&state, id, |it| {
        it.status = "downloading".to_string();
        it.percent = compute_percent(downloaded, Some(total));
        it.downloaded_bytes = downloaded;
        it.total_bytes = Some(total);
        it.error = None;
    });
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            id: id.to_string(),
            status: "downloading".to_string(),
            percent: compute_percent(downloaded, Some(total)),
            downloaded_bytes: downloaded,
            total_bytes: Some(total),
            speed_bps: None,
            error: None,
            file_name: None,
        },
    );

    let counter = Arc::new(AtomicU64::new(downloaded));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let progress = tauri::async_runtime::spawn(emit_http_progress(
        app.clone(),
        id.to_string(),
        counter.clone(),
        total,
        done.clone(),
    ));

    let mut futures = FuturesUnordered::new();
    for (i, range) in ranges.iter().enumerate() {
        let client = client.clone();
        let url = url.to_string();
        let part_path = segment_part_path(save_path, i);
        let range = *range;
        let have = existing[i];
        let counter = counter.clone();
        futures.push(async move {
            download_segment(&client, &url, &part_path, range, have, &counter).await
        });
    }

    let mut failure = None;
    while let Some(result) = futures.next().await {
        if let Err(e) = result {
            failure = Some(e);
            // Dropping the remaining futures closes their connections; the
            // bytes they already wrote stay in their part files for resume.
            break;
        }
    }
    done.store(true, Ordering::Relaxed);
    progress.abort();

    if let Some(e) = failure {
        return Err(e);
    }

    merge_segments(save_path, &ranges).await?;
    remove_segment_artifacts(save_path).await;
    Ok(total)
}

/// Creates (once) the shared librqbit session used by every torrent download.
async fn ensure_torrent_session(
    app: &tauri::AppHandle,
) -> Result<Arc<librqbit::Session>, String> {
    let state = app.state::<Mutex<AppState>>();
    {
        let app_state = state.lock().unwrap();
        let existing = app_state.torrent_session.lock().unwrap().clone();
        if let Some(session) = existing {
            return Ok(session);
        }
    }

    let init = {
        let app_state = state.lock().unwrap();
        app_state.torrent_init.clone()
    };
    let _guard = init.lock().await;

    // Another add may have finished initializing while we waited.
    {
        let app_state = state.lock().unwrap();
        let existing = app_state.torrent_session.lock().unwrap().clone();
        if let Some(session) = existing {
            return Ok(session);
        }
    }

    let state_dir = app
        .path()
        .app_local_data_dir()
        .map(|p| p.join("torrents"))
        .unwrap_or_else(|_| std::env::temp_dir().join("faast-torrents"));
    tokio::fs::create_dir_all(&state_dir)
        .await
        .map_err(|e| format!("Could not create torrent data folder: {}", e))?;

    let opts = librqbit::SessionOptions {
        listen: Some(librqbit::ListenerOptions {
            // Many WebTorrent-era peers are uTP-only; a TCP-only listener can
            // never reach them.
            mode: librqbit::ListenerMode::TcpAndUtp,
            ipv4_only: true,
            // Ask the router to forward our listening port so more peers can
            // reach us (helps download speed on NATed connections).
            enable_upnp_port_forwarding: true,
            max_pending_incoming_handshake_checks: 512,
            ..Default::default()
        }),
        ipv4_only: true,
        // More concurrent peers = better chance of saturating the link.
        peer_limit: Some(500),
        client_name_and_version: Some("Faast 0.1.0".to_string()),
        ..Default::default()
    };

    let session = librqbit::Session::new_with_opts(state_dir, opts)
        .await
        .map_err(|e| format!("Could not start torrent engine: {e:#}"))?;
    {
        let app_state = state.lock().unwrap();
        *app_state.torrent_session.lock().unwrap() = Some(session.clone());
    }
    Ok(session)
}

async fn run_torrent_download(
    app: tauri::AppHandle,
    id: String,
    source: TorrentSource,
    save_dir: String,
) {
    let state = app.state::<Mutex<AppState>>();

    if let Err(e) = tokio::fs::create_dir_all(&save_dir).await {
        fail_download(
            &app,
            &state,
            &id,
            format!("Could not create save folder '{}': {}", save_dir, e),
            0,
            None,
        );
        return;
    }

    let session = match ensure_torrent_session(&app).await {
        Ok(s) => s,
        Err(e) => {
            fail_download(&app, &state, &id, e, 0, None);
            return;
        }
    };

    // Resolve the source into either a magnet/HTTP URL or in-memory .torrent
    // bytes; librqbit also accepts HTTP(S) .torrent URLs directly.
    let add = match source {
        TorrentSource::Magnet(uri) => librqbit::AddTorrent::from_url(uri),
        TorrentSource::TorrentFile(path) => {
            let data = match tokio::fs::read(&path).await {
                Ok(d) => d,
                Err(e) => {
                    fail_download(
                        &app,
                        &state,
                        &id,
                        format!("Could not read torrent file: {}", e),
                        0,
                        None,
                    );
                    return;
                }
            };
            librqbit::AddTorrent::from_bytes(data)
        }
        TorrentSource::TorrentUrl(url) => {
            let client = match reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    fail_download(
                        &app,
                        &state,
                        &id,
                        format!("Torrent error: {}", e),
                        0,
                        None,
                    );
                    return;
                }
            };
            let response = match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => r,
                Ok(r) => {
                    fail_download(
                        &app,
                        &state,
                        &id,
                        format!("Could not download .torrent (HTTP {})", r.status()),
                        0,
                        None,
                    );
                    return;
                }
                Err(e) => {
                    fail_download(
                        &app,
                        &state,
                        &id,
                        format!("Could not download .torrent: {}", e),
                        0,
                        None,
                    );
                    return;
                }
            };
            let data = match response.bytes().await {
                Ok(d) => d,
                Err(e) => {
                    fail_download(
                        &app,
                        &state,
                        &id,
                        format!("Could not read .torrent: {}", e),
                        0,
                        None,
                    );
                    return;
                }
            };
            librqbit::AddTorrent::from_bytes(data)
        }
    };

    let opts = librqbit::AddTorrentOptions {
        output_folder: Some(save_dir.clone()),
        overwrite: true,
        ..Default::default()
    };
    let handle = match session.add_torrent(add, Some(opts)).await {
        Ok(response) => match response.into_handle() {
            Some(handle) => handle,
            None => {
                fail_download(
                    &app,
                    &state,
                    &id,
                    "Torrent error: engine did not start the torrent".to_string(),
                    0,
                    None,
                );
                return;
            }
        },
        Err(e) => {
            fail_download(
                &app,
                &state,
                &id,
                format!("Torrent error: {e:#}"),
                0,
                None,
            );
            return;
        }
    };

    // A remove or pause can land while the engine resolves metadata. Don't
    // leave an orphaned torrent running (or resume a paused row).
    enum PostAddAction {
        Delete,
        Pause,
        Continue,
    }
    let action = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        match map.get(&id) {
            None => PostAddAction::Delete,
            Some(item) if item.status != "downloading" => PostAddAction::Pause,
            _ => PostAddAction::Continue,
        }
    };
    match action {
        PostAddAction::Delete => {
            let _ = session
                .delete(librqbit::api::TorrentIdOrHash::Id(handle.id()), false)
                .await;
            return;
        }
        PostAddAction::Pause => {
            let _ = session.pause(&handle).await;
            let app_state = state.lock().unwrap();
            app_state
                .torrent_handles
                .lock()
                .unwrap()
                .insert(id.clone(), handle.clone());
            return;
        }
        PostAddAction::Continue => {}
    }

    {
        let app_state = state.lock().unwrap();
        app_state
            .torrent_handles
            .lock()
            .unwrap()
            .insert(id.clone(), handle.clone());
    }

    poll_torrent(app, id, handle, session, save_dir).await;
}

/// Polls librqbit's live stats and mirrors them into the download row.
async fn poll_torrent(
    app: tauri::AppHandle,
    id: String,
    handle: Arc<librqbit::ManagedTorrent>,
    session: Arc<librqbit::Session>,
    save_dir: String,
) {
    let state = app.state::<Mutex<AppState>>();
    let mut last_name: Option<String> = None;
    let _save_dir = save_dir.trim_end_matches(['/', '\\']).to_string();

    loop {
        let stats = handle.stats();

        // The real name only exists once metadata has been fetched, so update
        // the row the moment it appears. The save path stays the output folder.
        if let Some(name) = handle.name().and_then(|n| clean_display_name(&n)) {
            if last_name.as_deref() != Some(name.as_str()) {
                let name_for_patch = name.clone();
                patch_item(&state, &id, |it| {
                    it.file_name = name_for_patch.clone();
                    it.category = category_for(&it.file_name);
                });
                last_name = Some(name);
            }
        }

        let total = if stats.total_bytes > 0 {
            Some(stats.total_bytes)
        } else {
            None
        };
        let percent = compute_percent(stats.progress_bytes, total);
        let is_error = matches!(stats.state, librqbit::TorrentStatsState::Error)
            || stats.error.is_some();
        let is_paused = matches!(stats.state, librqbit::TorrentStatsState::Paused);
        let (status, error) = if stats.finished {
            ("completed", None)
        } else if is_error {
            (
                "failed",
                Some(
                    stats
                        .error
                        .clone()
                        .unwrap_or_else(|| "Torrent engine error".to_string()),
                ),
            )
        } else if is_paused {
            ("paused", None)
        } else {
            ("downloading", None)
        };

        let speed = if status == "downloading" {
            stats
                .live
                .as_ref()
                .map(|live| live.download_speed.as_bytes() as f64)
                .filter(|s| *s > 0.0)
        } else {
            Some(0.0)
        };

        patch_item(&state, &id, |it| {
            it.status = status.to_string();
            it.percent = percent;
            it.downloaded_bytes = stats.progress_bytes;
            it.total_bytes = total;
            it.speed_bps = speed;
            it.error = error.clone();
        });
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                id: id.clone(),
                status: status.to_string(),
                percent,
                downloaded_bytes: stats.progress_bytes,
                total_bytes: total,
                speed_bps: speed,
                error: error.clone(),
                file_name: last_name.clone(),
            },
        );

        if status == "completed" {
            // A download manager should stop uploading once the job is done.
            let _ = session.pause(&handle).await;
            forget_handle(&state, &id);
            break;
        }
        if status == "failed" || status == "paused" {
            forget_handle(&state, &id);
            break;
        }

        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

/// Effective byte-rate cap for a download: the tighter of global + per-download.
fn effective_limit_bps(
    state: &tauri::State<'_, Mutex<AppState>>,
    item: &DownloadItem,
) -> Option<f64> {
    let global = state.lock().unwrap().speed_limit_global.lock().unwrap().clone();
    let per = item.speed_limit_bps;
    match (global, per) {
        (Some(g), Some(p)) => Some(g.min(p)),
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

/// Every media format the add dialog offers. Anything not in this list is
/// coerced back to "best", so a hand-crafted IPC payload can never smuggle
/// arbitrary arguments into the yt-dlp command line.
const MEDIA_FORMATS: &[&str] = &[
    "best", "1080p", "720p", "480p", "mp3", "m4a", "wav", "flac", "opus",
];

fn normalize_media_format(raw: Option<&str>) -> String {
    let v = raw.unwrap_or("").trim().to_lowercase();
    if MEDIA_FORMATS.contains(&v.as_str()) {
        v
    } else {
        "best".to_string()
    }
}

/// True for the audio-only formats, which need ffmpeg to remux/encode.
fn is_audio_format(fmt: &str) -> bool {
    matches!(fmt, "mp3" | "m4a" | "wav" | "flac" | "opus")
}

/// Builds the yt-dlp `--format` selector for a user choice.
/// Video picks keep the best audio and let ffmpeg merge; audio picks take the
/// best available audio stream and convert it.
fn ytdlp_format_selector(fmt: &str) -> String {
    match fmt {
        "1080p" => "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best".to_string(),
        "720p" => "bestvideo[height<=720]+bestaudio/best[height<=720]/best".to_string(),
        "480p" => "bestvideo[height<=480]+bestaudio/best[height<=480]/best".to_string(),
        f if is_audio_format(f) => "bestaudio/best".to_string(),
        _ => "bestvideo+bestaudio/best".to_string(),
    }
}

/// Builds the full yt-dlp argument list for a download, including audio
/// conversion flags. Kept as one pure function so it can be unit-tested
/// without spawning a process.
fn ytdlp_args(
    fmt: &str,
    save_path: &str,
    url: &str,
    limit_bps: Option<f64>,
) -> Vec<String> {
    let mut args = vec![
        // `--newline` is required: without it yt-dlp redraws progress using
        // carriage returns, so the line-by-line stderr reader in
        // yt_dlp_download would never observe a finished progress line and the
        // row would sit at 0%. (Previously `--new-session`, which is not a
        // valid yt-dlp option â€” every media download died instantly with a
        // usage error, even with yt-dlp installed correctly.)
        "--newline".to_string(),
        "--continue".to_string(),
        "--no-part".to_string(),
        "--output".to_string(),
        save_path.to_string(),
        "--format".to_string(),
        ytdlp_format_selector(fmt),
        "--retries".to_string(),
        "10".to_string(),
        "--fragment-retries".to_string(),
        "10".to_string(),
        // Keep the chosen extension instead of whatever the source muxes to,
        // so the file on disk matches what the UI promised the user.
        "--merge-output-format".to_string(),
        "mp4".to_string(),
    ];
    if is_audio_format(fmt) {
        args.push("--extract-audio".to_string());
        args.push("--audio-format".to_string());
        args.push(fmt.to_string());
    }
    if let Some(limit) = limit_bps {
        args.push("--limit-rate".to_string());
        args.push(format!("{limit:.0}"));
    }
    args.push(url.to_string());
    args
}

/// Fetches the media title from yt-dlp without downloading anything.
/// Returns `None` when yt-dlp is missing, times out, or the URL is private.
/// Used so the row and the file on disk are named after the actual content
/// instead of the URL's last path segment ("watch").
async fn fetch_ytdlp_title(url: &str) -> Option<String> {
    let mut child = tokio::process::Command::new("yt-dlp")
        .args(["--no-download", "--print", "title", "--no-warnings", url])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let read = async {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut reader = stdout;
        reader.read_to_end(&mut buf).await.ok()?;
        Some(String::from_utf8_lossy(&buf).trim().to_string())
    };
    match tokio::time::timeout(std::time::Duration::from_secs(8), read).await {
        Ok(Some(title)) if !title.is_empty() => {
            let _ = child.wait().await;
            Some(title)
        }
        _ => {
            let _ = child.kill().await;
            None
        }
    }
}

/// Shared handle to a live yt-dlp child process so pause_download can stop it.
struct YtDlpSlot {
    child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    abort: std::sync::atomic::AtomicBool,
}

/// Parses a yt-dlp progress stderr line like:
///   [download]  42.1% of ~12.34MiB @ 1.20MiB/s ETA 0:10
fn parse_ytdlp_progress(line: &str) -> Option<(u32, Option<u64>, Option<f64>)> {
    if !line.contains('%') {
        return None;
    }
    let percent = line
        .split('%')
        .next()?
        .split_whitespace()
        .last()?
        .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .parse::<f64>()
        .ok()?
        .clamp(0.0, 100.0) as u32;
    // yt-dlp writes `of ~12.34MiB` when the size is an estimate and
    // `of  246.27KiB` (no tilde) when it is known exactly. Only anchoring on
    // the tilde dropped the size for every exactly-known download.
    let size_tok = line
        .split("of ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|t| t.trim_start_matches('~'))
        .and_then(parse_ytdlp_size);
    // Speed reads `at    4.19MiB/s` on current yt-dlp and `@ 1.20MiB/s` on
    // older builds. Match the token ending in `B/s` rather than splitting on
    // a single delimiter, which silently yielded `None` for real output.
    let speed_bps = line
        .split_whitespace()
        .find(|tok| tok.ends_with("B/s"))
        .and_then(parse_ytdlp_size)
        .map(|b| b as f64);
    Some((percent, size_tok, speed_bps))
}

/// Parse `12.34MiB`, `1.20KiB/s`, `500B`, or a plain byte count into bytes.
/// yt-dlp reports binary units (`KiB`/`MiB`/`GiB`) and decimal ones (`kB`/
/// `MB`) â€” the `i` must be honoured or every size reads ~2.4% low.
fn parse_ytdlp_size(tok: &str) -> Option<u64> {
    let t = tok.trim();
    if t.is_empty() {
        return None;
    }
    // Tolerate a leading `@` (older yt-dlp speed format: `@ 1.20MiB/s`).
    let t = t.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '.');
    // Split "246.27KiB" / "1.20MiB/s" / "500B" into number + unit.
    let split = t.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let n: f64 = t[..split].parse().ok()?;
    let unit = t[split..].trim();
    // Rates arrive as `MiB/s`; strip that so only the magnitude letter remains.
    let unit = unit.strip_suffix("/s").unwrap_or(unit);
    let binary = unit.contains('i');
    let letter = unit.chars().next()?.to_ascii_uppercase();
    let mult = match (letter, binary) {
        ('K', true) => 1024.0,
        ('K', false) => 1_000.0,
        ('M', true) => 1024.0 * 1024.0,
        ('M', false) => 1_000_000.0,
        ('G', true) => 1024.0f64.powi(3),
        ('G', false) => 1_000_000_000.0,
        ('T', true) => 1024.0f64.powi(4),
        ('T', false) => 1_000_000_000_000.0,
        // Bare bytes (`500B`) or a plain number.
        _ => 1.0,
    };
    Some((n * mult) as u64)
}

/// Download a YouTube / video-site URL via yt-dlp.
async fn yt_dlp_download(
    app: tauri::AppHandle,
    id: String,
    url: String,
    save_path: String,
) {
    let state = app.state::<Mutex<AppState>>();
    let item = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id).cloned()
    };
    let Some(item) = item else { return; };
    if should_stop(&state, &id) { return; }
    if let Some(start_at) = item.start_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as f64;
        if start_at > now {
            tokio::time::sleep(Duration::from_secs_f64((start_at - now) / 1000.0)).await;
        }
    }
    if should_stop(&state, &id) { return; }

    let args = ytdlp_args(
        &item.media_format,
        &save_path,
        &url,
        effective_limit_bps(&state, &item),
    );

    let slot = Arc::new(YtDlpSlot {
        child: tokio::sync::Mutex::new(None),
        abort: std::sync::atomic::AtomicBool::new(false),
    });
    {
        let app_state = state.lock().unwrap();
        let mut map = app_state.yt_dlp_children.lock().unwrap();
        map.insert(id.clone(), slot.clone());
    }

    let child = match Command::new("yt-dlp")
        .args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let app_state = state.lock().unwrap();
            app_state.yt_dlp_children.lock().unwrap().remove(&id);
            fail_download(&app, &state, &id,
                format!("yt-dlp could not be started ({e}). Install from https://github.com/yt-dlp/yt-dlp"),
                0, None);
            return;
        }
    };
    { let mut guard = slot.child.lock().await; *guard = Some(child); }
    let mut child = { let mut guard = slot.child.lock().await; guard.take().unwrap() };
    let stderr = child.stderr.take().expect("yt-dlp stderr");
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    loop {
        if slot.abort.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = child.kill().await;
            if let Some(it) = {
                let app_state = state.lock().unwrap();
                let map = app_state.downloads.lock().unwrap();
                map.get(&id).cloned()
            } {
                let _ = app.emit("download-progress", DownloadProgress {
                    id: it.id.clone(), status: "paused".to_string(), percent: it.percent,
                    downloaded_bytes: it.downloaded_bytes, total_bytes: it.total_bytes,
                    speed_bps: Some(0.0), error: None,
                    file_name: Some(it.file_name.clone()),
                });
            }
            let app_state = state.lock().unwrap();
            app_state.yt_dlp_children.lock().unwrap().remove(&id);
            return;
        }
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Some((percent, total, speed)) = parse_ytdlp_progress(&line) {
                    let downloaded = total.map(|t| (percent as f64 / 100.0 * t as f64) as u64)
                        .unwrap_or(item.downloaded_bytes);
                    patch_item(&state, &id, |it| {
                        it.status = "downloading".to_string(); it.percent = percent;
                        it.downloaded_bytes = downloaded; it.total_bytes = total;
                        it.speed_bps = speed; it.error = None;
                    });
                    let _ = app.emit("download-progress", DownloadProgress {
                        id: id.clone(), status: "downloading".to_string(), percent,
                        downloaded_bytes: downloaded, total_bytes: total, speed_bps: speed,
                        error: None, file_name: Some(item.file_name.clone()),
                    });
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    {
        let app_state = state.lock().unwrap();
        app_state.yt_dlp_children.lock().unwrap().remove(&id);
    }
    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => { fail_download(&app, &state, &id, format!("yt-dlp wait error: {e}"), 0, None); return; }
    };
    if status.success() {
        patch_item(&state, &id, |it| {
            it.status = "completed".to_string(); it.percent = 100;
            it.speed_bps = Some(0.0); it.error = None;
        });
        if let Some(it) = {
            let app_state = state.lock().unwrap();
            let map = app_state.downloads.lock().unwrap();
            map.get(&id).cloned()
        } {
            let _ = app.emit("download-progress", DownloadProgress {
                id: it.id.clone(), status: "completed".to_string(), percent: 100,
                downloaded_bytes: it.downloaded_bytes, total_bytes: it.total_bytes,
                speed_bps: Some(0.0), error: None, file_name: Some(it.file_name.clone()),
            });
        }
    } else {
        fail_download(&app, &state, &id, "yt-dlp finished with an error".to_string(), 0, None);
    }
}

#[tauri::command]
async fn download_ytdlp(
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<(), String> {
    let item = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id).cloned().ok_or("Download not found")?
    };
    if item.source_kind != "ytdlp" {
        return Err("Not a yt-dlp download".to_string());
    }
    if item.status == "downloading" {
        return Ok(());
    }
    tauri::async_runtime::spawn(yt_dlp_download(app, id, item.url, item.save_path));
    Ok(())
}

#[tauri::command]
fn set_speed_limit(
    state: State<'_, Mutex<AppState>>,
    limit_kbps: Option<f64>,
) -> Result<(), String> {
    let bps = limit_kbps.map(|k| k * 1024.0);
    *state.lock().unwrap().speed_limit_global.lock().unwrap() = bps;
    Ok(())
}

#[tauri::command]
fn get_speed_limit(state: State<'_, Mutex<AppState>>) -> Option<f64> {
    state.lock().unwrap().speed_limit_global.lock().unwrap().clone()
}

/// Schedule (or reschedule) a download to start at the given epoch-ms.
#[tauri::command]
fn set_schedule(
    state: State<'_, Mutex<AppState>>,
    id: String,
    start_at: Option<f64>,
) -> Result<(), String> {
    let app_state = state.lock().unwrap();
    let mut map = app_state.downloads.lock().unwrap();
    match map.get_mut(&id) {
        Some(it) => {
            it.start_at = start_at;
            Ok(())
        }
        None => Err("Download not found".to_string()),
    }
}

#[tauri::command]
async fn open_containing_folder(
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<(), String> {
    let save_path = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id)
            .map(|d| d.save_path.clone())
            .ok_or("Download not found")?
    };
    let folder = std::path::Path::new(&save_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(save_path);
    std::process::Command::new("explorer")
        .arg(&folder)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open folder: {e}"))
}

#[tauri::command]
async fn open_file(
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<(), String> {
    let save_path = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        map.get(&id)
            .map(|d| d.save_path.clone())
            .ok_or("Download not found")?
    };
    if !std::path::Path::new(&save_path).exists() {
        return Err("File not found on disk".to_string());
    }
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &save_path])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open file: {e}"))
}

/// Returns the persisted routing rules, falling back to defaults.
#[tauri::command]
fn get_routing_config(app: tauri::AppHandle) -> RoutingConfig {
    load_routing(&app)
}

/// Persists routing rules and returns the saved config.
#[tauri::command]
fn set_routing_config(app: tauri::AppHandle, config: RoutingConfig) -> RoutingConfig {
    // A bad write must not silently look like success â€” but the in-memory
    // value still goes back so the UI can show what the rules *should* be.
    if let Err(e) = save_routing(&app, &config) {
        eprintln!("faast: could not save routing rules: {e}");
    }
    config
}

/// Re-queue a failed or stopped download (same URL, same folder).
#[tauri::command]
async fn retry_download(    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
    id: String,
) -> Result<DownloadItem, String> {
    let (url, save_dir, media_format) = {
        let app_state = state.lock().unwrap();
        let map = app_state.downloads.lock().unwrap();
        let item = map.get(&id).ok_or("Download not found")?;
        (
            item.url.clone(),
            save_dir_of(&item.save_path),
            item.media_format.clone(),
        )
    };
    // Drop the old row and any task handle so the fresh add takes its place.
    {
        let app_state = state.lock().unwrap();
        let mut map = app_state.downloads.lock().unwrap();
        map.remove(&id);
        let mut handles = app_state.handles.lock().unwrap();
        handles.remove(&id);
        let mut ytdlp = app_state.yt_dlp_children.lock().unwrap();
        ytdlp.remove(&id);
    }
    add_download(app, state, url, Some(save_dir), Some(media_format)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::default()))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_downloads,
            get_download,
            add_download,
            download_ytdlp,
            pause_download,
            resume_download,
            remove_download,
            read_clipboard,
            set_speed_limit,
            get_speed_limit,
            set_schedule,
            open_containing_folder,
            open_file,
            retry_download,
            get_routing_config,
            set_routing_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    // â”€â”€ Folder routing â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn games_keyword_routes_to_games_folder() {
        // Torrents have no extension, so keyword matching is what sorts them.
        let cfg = RoutingConfig::default();
        assert_eq!(route_subfolder(&cfg, "Some.Indie.Game-GOG"), "Games");
        assert_eq!(route_subfolder(&cfg, "The Game Awards"), "Games");
        // Known limitation: keyword rules only see the *name*, so a title with
        // no matching word ("Elden Ring") is NOT detected as a game and stays
        // in the root. This is expected â€” adding more keywords is how users
        // tune it, not a bug in matching.
        assert_eq!(route_subfolder(&cfg, "Elden Ring"), "");
    }

    #[test]
    fn media_extensions_route_to_their_folders() {
        let cfg = RoutingConfig::default();
        assert_eq!(route_subfolder(&cfg, "clip.mp4"), "Video");
        assert_eq!(route_subfolder(&cfg, "song.flac"), "Music");
        assert_eq!(route_subfolder(&cfg, "setup.exe"), "Software");
        assert_eq!(route_subfolder(&cfg, "ubuntu.iso"), "Software");
        assert_eq!(route_subfolder(&cfg, "manual.pdf"), "Documents");
    }

    #[test]
    fn routing_is_case_insensitive() {
        let cfg = RoutingConfig::default();
        assert_eq!(route_subfolder(&cfg, "MOVIE.MP4"), "Video");
        assert_eq!(route_subfolder(&cfg, "MyGame"), "Games");
    }

    #[test]
    fn unknown_files_stay_in_the_root() {
        // No rule matched -> empty subfolder means "root, no subfolder".
        assert_eq!(route_subfolder(&RoutingConfig::default(), "notes.xyz"), "");
        assert_eq!(route_subfolder(&RoutingConfig::default(), "readme"), "");
    }

    #[test]
    fn disabled_routing_ignores_all_rules() {
        let mut cfg = RoutingConfig::default();
        cfg.enabled = false;
        assert_eq!(route_subfolder(&cfg, "movie.mp4"), "");
        assert_eq!(route_subfolder(&cfg, "Some Game"), "");
    }

    #[test]
    fn disabled_rule_is_skipped() {
        let mut cfg = RoutingConfig::default();
        for r in &mut cfg.rules {
            if r.pattern == "game" {
                r.enabled = false;
            }
        }
        assert_eq!(route_subfolder(&cfg, "Some Game"), "");
    }

    #[test]
    fn first_matching_rule_wins() {
        // An explicit user rule must beat the built-in defaults it shadows.
        let mut cfg = RoutingConfig::default();
        cfg.rules.insert(
            0,
            RouteRule {
                kind: "extension".into(),
                pattern: "mp4".into(),
                folder: "Keep".into(),
                enabled: true,
            },
        );
        assert_eq!(route_subfolder(&cfg, "clip.mp4"), "Keep");
        assert_eq!(route_subfolder(&cfg, "song.mp3"), "Music");
    }

    #[test]
    fn torrent_names_without_a_dot_do_not_fake_an_extension() {
        // "Some.Game" must match the `game` keyword, not be treated as
        // having extension "game".
        let cfg = RoutingConfig::default();
        assert_eq!(route_subfolder(&cfg, "Some.Game"), "Games");
    }

    #[test]
    fn rule_folder_cannot_escape_the_root() {
        // A hand-edited routing.json must not be able to write outside root.
        let cfg = RoutingConfig {
            enabled: true,
            root: String::new(),
            rules: vec![RouteRule {
                kind: "keyword".into(),
                pattern: "evil".into(),
                folder: "..\\..\\Windows".into(),
                enabled: true,
            }],
        };
        let sub = route_subfolder(&cfg, "evil-file.zip");
        // safe_path_component rewrites separators to underscores, so the value
        // is a single harmless folder name. What matters is that no separator
        // survives for an attacker to traverse with.
        assert!(
            !sub.contains('\\') && !sub.contains('/'),
            "subfolder must not contain a path separator: {sub}"
        );
        assert_eq!(sub, ".._.._Windows");
    }

    #[test]
    fn empty_pattern_rules_are_ignored() {
        // A blank pattern would otherwise match everything via `contains("")`.
        let cfg = RoutingConfig {
            enabled: true,
            root: String::new(),
            rules: vec![RouteRule {
                kind: "keyword".into(),
                pattern: "   ".into(),
                folder: "Trap".into(),
                enabled: true,
            }],
        };
        assert_eq!(route_subfolder(&cfg, "anything.txt"), "");
    }

    #[test]
    fn ytdlp_detection_covers_youtube_variants() {
        assert!(is_yt_dlp_url("https://www.youtube.com/watch?v=abc"));
        assert!(is_yt_dlp_url("https://youtube.com/watch?v=abc"));
        assert!(is_yt_dlp_url("https://m.youtube.com/watch?v=abc"));
        assert!(is_yt_dlp_url("https://music.youtube.com/watch?v=abc"));
        assert!(is_yt_dlp_url("https://youtu.be/abc"));
        assert!(!is_yt_dlp_url("https://example.com/file.zip"));
        assert!(!is_yt_dlp_url("https://notyoutube.com/watch"));
    }

    #[test]
    fn media_format_whitelist_rejects_injection() {
        // Anything not on the allow-list collapses to "best", so a crafted
        // IPC payload can't smuggle flags into the yt-dlp command line.
        assert_eq!(normalize_media_format(None), "best");
        assert_eq!(normalize_media_format(Some("")), "best");
        assert_eq!(normalize_media_format(Some("720p")), "720p");
        assert_eq!(normalize_media_format(Some("  MP3 ")), "mp3");
        assert_eq!(normalize_media_format(Some("flac")), "flac");
        assert_eq!(
            normalize_media_format(Some("mp3; rm -rf /")),
            "best",
            "shell-ish payload must not survive normalization"
        );
        assert_eq!(
            normalize_media_format(Some("--post-exec-hook")),
            "best",
            "flag-looking value must not survive normalization"
        );
    }

    #[test]
    fn audio_formats_select_best_audio_only() {
        for f in ["mp3", "m4a", "wav", "flac", "opus"] {
            assert_eq!(ytdlp_format_selector(f), "bestaudio/best");
        }
    }

    #[test]
    fn video_formats_cap_height() {
        assert!(ytdlp_format_selector("1080p").contains("height<=1080"));
        assert!(ytdlp_format_selector("720p").contains("height<=720"));
        assert!(ytdlp_format_selector("480p").contains("height<=480"));
    }

    #[test]
    fn audio_args_request_extraction_with_the_right_codec() {
        let args = ytdlp_args("mp3", "C:/dl/track", "https://x/watch?v=1", None);
        let joined = args.join(" ");
        assert!(joined.contains("--extract-audio"), "got: {joined}");
        assert!(joined.contains("--audio-format mp3"), "got: {joined}");
        assert!(
            !joined.contains("--audio-format mp4"),
            "audio pick must not claim to be video"
        );
    }

    #[test]
    fn best_video_args_do_not_extract_audio() {
        let args = ytdlp_args("best", "C:/dl/vid", "https://x/watch?v=1", None);
        let joined = args.join(" ");
        assert!(!joined.contains("--extract-audio"), "got: {joined}");
        assert!(joined.contains("--format bestvideo+bestaudio/best"), "got: {joined}");
    }

    #[test]
    fn speed_limit_is_appended_before_the_url() {
        let args = ytdlp_args("best", "C:/dl/v", "https://x/y", Some(512000.0));
        let joined = args.join(" ");
        assert!(joined.contains("--limit-rate 512000"), "got: {joined}");
        // The URL must be the final argument, or yt-dlp treats it as a flag.
        assert_eq!(args.last().unwrap(), "https://x/y");
    }

    // The following use lines captured verbatim from a real yt-dlp run, so a
    // change to yt-dlp's output format fails here instead of silently
    // pinning the progress bar at 0%.

    #[test]
    fn progress_parses_exact_size_line_without_tilde() {
        // Real output for a known-length file: "of  246.27KiB" (no `~`).
        let (pct, total, speed) =
            parse_ytdlp_progress("[download]   6.1% of  246.27KiB at    4.19MiB/s ETA 00:00")
                .expect("must parse");
        assert_eq!(pct, 6);
        // KiB is binary: 246.27 * 1024.
        assert_eq!(total, Some(252_180), "exact sizes carry no tilde");
        // MiB/s is binary too: 4.19 * 1024 * 1024.
        assert_eq!(speed, Some(4_393_533.0));
    }

    #[test]
    fn progress_parses_estimated_size_line_with_tilde() {
        let (pct, total, speed) = parse_ytdlp_progress(
            "[download]  42.1% of ~12.34MiB at    1.20MiB/s ETA 0:10",
        )
        .expect("must parse");
        assert_eq!(pct, 42);
        assert_eq!(total, Some(12_939_427), "estimated sizes use `of ~`");
        assert_eq!(speed, Some(1_258_291.0));
    }

    #[test]
    fn size_units_honour_binary_vs_decimal() {
        // yt-dlp emits both; confusing them skews every displayed size.
        assert_eq!(parse_ytdlp_size("1KiB"), Some(1024), "binary KiB");
        assert_eq!(parse_ytdlp_size("1kB"), Some(1000), "decimal kB");
        assert_eq!(parse_ytdlp_size("1MiB"), Some(1024 * 1024), "binary MiB");
        assert_eq!(parse_ytdlp_size("1MB"), Some(1_000_000), "decimal MB");
        assert_eq!(parse_ytdlp_size("500B"), Some(500), "bare bytes");
    }

    #[test]
    fn progress_parses_completed_line() {
        let (pct, _total, _speed) =
            parse_ytdlp_progress("[download] 100.0% of  246.27KiB at    3.44MiB/s ETA 00:00")
                .expect("must parse");
        assert_eq!(pct, 100);
    }

    #[test]
    fn progress_ignores_non_progress_lines() {
        // Extraction / metadata noise must not be mistaken for progress.
        assert!(parse_ytdlp_progress("[youtube] Extracting URL: https://x").is_none());
        assert!(parse_ytdlp_progress("[info] jNQXAC9IVRw: Downloading 1 format(s): 251").is_none());
        assert!(parse_ytdlp_progress("").is_none());
    }

    #[test]
    fn progress_percent_is_clamped_to_100() {
        let (pct, _, _) =
            parse_ytdlp_progress("[download] 142.0% of  1.00MiB at    1.00MiB/s ETA 0:01")
                .expect("must parse");
        assert_eq!(pct, 100, "percent must never exceed 100");
    }

    #[test]
    fn filename_strips_query_and_fragment() {
        assert_eq!(
            file_name_from_url("https://x.com/dir/file.zip?a=1#b"),
            "file.zip"
        );
    }

    #[test]
    fn filename_sanitizes_special_chars() {
        assert_eq!(
            file_name_from_url("https://x.com/a b<c>d\"e.txt"),
            "a_b_c_d_e.txt"
        );
    }

    #[test]
    fn filename_rejects_traversal() {
        // ".." must not survive â€” this was the path-traversal bug
        assert_eq!(file_name_from_url("https://x.com/.."), "download.bin");
        assert_eq!(file_name_from_url("https://x.com/dir/.."), "download.bin");
    }

    #[test]
    fn filename_handles_dot_and_empty() {
        assert_eq!(file_name_from_url("https://x.com/."), "download.bin");
        assert_eq!(file_name_from_url("https://x.com/"), "download.bin");
    }

    #[test]
    fn filename_allows_safe_chars() {
        assert_eq!(
            file_name_from_url("https://cdn.example.com/pkg/my-app_2.1-x64.exe"),
            "my-app_2.1-x64.exe"
        );
    }

    #[test]
    fn filename_handles_windows_paths() {
        // local .torrent dropped/browsed via drag-drop or file picker
        assert_eq!(
            file_name_from_url(r"C:\Users\me\Downloads\linux-distro.torrent"),
            "linux-distro.torrent"
        );
        assert_eq!(
            file_name_from_url("C:/Users/me/file.torrent"),
            "file.torrent"
        );
    }

    #[test]
    fn magnet_display_name_is_decoded_and_newline_safe() {
        let uri = "magnet:?xt=urn:btih:abcdef1234&dn=Ubuntu%2024.04%20Desktop.iso";
        assert_eq!(
            magnet_display_name(uri).as_deref(),
            Some("Ubuntu 24.04 Desktop.iso")
        );
        // A malicious name must not be able to inject newlines into the UI.
        let evil = "magnet:?xt=urn:btih:abc&dn=bad%0Aname.exe";
        let name = magnet_display_name(evil).unwrap_or_default();
        assert!(!name.contains('\n'));
    }

    #[test]
    fn magnet_without_display_name_is_none() {
        assert_eq!(magnet_display_name("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(magnet_display_name("magnet:?xt=urn:btih:abc&dn="), None);
    }

    #[test]
    fn classify_source_accepts_supported_kinds() {
        assert_eq!(
            classify_source("magnet:?xt=urn:btih:abc").unwrap(),
            SourceKind::Magnet
        );
        assert_eq!(
            classify_source("https://example.com/a.torrent").unwrap(),
            SourceKind::RemoteTorrent
        );
        assert_eq!(
            classify_source("https://example.com/a.torrent?token=1").unwrap(),
            SourceKind::RemoteTorrent
        );
        assert_eq!(
            classify_source("https://example.com/file.zip").unwrap(),
            SourceKind::Http
        );
        assert_eq!(
            classify_source(r"C:\Users\me\Downloads\a.torrent").unwrap(),
            SourceKind::LocalTorrent
        );
        assert_eq!(
            classify_source("  http://example.com/x  ").unwrap(),
            SourceKind::Http
        );
    }

    #[test]
    fn classify_source_rejects_ftp_and_junk() {
        // FTP was silently treated as an HTTP URL and produced a bogus row/request.
        let ftp = classify_source("ftp://example.com/file.zip").unwrap_err();
        assert!(ftp.contains("FTP"));
        assert!(classify_source("not a link").is_err());
        assert!(classify_source("").is_err());
    }

    #[test]
    fn category_maps_by_extension() {
        assert_eq!(category_for("movie.MKV"), "video");
        assert_eq!(category_for("song.flac"), "music");
        assert_eq!(category_for("setup.exe"), "app");
        assert_eq!(category_for("archive.7z"), "app");
        assert_eq!(category_for("report.pdf"), "document");
        assert_eq!(category_for("magnet-download"), "document");
    }

    #[test]
    fn percent_is_clamped() {
        assert_eq!(compute_percent(0, Some(1000)), 0);
        assert_eq!(compute_percent(500, Some(1000)), 50);
        assert_eq!(compute_percent(1500, Some(1000)), 100); // server overshoot
        assert_eq!(compute_percent(500, None), 0); // unknown size
        assert_eq!(compute_percent(500, Some(0)), 0); // guarded division
    }

    #[test]
    fn content_range_total_parsing() {
        assert_eq!(parse_content_range_total("bytes 200-999/1000"), Some(1000));
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
        assert_eq!(parse_content_range_total("garbage"), None);
    }

    #[test]
    fn resume_plan_honours_partial_content() {
        let plan = plan_resume(
            400,
            reqwest::StatusCode::PARTIAL_CONTENT,
            Some(600),
            Some("bytes 400-999/1000"),
        );
        assert_eq!(
            plan,
            ResumePlan {
                start_at: 400,
                total: Some(1000),
                truncate: false
            }
        );
    }

    #[test]
    fn resume_plan_restarts_when_server_ignores_range() {
        // The server returned 200 with the whole body: partial bytes are trash.
        let plan = plan_resume(400, reqwest::StatusCode::OK, Some(1000), None);
        assert_eq!(
            plan,
            ResumePlan {
                start_at: 0,
                total: Some(1000),
                truncate: true
            }
        );
    }

    #[test]
    fn resume_plan_fresh_download_truncates() {
        let plan = plan_resume(0, reqwest::StatusCode::OK, Some(1234), None);
        assert_eq!(
            plan,
            ResumePlan {
                start_at: 0,
                total: Some(1234),
                truncate: true
            }
        );
    }

    #[test]
    fn percent_encoded_names_are_decoded() {
        assert_eq!(
            file_name_from_url("https://x.com/My%20File.zip"),
            "My_File.zip"
        );
        // Local paths keep literal percent signs.
        assert_eq!(
            file_name_from_url(r"C:\torrents\100%-real.torrent"),
            "100_-real.torrent"
        );
    }

    #[test]
    fn windows_reserved_names_are_replaced() {
        assert_eq!(file_name_from_url("https://x.com/CON"), "download.bin");
        assert_eq!(file_name_from_url("https://x.com/nul.txt"), "download.bin");
        assert_eq!(file_name_from_url("https://x.com/com1"), "download.bin");
        // A trailing dot would otherwise desync the recorded path.
        assert_eq!(file_name_from_url("https://x.com/name."), "name");
    }

    #[test]
    fn ids_are_unique_even_in_the_same_millisecond() {
        let ids: std::collections::HashSet<String> = (0..500).map(|_| make_id()).collect();
        assert_eq!(ids.len(), 500);
    }

    #[test]
    fn unique_save_path_avoids_conflicts() {
        let dir = "C:\\faast-test-nonexistent";
        let taken = vec![
            format!("{}/download.bin", dir),
            format!("{}/download (1).bin", dir),
        ];
        assert_eq!(
            unique_save_path(dir, "download.bin", &taken),
            format!("{}/download (2).bin", dir)
        );
        assert_eq!(
            unique_save_path(dir, "other.zip", &taken),
            format!("{}/other.zip", dir)
        );
    }

    #[test]
    fn segment_count_scales_with_file_size() {
        assert_eq!(http_segment_count(1024), 8);
        assert_eq!(http_segment_count(1 << 30), 12);
        assert_eq!(http_segment_count(1 << 32), 16);
        assert_eq!(http_segment_count(8 << 30), 24);
        // A 39.9 GB installer gets 32 connections instead of 16.
        assert_eq!(http_segment_count((39u64 << 30) + (9u64 << 29)), 32);
    }

    #[test]
    fn ranges_cover_the_whole_file_exactly_once() {
        for (total, count) in [(1u64, 8usize), (8, 8), (9, 8), (1_000_000, 8), (u32::MAX as u64, 8)] {
            let ranges = split_ranges(total, count);
            assert!(!ranges.is_empty());
            assert_eq!(ranges.first().unwrap().start, 0);
            assert_eq!(ranges.last().unwrap().end, total - 1);
            let mut expected_start = 0u64;
            for range in &ranges {
                assert_eq!(range.start, expected_start);
                assert!(range.end >= range.start);
                expected_start = range.end + 1;
            }
            assert_eq!(expected_start, total);
            let sum: u64 = ranges.iter().map(|r| r.len()).sum();
            assert_eq!(sum, total, "ranges must not overlap or leave gaps");
            assert!(ranges.len() <= count.max(1));
        }
    }

    #[test]
    fn ranges_handle_zero_and_one() {
        assert!(split_ranges(0, 8).is_empty());
        let one = split_ranges(1, 8);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0], ByteRange { start: 0, end: 0 });
    }

    #[test]
    fn segment_meta_round_trips_and_rejects_garbage() {
        let dir = std::env::temp_dir().join(format!("faast-meta-{}", make_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save_path = dir.join("file.bin").to_string_lossy().to_string();

        assert!(read_segment_meta(&save_path).is_none());
        write_segment_meta(
            &save_path,
            SegmentMeta {
                total: 1234,
                count: 7,
            },
        )
        .unwrap();
        assert_eq!(
            read_segment_meta(&save_path),
            Some(SegmentMeta {
                total: 1234,
                count: 7
            })
        );

        std::fs::write(segment_meta_path(&save_path), "{not json").unwrap();
        assert!(read_segment_meta(&save_path).is_none());
        std::fs::write(
            segment_meta_path(&save_path),
            r#"{"total":100,"count":0}"#,
        )
        .unwrap();
        assert!(read_segment_meta(&save_path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn segment_part_paths_are_unique() {
        let base = "C:\\tmp\\file.bin";
        assert_ne!(segment_part_path(base, 0), segment_part_path(base, 1));
        assert_eq!(segment_part_path(base, 3), "C:\\tmp\\file.bin.faast-part3");
        assert_eq!(segment_meta_path(base), "C:\\tmp\\file.bin.faast-meta");
    }

    #[test]
    fn torrent_display_names_strip_control_chars() {
        assert_eq!(
            clean_display_name("  Big Buck Bunny\n"),
            Some("Big Buck Bunny".to_string())
        );
        assert_eq!(clean_display_name("\u{0}\t "), None);
        assert_eq!(clean_display_name(""), None);
    }

    #[test]
    fn safe_path_component_keeps_spaces_but_not_separators() {
        assert_eq!(safe_path_component("Big Buck Bunny"), "Big Buck Bunny");
        assert_eq!(safe_path_component("../evil"), ".._evil");
        assert_eq!(safe_path_component("a/b\\c:d"), "a_b_c_d");
        assert_eq!(safe_path_component("name.  "), "name");
        assert_eq!(safe_path_component("   "), "torrent");
    }

    /// Live check against the throttled local Range server:
    ///   node faast_range_server.mjs 4199 ...
    ///   $env:FAAS_RANGE_BASE="http://127.0.0.1:4199"; cargo test --lib segmented
    #[tokio::test]
    async fn segmented_core_downloads_and_merges_in_order() {
        let base = match std::env::var("FAAS_RANGE_BASE") {
            Ok(v) => v,
            Err(_) => {
                eprintln!("SKIP: set FAAS_RANGE_BASE to the local test server");
                return;
            }
        };
        let client = reqwest::Client::new();
        let url = format!("{}/big.bin", base.trim_end_matches('/'));
        let total = match probe_http_plan(&client, &url).await.expect("probe") {
            HttpPlan::Segmented(total) => total,
            other => panic!("expected a segmented plan, got {:?}", other),
        };

        let dir = std::env::temp_dir().join(make_id());
        std::fs::create_dir_all(&dir).unwrap();
        let save_path = dir.join("big.bin").to_string_lossy().to_string();
        let ranges = split_ranges(total, 8);
        let counter = AtomicU64::new(0);

        let results = futures_util::future::join_all(ranges.iter().enumerate().map(|(i, range)| {
            let client = client.clone();
            let url = url.clone();
            let part_path = segment_part_path(&save_path, i);
            let range = *range;
            let counter = &counter;
            async move { download_segment(&client, &url, &part_path, range, 0, counter).await }
        }))
        .await;
        for result in results {
            result.expect("segment download");
        }
        assert_eq!(counter.load(Ordering::Relaxed), total);

        merge_segments(&save_path, &ranges).await.expect("merge");
        let bytes = std::fs::read(&save_path).unwrap();
        assert_eq!(bytes.len() as u64, total);
        // The test server fills the payload with `i % 251`.
        assert!(
            bytes.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8),
            "merged bytes are out of order"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn probe_picks_the_right_plan() {
        let base = match std::env::var("FAAS_RANGE_BASE") {
            Ok(v) => v,
            Err(_) => {
                eprintln!("SKIP: set FAAS_RANGE_BASE to the local test server");
                return;
            }
        };
        let base = base.trim_end_matches('/');
        let client = reqwest::Client::new();

        assert_eq!(
            probe_http_plan(&client, &format!("{}/big.bin", base))
                .await
                .unwrap(),
            HttpPlan::Segmented(1_000_000)
        );
        // This endpoint ignores Range and answers 200 with the whole body.
        assert_eq!(
            probe_http_plan(&client, &format!("{}/norange.bin", base))
                .await
                .unwrap(),
            HttpPlan::Single
        );
        assert!(probe_http_plan(&client, &format!("{}/missing", base))
            .await
            .is_err());
    }
}
