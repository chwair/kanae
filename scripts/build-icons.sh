#!/usr/bin/env bash
# regenerates every packaged icon from assets/Kanae.icon (icon composer source).
# needs macos with xcode 26+ for actool; the outputs are committed so the
# windows/linux release jobs never have to run this.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
src="$root/assets/Kanae.icon"
out="$root/assets"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# macos: Assets.car carries the liquid glass icon for macos 26+, Kanae.icns is
# the flattened fallback for older systems
mkdir -p "$out/macos"
xcrun actool "$src" --compile "$tmp" --app-icon Kanae \
    --enable-on-demand-resources NO --development-region en \
    --target-device mac --platform macosx --minimum-deployment-target 12.0 \
    --output-partial-info-plist "$tmp/partial.plist" >/dev/null
cp "$tmp/Assets.car" "$tmp/Kanae.icns" "$out/macos/"

# windows/linux get the bare transparent logo, not the macos tile
logo="$src/Assets/kanaelogo transparent.png"
sips -z 256 256 "$logo" --out "$out/kanae.png" >/dev/null
for s in 16 24 32 48 64 128 256; do
    sips -z "$s" "$s" "$logo" --out "$tmp/$s.png" >/dev/null
done

# pack png frames into an .ico (png-in-ico is supported since windows vista)
python3 - "$tmp" "$out/kanae.ico" <<'PY'
import struct, sys
tmp, dest = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
frames = [open(f"{tmp}/{s}.png", "rb").read() for s in sizes]
offset = 6 + 16 * len(frames)
header = struct.pack("<HHH", 0, 1, len(frames))
entries = b""
for s, data in zip(sizes, frames):
    dim = 0 if s == 256 else s  # 0 means 256 in the ico directory
    entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
with open(dest, "wb") as f:
    f.write(header + entries + b"".join(frames))
PY

echo "icons written to $out"
