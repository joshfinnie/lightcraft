# The command line

The `lightcraft` program is also LightCraft's command line: when its first argument is a subcommand, it runs that
without opening a window and exits (`apps/lightcraft/src/cli/`). Anything else starts the app.

```text
lightcraft run [--demo | --library DIR] [--import PATH]… [--script FILE|-] [--keep-going] CMD [key=value…]…
lightcraft render IN -o OUT [--set control=value]… [--settings FILE] [--preset ID] [--size N] [--quality Q] [--opt KEY=VALUE]…
lightcraft snapshot [--demo | --library DIR] [--script FILE.jsonl] [-o OUT.png] [--size WxH] [--scale S] [FILES…]
lightcraft merge hdr|panorama|hdr-panorama|hdr-batch [OPTIONS] FILES…
lightcraft synth-merge hdr|panorama -o DIR
lightcraft denoise-serve [OPTIONS]
lightcraft commands [--json]
lightcraft controls [--json]
lightcraft calibrate [--max N] [--out DIR] FOLDERS/FILES…
```

`lightcraft --help` prints the app's options and every subcommand's.

Where the program is:

- **macOS:** `/Applications/LightCraft.app/Contents/MacOS/LightCraft run …` (an alias such as
  `alias lightcraft=/Applications/LightCraft.app/Contents/MacOS/LightCraft` makes it `lightcraft run …`).
- **Windows:** `lightcraft.exe` is a GUI program, which gets no console, so the packages put `lightcraft.com`
  beside it (`apps/lightcraft-console`). A terminal that names `lightcraft` runs the `.com` (PATHEXT lists `.COM`
  before `.EXE`), and it runs `lightcraft.exe` with the terminal's input and output, then exits with its status.
  Double-clicking `lightcraft.exe` opens the app as before.
- **Linux:** `/usr/bin/lightcraft`, or `flatpak run com.joshfinnie.lightcraft run …`.
- **From the source tree:** `cargo run --release -p lightcraft -- run …`.

## `lightcraft run`

Run a chain of commands in a session of its own and read one JSON line per command: `{"command", "ok", "result",
"ms"}`, or `{"command", "ok": false, "error", "code"}` with an [error code](control-protocol.md#error-codes). The
exit status is non-zero when a command failed. A word without `=` starts the next command; `key=value` values are
JSON when they parse, else strings; a `'{…}'` argument merges a JSON object into the params. Quote values with
brackets or spaces for your shell (`'ids=[3,4]'`; zsh treats `[…]` as a glob).

```sh
# import, edit, export
lightcraft run --import ~/Pictures/a.dng develop.set control=light.exposure value=0.7 \
    develop.auto app.export path=/tmp/a.jpg shortEdge=1080 colorSpace=displayP3
# a persistent library: edits are saved, later invocations see them
lightcraft run --library ~/lc-lib --import ~/Pictures/shoot library.info
lightcraft run --library ~/lc-lib library.select ids=[3] develop.get
# JSON lines from a file or stdin: {"command": id, "params": {…}} (or {"method": "engine.commands"})
lightcraft run --demo --script steps.jsonl --keep-going
```

The commands on the command line run first, then the script's lines, each as it is read: a program can pipe one
command at a time into `--script -` and read each reply before it sends the next. The session is the CLI's own,
never the running app's: a library the app has open is refused (the app holds it), and `ui.*` methods need the
app's window. To drive the running app, start it with `--control PORT` and talk to its
[control channel](control-protocol.md) (`docs/showcase/run.py` sends JSON-lines files); to script the UI
without a window, use `lightcraft snapshot --script`.

`app.export` takes the Export dialog's options (`lightcraft commands` lists them) and writes the files; with
several photos selected and no `ids` it exports the selection, as the app does.

## Other subcommands

```sh
lightcraft render in.dng -o out.tif --opt colorSpace=displayP3 --opt bitDepth=16 --opt percent=50
lightcraft render in.dng -o out.jpg --set light.exposure=0.5 --set light.contrast=20 --size 2048
lightcraft render in.jpg -o out.png --settings look.json --preset <presetId>
lightcraft render in.dng -o out.jpg --opt addToPhotos=true --opt photosAlbum=Trip   # macOS: then into Apple Photos
lightcraft snapshot --demo -o grid.png --size 1600x1000                            # the UI, rendered on the CPU
lightcraft commands [--json]   # the command registry
lightcraft controls [--json]   # develop control ids and ranges
lightcraft calibrate --max 300 ~/Pictures/2026   # camera colour profiles (docs/camera-preview-colour.md)
lightcraft merge hdr a.dng b.dng c.dng           # Photo Merge (docs/merge.md)
lightcraft denoise-serve --listen 100.101.102.103:7990   # AI Denoise for other computers (docs/denoise.md)
```

`snapshot` is described in [control-protocol.md](control-protocol.md#headless-rendering-no-window-no-gpu).

## Add to Apple Photos (macOS)

`app.export` takes `addToPhotos: true` and `photosAlbum` to hand the files it wrote to Apple Photos, into that
album (a top-level album of exactly that name, made when missing). The result's `applePhotos` is the import job:
`{job, running, requested, album}` plus `{imported, ids, warning?}`, `{error}` or `{skipped}` once Photos is done;
the files are written either way. The CLI waits for Photos by default, and a process about to end waits for an
import still running. `export.addToPhotos {paths, album?, wait?}` does the same for files already on disk. The
first time, macOS asks the user to allow the terminal running `lightcraft` to control Photos. See
[apple-photos.md](apple-photos.md).

## Tests

`apps/lightcraft/tests/cli.rs` runs the real binary: `render`, `run` (chains, libraries, error codes, a library
held by another process), `snapshot`, `commands`, `calibrate`. `apps/lightcraft-console/tests/forwarding.rs`
checks that the launcher passes arguments, input, output and the exit status through.
