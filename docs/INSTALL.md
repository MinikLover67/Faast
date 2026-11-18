# Installation & distribution

## End users (today)

Windows 10/11 x64. Download `Faast_0.1.0_x64-setup.exe` (NSIS) or `Faast_0.1.0_x64_en-US.msi` from the GitHub Releases page and run it. The installer creates a desktop shortcut and a Start Menu entry.

> The installer is currently **unsigned**. Windows SmartScreen will warn on first run. Signing is prepared below but requires a certificate.

## Package managers (manifests prepared, not yet published)

- **Scoop:** `packaging/scoop/faast.json` — validate with `scoop bucket add faast <repo>\packaging\scoop`, then `scoop install faast`.
- **winget:** `packaging/winget/` — three manifests (installer, version, default locale). Submit via `wingetcreate submit` once the installer URL is stable and the package is public.

These files are ready to submit; submission needs a public repo URL and a published release asset URL.

## Code signing (blocked on a certificate)

`scripts/sign-installer.ps1` signs the NSIS installer with `signtool.exe` if `FAST_SIGN_PFX` and `FAST_SIGN_PASSWORD` are set in the environment:

```powershell
$env:FAST_SIGN_PFX = 'C:\path\to\cert.pfx'
$env:FAST_SIGN_PASSWORD = '...'
powershell -File scripts\sign-installer.ps1
```

An OV/EV code-signing certificate is required. Without one the installer remains unsigned; that is the single biggest trust gap in the current distribution.

## Building from source

```bash
npm install
npm run tauri build
```

Outputs:
- `src-tauri\target\release\bundle\nsis\Faast_0.1.0_x64-setup.exe`
- `src-tauri\target\release\bundle\msi\Faast_0.1.0_x64_en-US.msi`

## Release checklist

1. Update version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
2. `npm run build && npm run e2e`
3. `cd src-tauri && cargo test --lib && cargo test --test engine_live && cargo test --test real_net && cargo test --test torrent_live`
4. `npm run tauri build`
5. Sign the NSIS installer (see above).
6. Upload both installers to a GitHub Release with checksums.
7. Update the Scoop manifest hash and the winget installer URL.
8. Publish the release; only then submit/refresh package-manager manifests.
