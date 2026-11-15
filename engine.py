#!/usr/bin/python3
"""Root-owned scheduling engine. External clients can only submit validated JSON."""
import contextlib
import datetime as dt
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import uuid

ROOT = Path('/var/lib/sleep-scheduler')
RTC = Path('/sys/class/rtc/rtc0/wakealarm')
STATUSES = {'idle', 'scheduled', 'sleeping', 'paused', 'conflict', 'error'}
SYSTEM = {'kind': 'system', 'name': 'Scheduler', 'source': 'system', 'model': ''}


def default_state():
    return dict(version=1, status='idle', config=None, next_sleep=None, next_wake=None,
                expected_alarm=None, actual_alarm=None, actor=None, cycle=None,
                message='', updated=time.time(), last_resume=None, trigger=None, scheduled_at=None)


def read_state():
    path = ROOT / 'state.json'
    return json.loads(path.read_text()) if path.exists() else default_state()


def save(state):
    state['updated'] = time.time()
    temp = ROOT / 'state.tmp'
    temp.write_text(json.dumps(state, ensure_ascii=False, indent=2) + '\n')
    temp.chmod(0o644)
    temp.replace(ROOT / 'state.json')


@contextlib.contextmanager
def locked():
    ROOT.mkdir(mode=0o755, parents=True, exist_ok=True)
    with (ROOT / 'lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def event(action, actor=SYSTEM, result='ok', **details):
    path = ROOT / 'events.jsonl'
    if path.exists() and path.stat().st_size > 2_000_000:
        path.replace(ROOT / 'events.previous.jsonl')
    record = dict(time=dt.datetime.now().astimezone().isoformat(timespec='seconds'),
                  action=action, actor=actor, result=result, **details)
    with path.open('a') as out:
        out.write(json.dumps(record, ensure_ascii=False) + '\n')
    path.chmod(0o644)


def run(args):
    p = subprocess.run(args, capture_output=True, text=True, timeout=35,
                       env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'})
    if p.returncode:
        raise RuntimeError((p.stderr or p.stdout or 'System command failed').strip())
    return p.stdout.strip()


def actual_alarm():
    value = RTC.read_text().strip()
    return int(value) if value else None


def clear_owned(state):
    """Never clear a different alarm placed by another application."""
    actual = actual_alarm()
    if state.get('expected_alarm') is not None and actual == state['expected_alarm']:
        run(['/usr/sbin/rtcwake', '-m', 'disable'])
        if actual_alarm() is not None:
            raise RuntimeError('RTC alarm did not clear.')
    state['expected_alarm'] = None
    state['actual_alarm'] = actual_alarm()


def clock(value):
    if not isinstance(value, str) or not re.fullmatch(r'(?:[01][0-9]|2[0-3]):[0-5][0-9]', value):
        raise ValueError('Time must be HH:MM in 24-hour format.')
    return tuple(map(int, value.split(':')))


def validate_config(c):
    if not isinstance(c, dict) or set(c) != {'sleep_time', 'wake_time', 'now', 'days'}:
        raise ValueError('Invalid schedule fields.')
    clock(c['sleep_time'])
    clock(c['wake_time'])
    if type(c['now']) is not bool:
        raise ValueError('Invalid Sleep Now value.')
    days = c['days']
    if not isinstance(days, list) or len(days) > 7 or any(type(d) is not int or d not in range(7) for d in days) or len(set(days)) != len(days):
        raise ValueError('Invalid repeat weekdays.')
    if c['now'] and days:
        raise ValueError('Sleep Now is one-time. Use a sleep time for repeat schedules.')
    if not c['now'] and c['sleep_time'] == c['wake_time']:
        raise ValueError('Sleep and wake times must be different.')
    return c


def local_stamp(day, hhmm):
    wanted = dt.datetime.combine(day, dt.time(*clock(hhmm)))
    stamp = wanted.timestamp()
    # A local wall time that does not exist during a DST transition is skipped.
    return stamp if dt.datetime.fromtimestamp(stamp) == wanted else None


def next_cycle(c, now=None, after_day=None):
    now = time.time() if now is None else now
    day = dt.datetime.fromtimestamp(now).date()
    if c['now']:
        sleep = now + 5  # let the UI return the confirmed schedule before suspend
    else:
        sleep = None
        for offset in range(16):
            candidate = day + dt.timedelta(days=offset)
            if after_day and candidate <= after_day:
                continue
            if c['days'] and candidate.weekday() not in c['days']:
                continue
            stamp = local_stamp(candidate, c['sleep_time'])
            if stamp is not None and stamp > now + 2:
                sleep = stamp
                break
        if sleep is None:
            raise ValueError('Could not find the next valid sleep date.')
    sleep_day = dt.datetime.fromtimestamp(sleep).date()
    wake = None
    for offset in range(4):
        stamp = local_stamp(sleep_day + dt.timedelta(days=offset), c['wake_time'])
        if stamp is not None and stamp > sleep + 15:
            wake = stamp
            break
    if wake is None:
        raise ValueError('Could not find the next valid wake date.')
    return int(sleep), int(wake)


def validate_actor(a):
    if not isinstance(a, dict) or set(a) != {'kind', 'name', 'model', 'source'}:
        raise ValueError('Invalid actor.')
    if a['kind'] not in ('user', 'agent') or a['source'] not in ('gui', 'cli'):
        raise ValueError('Invalid actor type.')
    for k in ('name', 'model'):
        if not isinstance(a[k], str) or len(a[k]) > 100 or any(ord(ch) < 32 for ch in a[k]):
            raise ValueError('Invalid actor label.')
    # Record the OS-authenticated caller separately from self-declared agent labels.
    return dict(a, uid=int(os.environ.get('SUDO_UID', os.getuid())))


def arm(state, stamp, replace=False):
    current = actual_alarm()
    if current is not None and current != state.get('expected_alarm') and not replace:
        raise ValueError(f'RTC_CONFLICT: external wake alarm is set to {current}. Explicit replacement is required.')
    run(['/usr/sbin/rtcwake', '-m', 'no', '-t', str(stamp)])
    actual = actual_alarm()
    if actual != stamp:
        raise RuntimeError(f'RTC verification failed: expected {stamp}, read {actual}.')
    state['expected_alarm'] = stamp
    state['actual_alarm'] = actual


def conflict(state, message):
    state['status'] = 'conflict'
    state['message'] = message
    state['actual_alarm'] = actual_alarm()
    event('rtc_conflict', result='blocked', expected=state.get('expected_alarm'), actual=state['actual_alarm'])
    save(state)


def finish_cycle(state, reason, now=None):
    now = time.time() if now is None else now
    previous_day = dt.datetime.fromtimestamp(state['next_sleep']).date() if state.get('next_sleep') else None
    c = state.get('config')
    clear_owned(state)
    state.update(status='idle', next_sleep=None, next_wake=None, cycle=None,
                 message='Computer resumed. The previous cycle has been reset.', last_resume=now)
    event('cycle_reset', reason=reason, trigger=state.get('trigger'))
    if c and c['days']:
        sleep, wake = next_cycle(c, now, after_day=previous_day)
        state.update(next_sleep=sleep, next_wake=wake, cycle=uuid.uuid4().hex, scheduled_at=now)
        try:
            arm(state, wake)
            state.update(status='scheduled', message='Next repeat cycle is ready.')
            event('repeat_scheduled', actor=SYSTEM, next_sleep=sleep, next_wake=wake,
                  created_by=state.get('actor'))
        except Exception as exc:
            state.update(status='conflict' if 'RTC_CONFLICT' in str(exc) else 'error', message=str(exc))
            event('repeat_failed', result='blocked', error=str(exc))
    else:
        state['config'] = None
        state['actor'] = None
    save(state)


def control(request):
    if not isinstance(request, dict) or set(request) != {'op', 'payload', 'actor'}:
        raise ValueError('Invalid request.')
    op, p = request['op'], request['payload']
    if op not in ('schedule', 'pause', 'resume', 'cancel') or not isinstance(p, dict):
        raise ValueError('Unsupported operation.')
    actor = validate_actor(request['actor'])
    with locked():
        state = read_state()
        if op == 'schedule':
            if set(p) != {'config', 'replace_alarm'} or type(p['replace_alarm']) is not bool:
                raise ValueError('Invalid schedule request.')
            c = validate_config(p['config'])
            sleep, wake = next_cycle(c)
            # Reject external alarms before making any changes.
            fresh = dict(state)
            arm(fresh, wake, replace=p['replace_alarm'])
            fresh.update(status='scheduled', config=c, next_sleep=sleep, next_wake=wake,
                         actor=actor, cycle=uuid.uuid4().hex, message='', trigger=None, scheduled_at=time.time())
            save(fresh)
            event('schedule_created', actor, next_sleep=sleep, next_wake=wake,
                  repeat_days=c['days'], sleep_now=c['now'], replaced_alarm=p['replace_alarm'])
            state = fresh
        elif op == 'pause':
            if p or state['status'] not in ('scheduled', 'conflict', 'error'):
                raise ValueError('No pending schedule to pause.')
            clear_owned(state)
            state.update(status='paused', message='Both scheduled sleep and its owned wake alarm are paused.')
            save(state)
            event('schedule_paused', actor)
        elif op == 'resume':
            if set(p) != {'replace_alarm'} or type(p['replace_alarm']) is not bool or state['status'] != 'paused' or not state['config']:
                raise ValueError('No paused schedule to resume.')
            sleep, wake = next_cycle(state['config'])
            arm(state, wake, replace=p['replace_alarm'])
            state.update(status='scheduled', next_sleep=sleep, next_wake=wake,
                         cycle=uuid.uuid4().hex, message='', actor=actor, scheduled_at=time.time())
            save(state)
            event('schedule_resumed', actor, next_sleep=sleep, next_wake=wake)
        elif op == 'cancel':
            if p:
                raise ValueError('Cancel takes no payload.')
            if state['status'] == 'sleeping':
                raise ValueError('The suspend request is already in progress.')
            clear_owned(state)
            fresh = default_state()
            fresh['actual_alarm'] = state['actual_alarm']
            fresh['last_resume'] = state.get('last_resume')
            save(fresh)
            event('schedule_removed', actor)
            state = fresh
        return state


def tick():
    with locked():
        state = read_state()
        actual = actual_alarm()
        changed = state.get('actual_alarm') != actual
        state['actual_alarm'] = actual
        if state['status'] == 'scheduled':
            now = time.time()
            if actual != state.get('expected_alarm'):
                conflict(state, 'RTC alarm changed outside this app. Sleep has been blocked.')
                return
            if now >= state['next_sleep']:
                if now - state['next_sleep'] > 60 or now >= state['next_wake'] - 15:
                    finish_cycle(state, 'missed_sleep', now)
                    return
                # Read back a second time immediately before requesting suspend.
                if actual_alarm() != state['next_wake']:
                    conflict(state, 'Wake alarm changed before suspend. Sleep has been blocked.')
                    return
                state.update(status='sleeping', message='Suspend requested.',
                             trigger={'created_by': state['actor'], 'executed_by': SYSTEM, 'at': now})
                save(state)
                event('sleep_triggered', SYSTEM, created_by=state['actor'],
                      next_wake=state['next_wake'], cycle=state['cycle'])
                try:
                    run(['/usr/bin/systemctl', 'suspend'])
                except Exception as exc:
                    clear_owned(state)
                    state.update(status='error', message='Suspend failed: ' + str(exc))
                    save(state)
                    event('sleep_failed', result='error', error=str(exc))
                return
        if changed:
            save(state)


def resumed():
    with locked():
        state = read_state()
        if state['status'] in ('scheduled', 'sleeping'):
            finish_cycle(state, 'system_resumed')
        else:
            state['last_resume'] = time.time()
            save(state)
            event('system_resumed', schedule_status=state['status'])


def daemon():
    with locked():
        state = read_state()
        # A service restart or reboot must never leave a stale sleeping state.
        if state['status'] == 'sleeping':
            finish_cycle(state, 'engine_restarted')
        else:
            save(state)
        event('engine_started')
    while True:
        try:
            tick()
        except Exception as exc:
            print(str(exc), file=sys.stderr, flush=True)
            with locked():
                state = read_state()
                state.update(status='error', message=str(exc))
                save(state)
                event('engine_error', result='error', error=str(exc))
        time.sleep(2)


if __name__ == '__main__':
    try:
        if os.geteuid() != 0:
            raise PermissionError('Administrator privileges required.')
        if sys.argv[1:] == ['daemon']:
            daemon()
        elif sys.argv[1:] == ['resumed']:
            resumed()
        elif len(sys.argv) == 3 and sys.argv[1] == 'control' and len(sys.argv[2]) <= 8192:
            result = control(json.loads(sys.argv[2]))
            print(json.dumps({'ok': True, 'state': result}))
        else:
            raise ValueError('Unsupported invocation.')
    except Exception as exc:
        try:
            if len(sys.argv) == 3 and sys.argv[1] == 'control':
                request = json.loads(sys.argv[2])
                actor = validate_actor(request.get('actor'))
                with locked():
                    event('request_failed', actor=actor, result='error', operation=request.get('op'), error=str(exc))
        except Exception:
            pass
        print(json.dumps({'ok': False, 'error': str(exc)}))
        sys.exit(1)
