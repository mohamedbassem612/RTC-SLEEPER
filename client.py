"""Shared unprivileged interface for the GUI and CLI."""
import json
from pathlib import Path
import subprocess

ROOT = Path('/var/lib/sleep-scheduler')
HELPER = '/usr/local/libexec/sleep-scheduler-control'


def status():
    state_path = ROOT / 'state.json'
    state = json.loads(state_path.read_text()) if state_path.exists() else dict(
        status='idle', config=None, next_sleep=None, next_wake=None, expected_alarm=None,
        actual_alarm=None, actor=None, message='Install the engine to enable scheduling.')
    try:
        value = Path('/sys/class/rtc/rtc0/wakealarm').read_text().strip()
        state['actual_alarm'] = int(value) if value else None
        state['rtc_readable'] = True
    except (OSError, ValueError):
        state['rtc_readable'] = False
    engine = subprocess.run(['/usr/bin/systemctl', 'is-active', 'sleep-scheduler.service'],
                            capture_output=True, text=True, timeout=4)
    state['engine_running'] = engine.stdout.strip() == 'active'
    if state['status'] == 'scheduled' and not state['engine_running']:
        state.update(status='error', message='Scheduling engine is not running. Sleep will not be triggered.')
    elif state['status'] == 'scheduled' and (not state['rtc_readable'] or state['actual_alarm'] != state['expected_alarm']):
        state.update(status='conflict', message='RTC wake alarm differs from this schedule. Sleep is blocked.')
    return state


def events(limit=100):
    result = []
    for name in ('events.previous.jsonl', 'events.jsonl'):
        path = ROOT / name
        if path.exists():
            for line in path.read_text().splitlines():
                try:
                    result.append(json.loads(line))
                except ValueError:
                    continue
    return result[-limit:][::-1]


def request(op, payload=None, *, kind='user', name='', model='', source='cli'):
    actor = dict(kind=kind, name=name or ('AI Agent' if kind == 'agent' else 'User'), model=model, source=source)
    envelope = dict(op=op, payload=payload or {}, actor=actor)
    p = subprocess.run(['/usr/bin/sudo', '-n', HELPER, json.dumps(envelope)],
                       capture_output=True, text=True, timeout=45)
    try:
        result = json.loads(p.stdout)
    except ValueError:
        raise RuntimeError(p.stderr.strip() or 'The scheduling engine is not installed or permission is unavailable.')
    if p.returncode or not result.get('ok'):
        raise RuntimeError(result.get('error', 'Request failed.'))
    return result['state']
