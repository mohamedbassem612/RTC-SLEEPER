#!/bin/bash
set -euo pipefail
if [[ $EUID -ne 0 ]]; then echo 'Run: sudo ./scripts/install.sh build/sleep-scheduler' >&2; exit 1; fi
source_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$source_dir/build/sleep-scheduler}"
target_user="${2:-${SUDO_USER:-}}"
[[ -f "$binary" && -n "$target_user" && "$target_user" != root ]] || { echo 'Supply a compiled binary and desktop username.' >&2; exit 1; }
id "$target_user" >/dev/null
[[ ! -e /opt/sleep-scheduler/engine.py ]] || { echo 'Cancel and uninstall the Python engine before installing C++.' >&2; exit 1; }
for tool in /usr/bin/systemctl /usr/sbin/rtcwake /usr/bin/sudo /usr/sbin/visudo; do [[ -x "$tool" ]] || { echo "Missing: $tool" >&2; exit 1; }; done
systemctl stop sleep-scheduler.service 2>/dev/null || true
install -d -m 755 /usr/lib/rtc-sleeper /usr/local/libexec /var/lib/sleep-scheduler
# The root-side parts (daemon, resume hook, sudo helper) use the small QtCore-only engine
# build when it sits next to the GUI binary, so no GUI/plugin code ever runs as root.
engine="$(dirname -- "$binary")/sleep-scheduler-engine"
[[ -f "$engine" ]] || { echo 'Warning: sleep-scheduler-engine not found; using the full binary as root helper.' >&2; engine="$binary"; }
install -m 755 "$binary" /usr/lib/rtc-sleeper/sleep-scheduler
install -m 755 "$engine" /usr/lib/rtc-sleeper/sleep-scheduler-engine
ln -sfn /usr/lib/rtc-sleeper/sleep-scheduler /usr/local/bin/sleep-scheduler
install -m 755 "$engine" /usr/local/libexec/sleep-scheduler-control
install -m 644 "$source_dir/packaging/sleep-scheduler.service" /etc/systemd/system/sleep-scheduler.service
install -m 755 "$source_dir/packaging/sleep-scheduler-hook" /usr/lib/systemd/system-sleep/sleep-scheduler
install -m 644 "$source_dir/packaging/sleep-scheduler.desktop" /usr/share/applications/sleep-scheduler.desktop
install -D -m 644 "$source_dir/icon.svg" /usr/share/icons/hicolor/scalable/apps/sleep-scheduler.svg
install -D -m 644 "$source_dir/AGENTS.md" /usr/share/doc/rtc-sleeper/AGENTS.md
target_uid="$(id -u "$target_user")"
rule_tmp="$(mktemp /etc/sudoers.d/.rtc-sleeper.XXXXXX)"
trap 'rm -f -- "$rule_tmp"' EXIT
printf '# RTC Sleeper validated control helper for one OS user.\n#%s ALL=(root) NOPASSWD: /usr/local/libexec/sleep-scheduler-control\n' "$target_uid" > "$rule_tmp"
chmod 440 "$rule_tmp"
visudo -cf "$rule_tmp"
mv "$rule_tmp" /etc/sudoers.d/sleep-scheduler
visudo -c
systemctl daemon-reload
systemctl enable --now sleep-scheduler.service
command -v update-desktop-database >/dev/null && update-desktop-database /usr/share/applications || true
echo 'RTC Sleeper C++ installed. No schedule has been created.'
