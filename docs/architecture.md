# Architecture

> **Last reviewed:** 2026-10-30 · **Last updated:** 2026-10-30 · **Change:** minor (crate table recounted and completed; gates and platforms as they are now) · **Target:** Adobe Lightroom Classic 15.5 / Lightroom 9.6

How LightCraft is built today. A longer design document exists only as a local, uncommitted
note; this page is the committed, current summary. Sizes are Rust lines counted
on 2026-10-30 (≈ 317,000 tracked lines in 740 files: 29 library crates, 3 apps, `testkit` and `xtask`).

## Layers

`cargo xtask layers` (part of `cargo xtask ci`, `xtask/src/layers.rs`) enforces that nothing below
L5 depends on egui, eframe, winit, rfd or `craft-ui`, and that each crate only uses lower layers.

| Layer | Crates (lines) | Role |
|---|---|---|
| L0 | `geom` (1.7k), `color` (0.9k), `raster` (1.4k), `tiff` (2.4k), `fetch` (1.6k), `sysmem` (0.1k), `heif` (2.1k, optional) | Matrices and warps, colour spaces and transfer functions, float image buffers, TIFF/IFD read and write, pinned HTTPS model downloads, the only `unsafe` (two FFI calls: `malloc_zone_pressure_relief` on macOS, `GlobalMemoryStatusEx` on Windows), HEIC |
| L1 | `raw` (37.0k), `codecs` (10.8k), `meta` (6.1k), `develop` (3.2k), `denoise-core` (2.3k), `denoise` (3.3k), `faces` (6.3k), `inpaint` (0.8k), `geocode` (0.4k), `slideshow` (1.0k), `scenes` (1.0k) | Raw decoders and demosaic, DNG colour model; standard codecs and encoders; EXIF/XMP/IPTC; the non-destructive edit model and control specs; AI denoise; face detection and recognition; content-aware fill; offline reverse geocoding; slideshow layout and slide composer; procedural demo photos |
| L2 | `pipeline` (12.3k), `lensdb` (1.0k) | The develop pipeline: scene-referred float, resolution independent; the CPU reference (oracle) for everything else; lens correction profiles |
| L3 | `catalog` (13.5k), `gpu` (6.9k), `preview` (2.0k), `merge` (4.6k), `segment` (2.3k) | Library model and persistence; wgpu compute twin of the pipeline; thumbnail/preview cache and job pool; HDR and panorama merge; SAM 3 segmentation |
| L4 | `engine` (91.1k) | Session, command registry (everything is a command), history, render and export jobs, import, Lightroom catalog and preset import, faces index, view-models |
| L5 | `ui-egui` (78.1k), `craft-ui` (5.7k) | The Lightroom-style egui interface (swappable); reusable egui controls and icon catalogs (vendored from craft-libs) |
| Standalone | `craft-docpath` (1.0k) | Whether a path or file URL stored in a document is a local file (vendored from craft-libs) |
| L6 | `apps/lightcraft` (6.9k), `apps/lightcraft-console` (0.1k), `apps/lightcraft-web` (3.5k), `xtask` (5.5k), `testkit` (0.4k, dev-dependency only) | Desktop app (eframe + wgpu, native menus, control server) and its CLI subcommands (run, render, snapshot, commands, calibrate…; `docs/cli.md`), Windows console launcher (`lightcraft.com`), WASM build, tooling; test helpers (skips the CI report lists) |

## Data model

- **Photo** records in `catalog` hold the original's path, content hash, metadata, ratings, flags,
  labels, keywords, faces and the current `develop::Settings`. Albums, smart albums (rule trees),
  stacks, virtual copies, versions and folder records live alongside.
- **Persistence** (`crates/catalog/src/journal.rs`): an append-only journal of human-readable
  operations plus periodic snapshots, compacted in the background; torn appends and crashes are
  tested (`tests_torn_append.rs`, `tests_journal.rs`). A lock file guards against two writers.
- **Edits** are a `develop::Settings` value (every slider is a control spec with id, range and
  default), normalized to image coordinates so previews and exports match. Process versions
  (`docs/process-versions.md`) keep old edits rendering the same after a retune.
- **Sidecars**: XMP read and write with Adobe `crs:` interop (`docs/xmp-interop.md`); Lightroom
  Classic `.lrcat` import through a pure-Rust SQLite reader (`docs/lightroom-catalog-import.md`).

## Rendering

1. **Decode** (`raw`, `codecs`): raw files to linear camera RGB (demosaic, black/white levels,
   opcodes, highlight handling) and a camera-to-XYZ colour model; standard images to linear light
   through their ICC profile.
2. **Develop** (`pipeline`): white balance, profile, exposure and tone (local Laplacian
   highlights/shadows), curves, HSL / colour mixer, point colour, colour grading, texture, clarity,
   dehaze, detail (sharpening, NR, AI denoise result), optics (lens profile, CA, defringe,
   vignette), geometry (crop, Upright, transforms), masks with local adjustments, heal/clone, grain,
   effects. Stage caching and a source pyramid make slider drags cheap.
3. **GPU** (`gpu`): the same stages in WGSL on Metal / DX12 / Vulkan, checked against the CPU path
   (within 1/255); GPU errors fall back to the CPU (`docs/gpu-pipeline.md`).
4. **Display and export**: display-profile conversion (`docs/display-profiles.md`); export through
   one encoder for app, CLI and web (`lightcraft_engine::export`).

## Agent control

Every action is an engine command (id, label, menu path, shortcut, params, enabled, run). The UI,
the CLI (`docs/cli.md`) and the JSON-lines control channel (`docs/control-protocol.md`) all
dispatch by id, and headless snapshots render the UI without a window.

## Quality gates

`cargo xtask ci`, run locally: fmt, clippy `-D warnings` with the never-crash lints, the HEIF build,
tests, `parity` (every id and path cited in [parity-checklist.md](parity-checklist.md) exists),
`layers`, `assets` (every asset attributed, no Adobe formats), `deps` (`cargo deny check` against
`deny.toml`: no C/C++ builds, copyleft licences or advisories) and the WASM build. The only hosted
workflow is `.github/workflows/packaging-lint.yml`, a quick static check of the packaging. Releases are
built locally (`make release`, [releasing.md](releasing.md)). The supported targets are Apple
silicon macOS, Windows x64 and the web; the Linux packages (`packaging/linux`) are kept building but
are not the tested target.

`ci` also probes the machine: OS, CPUs, RAM, the GPU adapters of each backend (from the ignored
`gpu_probe` test of `lightcraft-gpu`, run after the workspace tests so it reuses their build; xtask
has no wgpu) and the corpus. Tests
that need what is missing call `lightcraft_testkit::skip!(need, reason)`, which appends a JSON line to
the file `LIGHTCRAFT_SKIP_LOG` names. The run ends with the machine and the skips by need and reason,
and writes `target/ci/report.json` (commit, dirty flag, machine, capabilities, each step's result and
time, skips) and `target/ci/summary.md` (Markdown for a PR; `--summary-md` prints it). The report holds
no host name, user name or absolute path, and nothing is uploaded. `--hardware` adds a step that runs
the `lightcraft-gpu` tests once per backend with a hardware adapter (`LIGHTCRAFT_GPU_BACKEND`).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-30 | minor | Crate table recounted and completed (`craft-ui`, `craft-docpath`, `geocode`, `inpaint`, `lensdb`, `slideshow`; `heif` is L0); `deps` gate; no hosted CI but the packaging lint |
| 2026-10-21 | minor | Inter is the only font: no optional font input, so the CI machine probe no longer reports one |
| 2026-10-11 | minor | Quality gates: the CI machine probe, visible skips (`testkit`) and `target/ci/report.json` |
| 2026-10-10 | major | Created from the code (crate sizes, layers, data model, rendering, control, gates) |
