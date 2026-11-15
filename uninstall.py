#!/usr/bin/python3
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys

if os.geteuid()!=0:
    sys.exit('Run: sudo python3 uninstall.py')
engine=Path('/opt/sleep-scheduler/engine.py')
if engine.exists():
    spec=importlib.util.spec_from_file_location('engine',engine)
    e=importlib.util.module_from_spec(spec);spec.loader.exec_module(e)
    with e.locked():
        state=e.read_state()
        if state['status']=='sleeping':
            sys.exit('Suspend is in progress. Uninstall after the computer resumes.')
        e.clear_owned(state)
        e.save(e.default_state())
subprocess.run(['systemctl','disable','--now','sleep-scheduler.service'],check=True)
for name in (
 '/etc/systemd/system/sleep-scheduler.service',
 '/usr/lib/systemd/system-sleep/sleep-scheduler',
 '/usr/local/libexec/sleep-scheduler-control',
 '/usr/local/bin/sleep-scheduler',
 '/usr/share/applications/sleep-scheduler.desktop',
 '/usr/share/icons/hicolor/scalable/apps/sleep-scheduler.svg',
 '/etc/sudoers.d/sleep-scheduler'):
    Path(name).unlink(missing_ok=True)
for name in ('/opt/sleep-scheduler','/var/lib/sleep-scheduler'):
    if Path(name).exists():shutil.rmtree(name)
subprocess.run(['systemctl','daemon-reload'],check=True)
subprocess.run(['/usr/sbin/visudo','-c'],check=True)
if shutil.which('update-desktop-database'):
    subprocess.run(['update-desktop-database','/usr/share/applications'],check=True)
print('Sleep Scheduler removed.')
