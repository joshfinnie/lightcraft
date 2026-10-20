# LightCraft (Josh's fork)

This is a personal fork of [LightCraft](https://github.com/storytold/lightcraft), the open-source,
pure-Rust photo library and raw developer by the ArtCraft team. It is not an official build and is
not affiliated with or endorsed by the ArtCraft team.

The fork tracks upstream `main` and adds what I think makes the app better for how I use it:
upstream pull requests I've reviewed and merged early, fixes from my own code audits, and less
of what I don't need. It is built for and tested on two machines: an Apple silicon Mac and a
Windows x64 PC. The web build is kept working too.

<p align="center">
  <img src="docs/images/hero-tetons.jpg" alt="LightCraft's Edit view with Ansel Adams' The Tetons and the Snake River in the loupe, the Light and Effects panels open on the right, and the four showcase photos in the filmstrip" width="100%">
  <br>
  <sub><i>Ansel Adams, "The Tetons and the Snake River" (1942). Public domain, U.S. National Archives. Developed in LightCraft.</i></sub>
</p>

## What's different from upstream

- **Upstream PRs merged ahead of upstream**, picked for safety first: refusing malformed
  commands, mask and spot targets before they change anything; bounded TIFF parsing and
  overflow-safe size checks for hostile files; export and GPU failures that stop cleanly
  instead of crashing; MCP tools that write files marked as destructive.
- **A locked-down control port.** The automation channel requires a per-run token and drops
  idle connections after 5 minutes (see [Automation](#automation)).
- **Fixes of my own**, each with a regression test: RGBA images within the pixel limit no
  longer refused, click and drag counts bounded on the control channel, one format for
  shutter, aperture, ISO and focal length everywhere, real timestamps in the web build, a
  flaky MCP test on macOS, and a warning-free wasm build.
- **Less to maintain.** No FreeBSD or Nix packaging and no CI for platforms I don't use.

`git log --oneline upstream/main..main` lists every change.

## What LightCraft does

A non-destructive raw developer and photo library modelled on Lightroom Classic's workflow:
Light, Color, Effects, Tone Curve, colour grading, masking, presets and profiles, crop and
geometry, black and white, ratings, flags, labels, albums, smart collections and search. Native
on macOS and Windows, in the browser via WebAssembly, and drivable by AI agents over MCP.
Upstream's [ROADMAP.md](ROADMAP.md) has the detailed feature status.

## Build and run

Rust 1.95 or newer.

```sh
git clone git@github.com:joshfinnie/lightcraft.git && cd lightcraft
cargo run --release -p lightcraft                       # opens ~/Pictures/LightCraft Library (a new one starts with demo photos)
cargo run --release -p lightcraft -- ~/Pictures/trip    # import a folder (duplicates skipped)
cargo run --release -p lightcraft -- --memory           # throwaway in-memory demo session, writes nothing
cargo xtask web --serve                                 # the browser build at http://127.0.0.1:8080/
cargo run --release -p lightcraft-cli -- render photo.jpg -o out.jpg --set light.exposure=0.5
```

The web build needs the `wasm32-unknown-unknown` target and the matching `wasm-bindgen` CLI;
`cargo xtask web` prints the install command. See [docs/web.md](docs/web.md).

Chinese and Japanese text need the optional [craft-fonts](https://github.com/storytold/craft-fonts)
build input (`CRAFT_FONTS_DIR=../craft-fonts`); without it everything works except those glyphs.

Logs go to `~/Library/Application Support/LightCraft/logs/` on macOS and
`%APPDATA%\LightCraft\logs\` on Windows (**Help ▸ Open Log Folder**).

## Automation

`--control PORT` opens a JSON-lines control channel on localhost; `lightcraft-cli mcp --connect`
drives the running app over MCP, and `lightcraft-cli mcp --demo` runs headless. In this fork every
connection must authenticate first:

```json
{"method": "auth", "params": {"token": "<64 hex digits>"}}
```

The app writes a fresh token to `control-token-<PORT>` in its settings folder on each run, or
takes one from `--control-token` / `LIGHTCRAFT_CONTROL_TOKEN`. Protocol details are in
[docs/control-protocol.md](docs/control-protocol.md) and [docs/mcp.md](docs/mcp.md).

## Working on the fork

- `cargo xtask ci` (fmt, clippy, tests, parity, layers, assets, wasm) passes before anything
  lands on `main`. There is no hosted CI; this is the gate.
- Upstream PRs are fetched with `git fetch upstream pull/<N>/head:pr/<N>`, merged in small
  batches on an integration branch, and land on `main` only after a green `cargo xtask ci`.
- The engineering rules are upstream's and still apply: clean-room, pure Rust, no proprietary
  assets, and never crash. See [AGENTS.md](AGENTS.md).

## License and credits

LightCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Copyright (c) 2026 ArtCraft Team and the LightCraft contributors; required notices are in
[NOTICE](NOTICE). This fork is based on LightCraft by the ArtCraft team.

Bundled assets keep their own open licenses and are listed in
[assets/ATTRIBUTION.md](assets/ATTRIBUTION.md). The showcase photograph is a public-domain work by
Ansel Adams (1942, U.S. National Archives). UI font: Inter (SIL OFL 1.1).
