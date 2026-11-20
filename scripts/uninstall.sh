#!/bin/bash
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo 'Run with sudo.' >&2; exit 1; }
# Use the installed engine to clear only its own alarm before removing files.
desktop_uid="${SUDO_UID:-}"
[[ -n "$desktop_uid" && "$desktop_uid" -gt 0 ]] || { echo 'Run from your desktop account with sudo.' >&2; exit 1; }
SUDO_UID="$desktop_uid" /usr/local/libexec/sleep-scheduler-control '{"op":"cancel","payload":{},"actor":{"kind":"user","name":"Uninstaller","model":"","source":"cli"}}'
systemctl disable --now sleep-scheduler.service
rm -f /etc/systemd/system/sleep-scheduler.service /usr/lib/systemd/system-sleep/sleep-scheduler /usr/local/libexec/sleep-scheduler-control /usr/local/bin/sleep-scheduler /usr/share/applications/sleep-scheduler.desktop /usr/share/icons/hicolor/scalable/apps/sleep-scheduler.svg /etc/sudoers.d/sleep-scheduler /usr/lib/rtc-sleeper/sleep-scheduler /usr/lib/rtc-sleeper/sleep-scheduler-engine /usr/share/doc/rtc-sleeper/AGENTS.md
rmdir /usr/lib/rtc-sleeper /usr/share/doc/rtc-sleeper 2>/dev/null || true
systemctl daemon-reload
command -v update-desktop-database >/dev/null && update-desktop-database /usr/share/applications || true
echo 'Application removed. Audit logs retained in /var/lib/sleep-scheduler.'
