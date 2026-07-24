> [!IMPORTANT]
> Kanae is in beta!! While most features work, there's still a bit of polish needed to get it in a workable, reliable state. Thanks!

<div align=center>
<h1>Kanae</h1>
<p>A music and CD player with a TUI and GUI, written in Rust.<p>
<div><img width="40%" height="auto" alt="Player view (TUI)" src="https://github.com/user-attachments/assets/7c59f080-f8f9-4cbb-97f9-1f885e4d5aed" />
<img width="40%" height="auto" alt="Player view (GUI)" src="https://github.com/user-attachments/assets/c1962b96-c281-42b5-b8b0-3f0814449fe4" /></div>
</div>

## Features
- Clean UI
- Automatic CD metadata fetching from MusicBrainz
- Synced lyrics from LRCLIB
- OS Media controls and metadata
- Discord RPC support
- Windows, Mac and Linux support

## Install
Grab a build from the [releases page](https://github.com/chwair/kanae/releases). Each release ships three variants per platform:

| Variant | Windows | macOS | Linux |
|---|---|---|---|
| **hybrid** | installer | `.app` zip | AppImage |
| **gui** | installer | `.app` zip | AppImage |
| **tui** | `.exe` | binary | binary |

The hybrid build will try to detect start intent but you can always force a frontend with `--gui`/`-g` or `--tui`/`-t`.

## Build from source
- Install Rust and Qt 6 (Qt only needed for the GUI; point `QMAKE` at your Qt install if it isn't on PATH)
```bash
git clone https://github.com/chwair/kanae
cd kanae

cargo install --path .                                   # hybrid (GUI + TUI)
cargo install --path . --no-default-features --features tui   # TUI only, no Qt required
cargo install --path . --no-default-features --features gui   # GUI only
```

### Packaging locally
- **Windows**: `.\scripts\package.ps1 -Variant hybrid -MakeInstaller`
- **macOS**: `cargo bundle-mac` then `macdeployqt` on the resulting `.app`.
- **CI**: pushing a `v*` tag builds installers, `.app` zips, AppImages, and TUI binaries for all three platforms and attaches them to a GitHub release (see `.github/workflows/release.yml`).

## License
MIT
