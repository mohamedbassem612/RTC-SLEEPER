#!/usr/bin/python3
"""Install a root-owned engine and a single narrowly scoped passwordless helper."""
import os
from pathlib import Path
import shutil
import subprocess
import sys


def command(*args):
    subprocess.run(args,check=True)


def write(path,content,mode=0o644):
    p=Path(path)
    p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(content)
    p.chmod(mode)
    os.chown(p,0,0)


def main():
    if os.geteuid()!=0:
        sys.exit('Run: sudo python3 install.py')
    uid=int(os.environ.get('SUDO_UID',os.environ.get('PKEXEC_UID','0')))
    if uid<=0:
        sys.exit('Run the installer using sudo from your desktop account.')
    source=Path(__file__).resolve().parent
    # This small GTK Cairo bridge is needed to render the vector icons and ring.
    try:
        subprocess.run(['/usr/bin/python3','-c',"import gi; gi.require_foreign('cairo'); gi.require_version('Gtk','3.0')"],check=True,capture_output=True)
    except subprocess.CalledProcessError:
        command('/usr/bin/apt-get','install','-y','python3-gi','python3-gi-cairo','gir1.2-gtk-3.0')
    for tool in ('/usr/sbin/rtcwake','/usr/bin/systemctl','/usr/bin/sudo','/usr/sbin/visudo'):
        if not Path(tool).is_file():sys.exit('Missing dependency: '+tool)
    target=Path('/opt/sleep-scheduler')
    target.mkdir(mode=0o755,parents=True,exist_ok=True)
    os.chown(target,0,0)
    # During upgrades, stop the engine while preserving schedule and audit state.
    subprocess.run(['systemctl','stop','sleep-scheduler.service'],capture_output=True)
    for name in ('engine.py','client.py','cli.py','gui.py','style.css','icon.svg','AGENTS.md','README.md'):
        write(str(target/name),(source/name).read_text())
    write('/usr/local/bin/sleep-scheduler','#!/bin/sh\nexec /usr/bin/python3 /opt/sleep-scheduler/cli.py "$@"\n',0o755)
    write('/usr/local/libexec/sleep-scheduler-control','#!/bin/sh\n[ "$#" -eq 1 ] || exit 2\nexec /usr/bin/python3 -I /opt/sleep-scheduler/engine.py control "$1"\n',0o755)
    write('/etc/systemd/system/sleep-scheduler.service','''[Unit]
Description=Sleep Scheduler verified RTC scheduling engine
After=time-sync.target
[Service]
Type=simple
ExecStart=/usr/bin/python3 -I /opt/sleep-scheduler/engine.py daemon
Restart=on-failure
RestartSec=3
UMask=0022
[Install]
WantedBy=multi-user.target
''')
    write('/usr/lib/systemd/system-sleep/sleep-scheduler','''#!/bin/sh
if [ "$1" = post ]; then
    /usr/bin/python3 -I /opt/sleep-scheduler/engine.py resumed
fi
''',0o755)
    write('/usr/share/applications/sleep-scheduler.desktop','''[Desktop Entry]
Type=Application
Name=Sleep Scheduler
Name[ar]=Sleep Scheduler — جدولة السكون
Comment=Verified sleep and wake schedules
Exec=/usr/local/bin/sleep-scheduler
Icon=sleep-scheduler
Terminal=false
Categories=Utility;
StartupNotify=true
''')
    write('/usr/share/icons/hicolor/scalable/apps/sleep-scheduler.svg',(source/'icon.svg').read_text())
    rule=Path('/etc/sudoers.d/.sleep-scheduler.tmp')
    write(str(rule),f'# Sleep Scheduler: validated schedule operations for one OS user only.\n#{uid} ALL=(root) NOPASSWD: /usr/local/libexec/sleep-scheduler-control\n',0o440)
    try:
        command('/usr/sbin/visudo','-cf',str(rule))
        rule.replace('/etc/sudoers.d/sleep-scheduler')
    finally:
        rule.unlink(missing_ok=True)
    command('/usr/sbin/visudo','-c')
    command('systemctl','daemon-reload')
    command('systemctl','enable','--now','sleep-scheduler.service')
    if shutil.which('update-desktop-database'):command('update-desktop-database','/usr/share/applications')
    if shutil.which('gtk-update-icon-cache'):
        subprocess.run(['gtk-update-icon-cache','-f','-t','/usr/share/icons/hicolor'],capture_output=True)
    print('Sleep Scheduler installed. No schedule was created or activated.')


if __name__=='__main__':main()
