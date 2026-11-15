#!/usr/bin/python3
"""Machine-readable CLI. No argument launches the desktop window."""
import argparse
import json
import sys
import client

DAY_NAMES = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']


def main():
    if not sys.argv[1:]:
        from gui import main as gui_main
        return gui_main()
    p = argparse.ArgumentParser(prog='sleep-scheduler', description='Manage verified sleep/wake schedules. All commands return JSON.')
    p.add_argument('--actor', choices=['user', 'agent'], default='user')
    p.add_argument('--agent-name', default='')
    p.add_argument('--model', default='', help='Self-declared model name, recorded in logs.')
    sub = p.add_subparsers(dest='command', required=True)
    sub.add_parser('status', help='Read live RTC alarm, engine state and schedule.')
    log = sub.add_parser('logs', help='Read structured audit events, newest first.')
    log.add_argument('--limit', type=int, default=50)
    schedule = sub.add_parser('schedule', help='Create or replace a schedule; time values use local 24-hour time.')
    group = schedule.add_mutually_exclusive_group(required=True)
    group.add_argument('--sleep-at', metavar='HH:MM')
    group.add_argument('--now', action='store_true', help='Suspend within about 5 seconds after RTC verification.')
    schedule.add_argument('--wake-at', required=True, metavar='HH:MM')
    schedule.add_argument('--repeat', default='', metavar='daily|mon,tue,...')
    schedule.add_argument('--replace-alarm', action='store_true', help='Explicitly replace an external RTC alarm.')
    sub.add_parser('pause', help='Pause the sleep deadline and clear only this app\'s alarm.')
    resume = sub.add_parser('resume', help='Recalculate and resume a paused schedule.')
    resume.add_argument('--replace-alarm', action='store_true')
    sub.add_parser('cancel', help='Remove the schedule and clear only this app\'s alarm.')
    args = p.parse_args()
    try:
        if args.command == 'status':
            result = client.status()
        elif args.command == 'logs':
            if not 1 <= args.limit <= 2000:
                raise ValueError('Log limit must be 1–2000.')
            result = client.events(args.limit)
        else:
            payload = {}
            if args.command == 'schedule':
                if args.repeat == 'daily':
                    days = list(range(7))
                elif args.repeat:
                    days = [DAY_NAMES.index(s.strip().lower()) for s in args.repeat.split(',')]
                else:
                    days = []
                config = dict(sleep_time=args.sleep_at or '00:00', wake_time=args.wake_at,
                              now=args.now, days=days)
                payload = dict(config=config, replace_alarm=args.replace_alarm)
            elif args.command == 'resume':
                payload = dict(replace_alarm=args.replace_alarm)
            result = client.request(args.command, payload, kind=args.actor,
                                    name=args.agent_name, model=args.model)
        print(json.dumps({'ok': True, 'result': result}, ensure_ascii=False, indent=2))
        return 0
    except Exception as exc:
        print(json.dumps({'ok': False, 'error': str(exc)}, ensure_ascii=False))
        return 1


if __name__ == '__main__':
    sys.exit(main())
