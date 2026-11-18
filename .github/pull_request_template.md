## Description

<!-- What does this PR do? Link issue if applicable. -->

## Type of change

- [ ] Bug fix
- [ ] Feature (aligned with reliability wedge)
- [ ] Docs update
- [ ] Refactor / cleanup
- [ ] Test addition
- [ ] CI / build

## Testing

- [ ] `cargo test --lib` passes (52 tests)
- [ ] `cargo test --test engine_live` passes
- [ ] `npm run build` clean
- [ ] Manual smoke test: `npm run tauri dev` → add HTTP + torrent + yt-dlp downloads

## Checklist

- [ ] Conventional commit messages (`feat:`, `fix:`, `docs:`, etc.)
- [ ] No `console.log` / `dbg!` left in
- [ ] Updated `README.md` / `docs/*.md` if user-facing change
- [ ] `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` clean
- [ ] Focus rings use `currentColor`, spacing on 4px grid, semantic tokens

## Screenshots (if UI change)

<!-- Drag/drop or link -->