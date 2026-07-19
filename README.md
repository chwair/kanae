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

| Variant | What it is | Windows | macOS | Linux |
|---|---|---|---|---|
| **hybrid** | One app, both frontends — opens the TUI when run from a terminal, the GUI otherwise | installer | `.app` zip | AppImage |
| **gui** | GUI only | installer | `.app` zip | AppImage |
| **tui** | TUI only, single-file executable, no Qt needed | `.exe` | binary | binary |

The hybrid build decides at launch: started from a terminal → TUI; double-clicked / launched from the desktop → GUI. Force a frontend with `--gui`/`-g` or `--tui`/`-t`.

> macOS builds are ad-hoc signed; on first launch you may need to right-click → Open (or `xattr -d com.apple.quarantine Kanae.app`).

## Build from source
- Install Rust and Qt 6 (Qt only needed for the GUI; point `QMAKE` at your Qt install if it isn't on PATH)
```bash
git clone https://github.com/chwair/kanae
cd kanae

cargo install --path .                                   # hybrid (GUI + TUI)
cargo install --path . --no-default-features --features tui   # TUI only, no Qt required
```

### Packaging locally
- **Windows**: `.\scripts\package.ps1 -Variant hybrid -MakeInstaller` — builds, runs `windeployqt6`, and produces the NSIS installer.
- **macOS**: `cargo bundle-mac` then `macdeployqt` on the resulting `.app`.
- **CI**: pushing a `v*` tag builds installers, `.app` zips, AppImages, and TUI binaries for all three platforms and attaches them to a GitHub release (see `.github/workflows/release.yml`).

## License
MIT
