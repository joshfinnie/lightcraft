# Releasing LightCraft

Releases are built locally with the Makefile, one platform per machine: there is no hosted release
pipeline. `make release` packages for the platform it runs on, and `make publish` uploads the result
to a draft GitHub Release.

## Make a release

1. Set the workspace version in `Cargo.toml` (for example, `cargo xtask version set 1.2.0`). Use a
   semantic version such as `1.2.0` or `1.2.0-rc.1`; a version with a hyphen is published as a
   prerelease.
2. Run `make check` on the commit you are releasing.
3. On each platform you ship, run the following. The supported targets are Apple silicon macOS and
   Windows x64 (the web zip comes with either); the Linux packages are kept building, but Linux is not
   the tested target.

   ```sh
   make release          # installers for this platform, the web zip and SHA256SUMS.txt in dist/release/
   make publish          # upload dist/release/ to the draft GitHub Release v<version>
   ```

   The first `make publish` creates the draft `LightCraft v<version>`; later ones (other platforms)
   add their files to it. On Windows, run both in Git Bash (GNU make, PowerShell 7 and WiX on `PATH`).
4. Review the draft on GitHub, then publish it.

`make release` runs `make tools` first, so a fresh machine needs nothing but `make`. Pass
`RELEASE_ARCH` to pick the architecture: `universal` (default), `aarch64` or `x86_64` on macOS;
`x64` (default), `x86` or `arm64` on Windows. Linux builds for the machine's own architecture.

## What each platform produces

The platform scripts in [`packaging/`](../packaging/) write to `dist/release/`:

- macOS (`packaging/macos/package.sh`): the app DMG and the CLI ZIP.
- Windows (`packaging/windows/package.ps1`): an MSI installer and a portable ZIP.
- Linux (`packaging/linux/package.sh`): AppImage (with its `.AppImage.zsync`), `.deb`, `.rpm` and
  `.tar.gz`. Each AppImage embeds
  `gh-releases-zsync|joshfinnie|lightcraft|latest|lightcraft-*-linux-<arch>.AppImage.zsync`, so
  AppImageUpdate fetches only the changed blocks from the latest published (non-pre-) release. It
  needs `nfpm` for `.deb`/`.rpm`; `appimagetool` is downloaded if missing. Build on an older distro
  (Ubuntu 22.04, glibc 2.35) so the binaries run on newer ones.
- Every platform: `lightcraft-web-<version>.zip` (`packaging/web/package.sh`) and `SHA256SUMS.txt`
  for the files in `dist/release/`.

A Flatpak bundle can be made from the Linux tarball with `packaging/linux/flatpak-bundle.sh`
(`packaging/linux/flatpak/com.joshfinnie.lightcraft.bundle.yml`; no Rust build).

## Fonts

Inter (`assets/fonts/`) is the only font, embedded in every build. The packages include its OFL
licence (`OFL-Inter.txt`).

## Signing

Signing is optional: without credentials, packaging continues with unsigned artifacts and warnings.

- **macOS:** `MACOS_SIGN_IDENTITY` (a codesign identity in your keychain; default `-`, ad hoc, which
  Gatekeeper rejects on other Macs) and optionally `MACOS_KEYCHAIN`. **Notarization** additionally
  needs all of `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
- **Windows:** either `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`, or Azure Trusted
  Signing: `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`,
  `AZURE_SIGNING_ACCOUNT` and `AZURE_CERT_PROFILE` (see `packaging/windows/sign.ps1`).

Linux and web artifacts are not code-signed; `SHA256SUMS.txt` covers every file.
