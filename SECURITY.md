# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Current alpha |

Only the latest release receives security fixes.

## Reporting a Vulnerability

**Do not open a public issue.** Email **security@faast.local** (placeholder — replace with real contact) with:

- Description of the vulnerability
- Steps to reproduce
- Impact assessment
- Suggested fix (if any)

We aim to acknowledge within 48 hours and ship a fix within 14 days for critical issues.

## Scope

- Remote code execution via download handling (malicious HTTP headers, torrent metadata, yt-dlp output)
- Path traversal in save folder / file naming
- Local privilege escalation via installer or auto-update (when implemented)
- Data exfiltration (Faast collects no telemetry; verify this claim)

Out of scope: issues in upstream dependencies (`reqwest`, `librqbit`, `yt-dlp`, `tauri`, `webview2`) — report those upstream.