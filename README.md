# RTC Sleeper · Sleep Scheduler

[![Build and test](https://github.com/mohamedbassem612/RTC-SLEEPER/actions/workflows/build.yml/badge.svg)](https://github.com/mohamedbassem612/RTC-SLEEPER/actions/workflows/build.yml)

A native Linux desktop application that suspends the computer on schedule and wakes it through the hardware RTC alarm. It has an AMOLED black interface, a fixed 1000×720 window, animated screen transitions, English and Arabic, one-time and weekday-repeat schedules, a live countdown, RTC conflict protection, and structured user/AI-agent logs.

The project deliberately uses **system suspend**, rather than shutting down and starting from ACPI S5. On the development laptop, RTC wake from suspend restored Wi-Fi and Bluetooth correctly, while RTC startup after a full shutdown coincided with Intel AX201 initialization failures.

## How it works

```text
GUI or JSON CLI
      │
      ▼
validated root helper ───► protected schedule state + audit log
      │
      ▼
system scheduling engine ─► verifies RTC ─► systemd suspend
                                    │
                                    ▼
                              hardware wakes PC
                                    │
                                    ▼
                         system-sleep post hook resets cycle
```

The GUI and CLI are clients of the same engine:

1. The client validates the requested sleep time, wake time and repeat weekdays.
2. The privileged engine writes the wake time with `rtcwake -m no`, then immediately reads `/sys/class/rtc/rtc0/wakealarm` back. The schedule is not marked active unless the values match.
3. The engine keeps watching the real RTC value. If another program changes or removes it, the app enters **Schedule conflict** and blocks suspend.
4. At the sleep deadline, the engine reads RTC one final time and requests ordinary `systemctl suspend`.
5. When Linux resumes, a system-sleep hook resets the completed cycle. A one-time schedule returns to idle; a repeating schedule advances to a later selected sleep-start day.

The RTC contains only one wake timestamp. The sleep deadline, repeat policy, creator and audit data live in `/var/lib/sleep-scheduler`. Because RTC does not expose ownership, the app never silently overwrites an external alarm. Replacing it requires an explicit user or CLI option.

## Features

- AMOLED black UI with white accents and a fixed window size.
- **Sleep now** with a separately chosen wake time; the disabled Sleep at row is fully greyed out.
- One-time schedules or selected weekday repetition.
- Live **Will sleep in** countdown and computed sleep duration.
- English by default, with Arabic available in Settings.
- Pause, resume, edit and remove controls.
- Settings page showing engine health, the live RTC alarm and audit logs.
- JSON CLI designed for automation and AI agents.
- Audit attribution for GUI/CLI, user/agent, declared agent/model name, authenticated OS UID, system trigger and resume/reset events.
- No shutdown scheduler and no idle-shutdown timer.

## Install

On Ubuntu with systemd and GTK 3:

```bash
sudo python3 install.py
```

Open **Sleep Scheduler** from the application menu or run `sleep-scheduler`. Installation leaves the schedule inactive. The installer adds only one narrow passwordless rule for the installing OS user: `/usr/local/libexec/sleep-scheduler-control`. That helper accepts validated schedule operations; it is not a general root shell.

Requirements: Ubuntu 24.04 or a compatible systemd-based Linux desktop, Python 3, GTK 3, PolicyKit/sudo, `rtcwake`, and hardware/firmware that supports RTC wake from suspend. The installer adds the GTK Python bridge if it is missing.

## Use

1. Choose **Sleep at** and **Wake up at**. The summary shows the actual dates and duration.
2. Optionally enable **Repeat** and select sleep-start weekdays.
3. Click **Schedule sleep**. The engine programs RTC and verifies the alarm before marking the schedule active.
4. The dashboard shows the sleep countdown, sleep duration, actual wake time and creator. **Pause** pauses sleep and clears only the app's own alarm. **Remove** deletes the schedule.
5. **Sleep now** ignores the scheduled sleep time; submitting verifies the wake alarm and suspends in about five seconds. It is a one-time action.

Closing the window does not stop a schedule. The root engine handles deadlines even when the UI is closed. A schedule missed by more than one minute is reset, never caught up by immediately sleeping after a reboot. After any system resume, the current cycle is reset. Repeating schedules advance to a later selected sleep-start day.

Settings contains the language selector, **Logs**, engine state and the live RTC alarm. English is the default. Language preferences are per desktop user.

## RTC and external changes

RTC stores only one hardware wake alarm. Sleep time, repeat settings and actor identity live in the app's protected state. The engine polls RTC every two seconds, the GUI also reads it live, and the engine rechecks immediately before suspend. A changed or unreadable alarm blocks scheduled sleep and is logged. Replacing another application's alarm requires an explicit choice. Cancellation never clears a different alarm.

A successful RTC read-back verifies the configured alarm, not the physical hardware's ability to wake. Another privileged application can race to change a single shared alarm; cross-application ownership cannot be proven from the timestamp alone. Use this application as the single scheduler where possible.

On this computer, RTC wake after ordinary suspend worked in prior tests, while RTC startup after a full shutdown coincided with Intel Wi-Fi/Bluetooth initialization failures. This app deliberately uses the tested suspend path.

## CLI and agent instructions

See [AGENTS.md](AGENTS.md), also installed at `/opt/sleep-scheduler/AGENTS.md`.

```bash
sleep-scheduler status
sleep-scheduler --actor agent --agent-name Codex schedule --sleep-at 23:00 --wake-at 07:00
sleep-scheduler --actor agent --agent-name Codex cancel
sleep-scheduler logs --limit 20
```

All CLI operations emit JSON. The shared engine records the actor's declared user/agent kind, name, model, GUI/CLI source and the authenticated OS UID. Model identity is not independently verified. Logs distinguish schedule creation from the system's eventual execution. Logs rotate at about 2 MB, retaining one previous file.

Examples:

```bash
# Read live state and the RTC alarm
sleep-scheduler status

# One-time overnight schedule
sleep-scheduler schedule --sleep-at 23:00 --wake-at 07:00

# Weekday repeat schedule attributed to an AI agent
sleep-scheduler --actor agent --agent-name Codex --model MODEL_NAME \
  schedule --sleep-at 23:00 --wake-at 07:00 --repeat mon,tue,wed,thu,fri

# Sleep in about five seconds after wake-alarm verification
sleep-scheduler --actor agent --agent-name Codex schedule --now --wake-at 08:10

# Pause/resume/cancel and inspect audit events
sleep-scheduler pause
sleep-scheduler resume
sleep-scheduler cancel
sleep-scheduler logs --limit 30
```

All time inputs are local 24-hour `HH:MM`. `status` exposes Unix timestamps so automation can compare them without parsing localized display strings. See [AGENTS.md](AGENTS.md) for conflict handling and safe automation rules.

## Tests (no hardware writes or suspend)

```bash
python3 -m unittest discover -s tests -v
```

The tests replace RTC and system commands with local fakes. They cover overnight/DST calculations, repeat dates, validation, RTC overrides, ownership-preserving cancellation, pause/resume, early manual wake reset, missed deadlines, suspend failure and audit attribution.

## Automated builds

The GitHub Actions workflow runs on every push and pull request. It:

1. installs GTK and a virtual X display;
2. compiles every Python module;
3. runs engine and GUI tests;
4. validates the installer desktop-entry source;
5. builds `rtc-sleeper-linux.zip`, `rtc-sleeper-linux.tar.gz`, and `SHA256SUMS`;
6. uploads those files as a 30-day workflow artifact.

Pushing a tag such as `v1.0.0` also creates a GitHub Release containing the archives and checksums.

## Project layout

| Path | Purpose |
|---|---|
| `gui.py` / `style.css` | Fixed-size GTK interface, localization and AMOLED theme |
| `client.py` | Shared unprivileged GUI/CLI client |
| `cli.py` | JSON command-line interface |
| `engine.py` | Validated root engine, RTC checks, recurrence and audit logging |
| `install.py` / `uninstall.py` | System integration and safe removal |
| `AGENTS.md` | Instructions for AI/automation clients |
| `tests/` | Fake-RTC unit and interface tests |
| `.github/workflows/build.yml` | CI tests, package builds and tagged releases |

## Uninstall

```bash
sudo python3 uninstall.py
```

The uninstaller refuses to remove an in-progress suspend cycle. It cancels any pending schedule, preserves unrelated RTC alarms and removes the engine, menu entry, hook and permission rule. Language preferences in your user account remain.
