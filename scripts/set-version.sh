#!/usr/bin/env bash
# sets the package version in Cargo.toml and Cargo.lock from a release name.
# "v1.2.3" or "1.2.3" becomes 1.2.3; anything else (e.g. "dev") becomes the
# current version with a "-dev" suffix. prints the version it wrote.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
name="${1:?usage: set-version.sh <release name>}"

current="$(awk -F'"' '/^\[package\]/{p=1} p && /^version *=/{print $2; exit}' "$root/Cargo.toml")"
if [[ "$name" =~ ^v?([0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?)$ ]]; then
  version="${BASH_REMATCH[1]}"
else
  version="${current%%-*}-dev"
fi

# only the [package] version line, not dependency tables
awk -v v="$version" '
  /^\[/ { inpkg = ($0 == "[package]") }
  inpkg && !done && /^version *=/ { print "version = \"" v "\""; done = 1; next }
  { print }
' "$root/Cargo.toml" > "$root/Cargo.toml.tmp" && mv "$root/Cargo.toml.tmp" "$root/Cargo.toml"

# the lock entry right after `name = "kanae"`
if [[ -f "$root/Cargo.lock" ]]; then
  awk -v v="$version" '
    prev == "name = \"kanae\"" && /^version = / { print "version = \"" v "\""; prev = ""; next }
    { print; prev = $0 }
  ' "$root/Cargo.lock" > "$root/Cargo.lock.tmp" && mv "$root/Cargo.lock.tmp" "$root/Cargo.lock"
fi

echo "$version"
