> [!IMPORTANT]
> Kanae is in beta!! While most features work, there's still a bit of polish needed to get it in a workable, reliable state. Thanks!

<div align=center>
<h1>Kanae</h1>
<p>A music and player with a TUI and GUI, written in Rust.<p>
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

## Get started
The simplest way to install Kanae is to:
- Install Rust if you haven't already
- Install QT6
- Clone this repo:
```bash
git clone https://github.com/chwair/kanae
cd kanae
```
- Install thru cargo:
```bash
cargo install --path .
```
- Run it:
```bash
# TUI
kanae

# GUI
kanae -g
```
I'll document more specific ways on installing Kanae later.

## License
MIT
