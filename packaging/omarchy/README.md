# Terraphim Editor for Omarchy

This package runs the native GPUI editor as an Omarchy Managed Application on
x86_64 Omarchy. The same Rust editing code backs the macOS and Omarchy builds.

## Build and run

Run this from the editor checkout on x86_64 Omarchy after installing a recent
stable Rust and ODK.

```sh
odk oma dev packaging/omarchy
```

The development command builds the executable, installs a development bundle
for the current user and launches it. Current ODK signs the bundle with a local
development key and accepts `--no-run` to install without launching. For a build
without installation, run `sh packaging/omarchy/build.sh`. To relaunch an
installed bundle, run `odk oma run ai.terraphim.gpui-editor`.

On Arch, the GPUI build uses `base-devel`, `clang`, `pkgconf`, `fontconfig`,
`freetype2`, `wayland`, `libxcb`, `libxkbcommon` and `libxkbcommon-x11`.
Running the editor requires `vulkan-icd-loader` and a Vulkan driver for your
GPU. The current ODK bundle format uses `erofs-utils`. Keep the generated
`Cargo.lock` to reproduce the same dependency resolution later.

Use the in-editor Role chooser to switch between Demo and Plain text. The
menu-bar integration runs on macOS. For a direct launch outside OMA, the built
editor also accepts `--role NAME=PATH` for custom thesauri. Inside the OMA
sandbox, `/open` and `/save` can use the app's own data directory or a granted
XDG Documents directory.

## ODK integration

The package follows the ODK v1 Manifest and `oma.toml` project format. The
`dev.toml` file supports ODK v0.1.0-rc.3. OMA bundles the editor's long-running
x86_64 ELF directly. The app requests GPU access to draw and write access to
Documents to edit existing Markdown files. OMA denies other optional capabilities
by default.

Current ODK UI code uses Rune. Its design guidelines cover a 4 px spacing grid,
0 or 2 px corners, contrast-checked colours, keyboard focus and reduced motion.
The editor uses GPUI and follows the host palette, font family, text scale,
and reduced-motion preference published by `odkd`. OMA exposes the daemon's
read-only socket at `$XDG_RUNTIME_DIR/odk/odkd.sock` without an extra permission.
Changes apply to the open editor. When the daemon is absent, GPUI defaults
apply. Set `ODKD_SOCKET=none` before a direct launch to test those defaults.
Reduced motion keeps GPUI scrollbars visible without a fade. The caret still
uses GPUI's built-in blink.

A distributable release requires setting the `publisher` hostname and
configuring the project's publisher signing and trust on current ODK. ODK
v0.1.0-rc.3 uses the earlier directory bundle format.

- [ODK v0.1.0-rc.3](https://github.com/csfh/odk/releases/tag/v0.1.0-rc.3)
- [ODK upstream revision](https://github.com/csfh/odk/tree/ba50c0db12e87676029b57e21dd61ff4e6bee0fc)
- [OMA Manifest](https://github.com/csfh/odk/blob/ba50c0db12e87676029b57e21dd61ff4e6bee0fc/docs/spec/manifest.md)
- [OMA sandbox baseline](https://github.com/csfh/odk/blob/ba50c0db12e87676029b57e21dd61ff4e6bee0fc/docs/spec/sandbox-baseline.md)
- [ODK host settings protocol](https://github.com/csfh/odk/blob/ba50c0db12e87676029b57e21dd61ff4e6bee0fc/docs/adr/0062-odkd-tells-every-application-what-the-host-says.md)
- [ODK design language](https://github.com/csfh/odk/blob/ba50c0db12e87676029b57e21dd61ff4e6bee0fc/docs/design-language.md)
