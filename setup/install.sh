#!/usr/bin/env bash
# Build tk (release) and install it to ~/.local/bin/tk atomically:
# copy to a temp file in the same dir, check it runs, then rename over tk.
# Running tk processes keep the old file; new ones get the new one.
# Usage: setup/install.sh        (TK_INSTALL_DIR overrides ~/.local/bin)
set -euo pipefail

cd "$(dirname "$0")/.."
dest_dir="${TK_INSTALL_DIR:-$HOME/.local/bin}"
target_dir="${CARGO_TARGET_DIR:-target}"

cargo build --release
mkdir -p "$dest_dir"
tmp="$(mktemp "$dest_dir/.tk.install.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
cp "$target_dir/release/tk" "$tmp"
chmod 755 "$tmp"
"$tmp" --version >/dev/null
mv -f "$tmp" "$dest_dir/tk"
trap - EXIT

echo "installed $dest_dir/tk: $("$dest_dir/tk" --version)"
