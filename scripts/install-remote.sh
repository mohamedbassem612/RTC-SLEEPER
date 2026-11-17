#!/bin/bash
set -euo pipefail
[[ $EUID -ne 0 ]] || { echo 'Run this installer as the desktop user, without sudo.' >&2; exit 1; }
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$repo_dir/remote/target/release/rtc-sleeper-remote}"
library="${2:-$repo_dir/dist/remote/libtailscale.so}"
[[ -f "$binary" && -f "$library" ]] || { echo 'Build the Rust desktop bridge and libtailscale first.' >&2; exit 1; }
target_dir="$HOME/.local/lib/rtc-sleeper-remote"
systemctl --user stop rtc-sleeper-remote.service 2>/dev/null || true
install -d -m 755 "$target_dir" "$HOME/.local/bin" "$HOME/.config/systemd/user"
install -m 755 "$binary" "$target_dir/rtc-sleeper-remote"
install -m 755 "$library" "$target_dir/libtailscale.so"
ln -sfn "$target_dir/rtc-sleeper-remote" "$HOME/.local/bin/rtc-sleeper-remote"
install -m 644 "$repo_dir/packaging/rtc-sleeper-remote.service" "$HOME/.config/systemd/user/rtc-sleeper-remote.service"
systemctl --user daemon-reload
systemctl --user enable --now rtc-sleeper-remote.service
echo 'Remote bridge installed. Sign in through Settings → Remote Control. No power command was sent.'
