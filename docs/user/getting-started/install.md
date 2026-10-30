---
title: Install LightCraft
description: Download LightCraft for an Apple silicon Mac or 64-bit Windows, or build it from source, and open your photo library.
order: 10
verified: 0.7.1
---

Download the file for your system from the [releases page](https://github.com/joshfinnie/lightcraft/releases) and install it. If there is no release for your system yet, [build LightCraft from source](#build-from-source). In the file names, `<version>` stands for the version number, for example `0.10.0`.

LightCraft is tested on Apple silicon Macs, 64-bit Windows and the web version.

## Windows

1. Open the [releases page](https://github.com/joshfinnie/lightcraft/releases).
2. Download `lightcraft-<version>-windows-x64.msi`. It fits most PCs (Intel and AMD, 64-bit).
3. Run the file. The installer asks where to install. The default is `C:\Program Files\LightCraft`, and upgrades stay in the folder you chose.
4. At the end, choose the option to start LightCraft.

To run it without installing, download `lightcraft-<version>-windows-x64-portable.zip`.

## macOS

1. Download `lightcraft-<version>-macos-universal.dmg`.
2. Open the file and drag LightCraft to **Applications**.

If macOS refuses to open LightCraft because it cannot check it for malicious software, open **System Settings** > **Privacy & Security** and click **Open Anyway** next to the message about LightCraft.

## In a web browser

A release can also have `lightcraft-web-<version>.zip`, a static site that runs LightCraft in a modern browser. Host it on any static web server.

## Build from source

You need [Rust](https://www.rust-lang.org/tools/install) 1.95 or newer. Then, in a terminal:

    git clone https://github.com/joshfinnie/lightcraft.git
    cd lightcraft
    cargo run --release -p lightcraft

The first build takes a while. The [README](https://github.com/joshfinnie/lightcraft/blob/main/README.md) has more commands, including the web version.

## Linux

There are no Linux downloads, and Linux is not tested. You can build the packages yourself from a source checkout: `packaging/linux/package.sh` makes an AppImage, a `.deb`, an `.rpm` and a `.tar.gz`, and `packaging/linux/flatpak-bundle.sh` turns the `.tar.gz` into a Flatpak. On a Wayland desktop, LightCraft runs through XWayland when it is available, because only X11 gives it file drag-and-drop.

## First run

1. Start LightCraft. It opens your library at `~/Pictures/LightCraft Library`. A new library starts with demo photos.
2. To bring in your own photos, see [Import photos](../guides/import-photos.md).

## Still stuck?

Open an issue in the [issue tracker](https://github.com/joshfinnie/lightcraft/issues).
