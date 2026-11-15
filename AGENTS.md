# Sleep Scheduler — instructions for AI agents

Use the installed `sleep-scheduler` CLI to control the same engine as the GUI. Do not edit state files, systemd units or RTC sysfs directly. Do not create another shutdown/idle-shutdown service. This application only uses ordinary systemd suspend; shutdown plus RTC wake previously caused Wi-Fi/Bluetooth initialization failures on this computer.

## Discover state before changing it

```bash
sleep-scheduler status
sleep-scheduler logs --limit 30
sleep-scheduler --help
sleep-scheduler schedule --help
```

All commands except opening the GUI print JSON. Success exits 0 with `ok:true`; validation or permission failures exit nonzero with `ok:false`. Times use the device's local timezone and 24-hour HH:MM input. Status timestamps are Unix epoch seconds. `actual_alarm` is read live from RTC; it contains only the wake time, not the sleep deadline or repeat policy.

## Identify yourself

Pass actor information before the subcommand. Labels are self-declared; the engine separately records the authenticated OS UID.

```bash
sleep-scheduler --actor agent --agent-name Codex --model MODEL_NAME schedule --sleep-at 23:00 --wake-at 07:00
sleep-scheduler --actor agent --agent-name Codex pause
sleep-scheduler --actor agent --agent-name Codex resume
sleep-scheduler --actor agent --agent-name Codex cancel
```

A normal schedule chooses the next future sleep time and the first valid wake time after it. Overnight schedules show tomorrow's date. A passed sleep deadline is never caught up by immediately suspending on the next boot. Weekly recurrence refers to the day sleep STARTS:

```bash
sleep-scheduler --actor agent --agent-name Codex schedule --sleep-at 23:00 --wake-at 07:00 --repeat mon,tue,wed,thu,fri
sleep-scheduler --actor agent --agent-name Codex schedule --sleep-at 23:00 --wake-at 07:00 --repeat daily
```

## Sleep immediately, only when explicitly requested

```bash
sleep-scheduler --actor agent --agent-name Codex schedule --now --wake-at 08:10
```

This verifies the wake alarm, then requests suspend in about five seconds. It can interrupt the remote connection. Never use this as a smoke test. This mode is one-time and cannot repeat.

## RTC conflicts

If `RTC_CONFLICT` is returned or status becomes `conflict`, scheduled sleep is blocked. Explain the actual vs expected alarm. Do not silently replace an alarm belonging to another program. Only with an explicit user request to replace it:

```bash
sleep-scheduler --actor agent --agent-name Codex schedule --sleep-at 23:00 --wake-at 07:00 --replace-alarm
```

`pause` removes only this schedule's matching RTC alarm and pauses sleep. `resume` recalculates the next future cycle; it never executes a missed deadline. `cancel` clears the schedule, but never clears an alarm that has been changed externally.

## Reset after resume

The root system-sleep post hook resets the current cycle on every system resume, including manual early wake. One-time schedules return to idle. Repeat schedules move to a later selected day, never resuspending for the same cycle. The logs distinguish the request creator (`created_by`) from the system executing the sleep trigger.

## Installation and verification

Installed engine: `/opt/sleep-scheduler/engine.py`. State and rotated logs: `/var/lib/sleep-scheduler/`. System service: `sleep-scheduler.service`. The passwordless privilege applies only to `/usr/local/libexec/sleep-scheduler-control`, which accepts validated scheduling operations; it does not grant general sudo access.

Read-only checks:

```bash
systemctl status sleep-scheduler.service --no-pager
journalctl -u sleep-scheduler.service -n 30 --no-pager
```

Hardware wake support can still fail despite a successful alarm read-back. Concurrent unrelated programs writing the hardware alarm cannot be made atomic by this application. Preserve remote access: do not suspend/reboot or test physical wake unless explicitly authorized.
