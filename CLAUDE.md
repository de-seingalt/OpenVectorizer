# OpenVectorizer — agent guide

Offline raster→SVG vectorizer aiming at vectorizer.ai-level quality. Rust engine compiled to WASM,
Svelte UI, shipped as a static web app (PWA) and a portable Tauri desktop app.
The project owner is French-speaking and does not write Rust: **answer and write docs/UI text in
French**; code, identifiers, comments and commit messages in English.

## Layout
```
crates/core    engine: RGBA + Options -> SVG. Pure, no I/O, must compile to wasm32. Types exported to TS (ts-rs).
crates/eval    quality metrics: renders SVG with resvg, compares to source (ΔE Oklab, PSNR, node count), heatmaps.
crates/cli     `openvectorizer convert|eval` — the agent's main feedback loop.
crates/wasm    wasm-bindgen wrapper used by the UI's Web Worker.
app/           Svelte 5 + TS strict + Tailwind 4 + shadcn-svelte UI (Vite).
app/src-tauri  Tauri 2 desktop shell (dialogs + file access only; engine stays in WASM).
testdata/      benchmark: ground-truth SVGs (synthetic, flags, emoji, icons) × degradations (jpeg, half, blur, noise); see testdata/README.md.
docs/          STACK.md (decisions), ROADMAP.md (milestones), ETUDE-PREALABLE.md (research).
```

## Commands
| Goal | Command |
|---|---|
| Everything CI checks | `scripts/check.sh` (`--quick` skips e2e) |
| Engine tests | `cargo test` |
| Quality benchmark | `cargo run --release -p openvectorizer-cli -- eval testdata --baseline testdata/baseline.json` (`--filter flags`, `--variants clean,jpeg35`) → `target/eval/index.html` |
| Re-download third-party corpus | `scripts/fetch-corpus.sh` |
| Convert one file | `cargo run --release -p openvectorizer-cli -- convert in.png -o out.svg` |
| Build WASM for the UI | `scripts/build-wasm.sh` |
| UI dev server | `cd app && pnpm dev` (http://localhost:5173) |
| UI checks | `cd app && pnpm check && pnpm lint && pnpm test && pnpm test:e2e` |
| Desktop app (needs WebKitGTK libs) | `cd app && pnpm tauri dev` / `pnpm tauri build` |

## Rules
- **Quality is measured, not eyeballed.** Any engine change: run `eval`, report the ΔE/p95/nodes delta vs
  baseline in the PR. Inspect `target/eval/*.diff.png` heatmaps (Read them as images) to locate defects.
  Only update `testdata/baseline.json` when an improvement is validated, in a dedicated commit.
- Never commit personal or copyrighted images to testdata: the repo is public.
- **Never regress**: `eval --baseline` fails if mean ΔE worsens by more than 0.25.
- `crates/core` stays pure: no `std::fs`, threads, clocks, or platform crates. Must build for
  `wasm32-unknown-unknown`.
- Changing `Options`/`Output` in core regenerates `app/src/lib/engine/generated/*.ts` on `cargo test`;
  commit them, and update `DEFAULT_OPTIONS` in `app/src/lib/engine/index.ts` to mirror `Options::default()`.
- UI code talks to the engine **only** through `$lib/engine` and to the OS **only** through `$lib/platform`.
- All user-visible strings go in `app/src/lib/i18n.ts` (fr + en).
- Use shadcn-svelte components (`pnpm dlx shadcn-svelte@latest add <name>`) rather than hand-rolled widgets;
  don't edit `src/lib/components/ui/` except to regenerate it.
- Licenses: dependencies must be permissive (MIT/Apache/BSD/Zlib/ISC/MPL). **No GPL code** (Potrace,
  AutoTrace, libimagequant, libdepixelize) — not even "inspired by" copies.
- clippy pedantic with `-D warnings`, TS `strict`: fix warnings, don't silence them without a comment.
- `wasm-bindgen` crate version is pinned (`=x.y.z`) and must match `wasm-bindgen-cli` used in scripts/CI.

## Workflow
- One milestone task per branch/PR (`feat/m1-planar-map`, …). See `docs/ROADMAP.md`.
- Each pipeline stage = its own module in `crates/core/src/` with unit tests; replace the `baseline`
  (VTracer) stage by stage.
- Run `scripts/check.sh` before committing. Conventional commits (`feat:`, `fix:`, `perf:`, `docs:`…).
- Commit trailer: only `Co-Authored-By: Claude …`. **Never add session links (`Claude-Session: …`) or any personal data** to commits or PRs.
- Releases: push a `vX.Y.Z` tag → GitHub Actions builds AppImage, Windows portable `.exe` + installer, macOS `.dmg`.
