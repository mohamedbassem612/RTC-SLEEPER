# RTC Sleeper · C++ / Qt 6

[![Build and test](https://github.com/mohamedbassem612/RTC-SLEEPER/actions/workflows/build.yml/badge.svg)](https://github.com/mohamedbassem612/RTC-SLEEPER/actions/workflows/build.yml)

A Linux desktop application that schedules **suspend and RTC wake**, with an AMOLED black and white interface. The GUI, CLI, privileged scheduling engine and tests are written in C++17 using Qt 6. No Python runtime is needed.

The original working Python/GTK release is preserved on the [`python-version`](https://github.com/mohamedbassem612/RTC-SLEEPER/tree/python-version) branch, including its README and successful build history.

An Android remote-control app written in Rust is under `remote/android` and `android/`. It embeds Tailscale, pairs with the laptop using a QR code or a 24-word phrase, and uses the existing C++ scheduler through an unprivileged companion. Setup, builds, signing and safe mock testing: [Android guide](docs/ANDROID.md).

## Features

- Fixed 1000 × 720 desktop window, black backgrounds and white accents.
- English by default; Arabic and activity logs in Settings.
- Horizontal animated toggle switches for Sleep Now and repeat weekdays, with keyboard support and mirrored RTL layout.
- Sleep Now or a future sleep time, plus a wake time and optional weekday repeat.
- Entire Sleep at row disabled and dimmed when Sleep Now is enabled.
- Separate hour/minute controls: GUI hours are restricted to **1–12 with AM/PM**, minutes to 0–59. The engine independently validates 24-hour CLI input.
- Visible increase/decrease arrows above each numeric field, with animated digits. Holding an arrow repeats after 400 ms, accelerates gradually and caps at one step every 140 ms (about 7 steps per second). Releasing or disabling the control stops repetition.
- Animated transition to the active schedule screen, with a live **Will sleep in:** countdown. The countdown is the time until scheduled suspend; it is separate from sleep duration.
- Edit Schedule, **Cancel Editing**, Pause/Resume and Remove.
- Remove preserves the last accepted time selections, including after closing and reopening the app. Cancel Editing discards the draft without changing the active schedule.
- JSON CLI shared with the GUI and structured audit logs identifying user/agent, model label and OS UID.

## How it works

1. The unprivileged GUI or CLI submits a validated JSON request through a narrowly scoped, root-owned helper. The installer authorizes only that helper for the desktop user; ordinary app use does not ask for a password.
2. The engine calculates the next valid local sleep/wake dates. Weekly repeat applies to the day **sleep starts**, and overnight wake times move to the following day.
3. `rtcwake -m no -t EPOCH` programs only the hardware wake alarm. The engine reads `/sys/class/rtc/rtc0/wakealarm` back and accepts the schedule only if it matches.
4. A systemd service checks the live alarm every second and checks it again immediately before requesting ordinary `systemctl --no-block suspend`.
5. If another program changes the RTC alarm, scheduled sleep is blocked and the conflict appears in the UI and logs. An external alarm is never silently replaced. Pause/Remove clear only an alarm that still matches this app's own alarm.
6. The systemd post-resume hook resets the completed cycle, including after an early manual wake. One-time schedules return to idle; repeat schedules advance to a later selected day instead of suspending again for the same cycle.

RTC contains **only the wake alarm**. Sleep deadlines, repeat settings and creator identity are stored separately in root-owned state. Status always combines that state with a fresh hardware alarm read. A missed sleep deadline more than 60 seconds old is reset rather than caught up by putting the machine to sleep on the next boot.

This app uses suspend, not shutdown, hibernation or idle-shutdown. Shutdown followed by hardware wake previously caused Wi-Fi initialization problems on the target laptop; ordinary suspend/resume worked in the user's test. Wake support still depends on the firmware, kernel, selected sleep mode and power conditions. Concurrent unrelated writers to the RTC alarm cannot be made fully atomic by this application.

## Build and test

Ubuntu 24.04 / Debian-like systems:

```bash
sudo apt-get install build-essential cmake ninja-build qt6-base-dev
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

Tests use a fake clock, fake RTC and fake suspend implementation. They do not suspend the test machine or alter its hardware alarm. UI tests run with Qt's offscreen platform.

## Install

Cancel any active Python schedule and uninstall the Python release first. Then, from the desktop user's account:

```bash
sudo apt-get install sudo util-linux systemd libqt6widgets6t64
sudo bash scripts/install.sh build/sleep-scheduler
sleep-scheduler
```

The app appears as **RTC Sleeper** in the applications menu. The installer starts an idle engine and does not create a schedule. A downloaded binary archive includes the binary under `build/sleep-scheduler` and the same installer. Binaries built by CI target Ubuntu 24.04 x86-64 and need compatible Qt 6 runtime libraries; the archive is not a static executable or an AppImage.

For a different target user: `sudo bash scripts/install.sh build/sleep-scheduler USERNAME`.

Installed files:

| Purpose | Location |
| --- | --- |
| C++ executable | `/usr/lib/rtc-sleeper/sleep-scheduler` |
| CLI / GUI launcher | `/usr/local/bin/sleep-scheduler` |
| Validated privileged helper | `/usr/local/libexec/sleep-scheduler-control` |
| State and rotated JSON logs | `/var/lib/sleep-scheduler/` |
| Service | `sleep-scheduler.service` |
| Resume hook | `/usr/lib/systemd/system-sleep/sleep-scheduler` |
| User preferences | Qt AppConfigLocation: `RTCSleeper/RTC Sleeper/preferences.ini` |

## CLI / AI agent use

No arguments opens the GUI. Commands work without a display server and print JSON; success exits 0 with `ok:true`, errors exit nonzero with `ok:false`. CLI time values are **24-hour HH:MM**. Agent/model names are self-declared metadata; the helper separately records the OS UID supplied by sudo.

```bash
sleep-scheduler --help
sleep-scheduler status
sleep-scheduler logs --limit 30

sleep-scheduler --actor agent --agent-name Codex --model MODEL_NAME \
  schedule --sleep-at 23:00 --wake-at 07:00 --repeat mon,tue,wed,thu,fri

sleep-scheduler pause
sleep-scheduler resume
sleep-scheduler cancel
```

`--repeat daily` selects every day. Sleep Now is one-time:

```bash
# Actually suspends the machine in about 5 seconds. Only on explicit request.
sleep-scheduler schedule --now --wake-at 08:10
```

An RTC conflict requires explicit permission to replace the external alarm:

```bash
sleep-scheduler schedule --sleep-at 23:00 --wake-at 07:00 --replace-alarm
```

See [AGENTS.md](AGENTS.md) for operating instructions. Read-only diagnostics:

```bash
systemctl status sleep-scheduler.service --no-pager
journalctl -u sleep-scheduler.service -n 30 --no-pager
```

## GitHub builds

GitHub Actions builds the C++ executable on Ubuntu 24.04, runs engine and GUI regression tests, checks the desktop entry and service unit, and uploads a Linux binary archive, source archive and SHA-256 checksums. Find downloads under **Actions → successful run → Artifacts**. Pushing a `v*` tag additionally publishes the archives as a GitHub Release.

CI verifies application behavior with fake hardware. A successful build does not certify wake support on every laptop.

## Source layout

```text
src/engine.*       Validation, date calculation, RTC ownership, audit and suspend
src/client.*       Shared unprivileged CLI/GUI interface
src/window.*       Qt desktop UI and saved preferences
src/main.cpp       Headless CLI, root helper, daemon, resume hook and GUI entry
tests/            C++ engine and interface regression tests
scripts/          Install and uninstall scripts
packaging/        systemd service, resume hook and desktop entry
```

## Uninstall

```bash
sudo bash scripts/uninstall.sh
```

The uninstaller cancels the app's schedule, stops the service and removes installed application files. Audit logs are retained in `/var/lib/sleep-scheduler/`. The Python migration backup on the original laptop is retained under `/var/backups/rtc-sleeper-python/`.

## License

MIT. See [LICENSE](LICENSE). Qt is a separate dependency subject to its own license.
