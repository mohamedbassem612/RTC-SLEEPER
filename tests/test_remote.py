"""Loopback integration test. The mock gateway cannot execute power commands."""
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]

class RemoteIntegration(unittest.TestCase):
    def test_pairing_and_commands_without_power_actions(self):
        binary = ROOT/'remote/target/debug/rtc-sleeper-remote'
        with tempfile.TemporaryDirectory(prefix='rtc-remote-test-') as folder:
            with socket.socket() as reservation:
                reservation.bind(('127.0.0.1',0))
                port = reservation.getsockname()[1]
            env = dict(os.environ, XDG_CONFIG_HOME=folder, RTC_MOCK_PORT=str(port))
            process = subprocess.Popen([str(binary), 'serve', '--mock'], env=env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                control = Path(folder)/'rtc-sleeper-remote/control.sock'
                for _ in range(100):
                    if control.exists():
                        try:
                            with socket.create_connection(('127.0.0.1',port),timeout=.1): pass
                            break
                        except OSError: pass
                    if process.poll() is not None: self.fail(process.stderr.read().decode())
                    time.sleep(.05)
                def exchange(family, address, request):
                    with socket.socket(family, socket.SOCK_STREAM) as stream:
                        stream.settimeout(5); stream.connect(address)
                        stream.sendall(json.dumps(request).encode()+b'\n')
                        return json.loads(stream.makefile('rb').readline())
                def local(op, **fields): return exchange(socket.AF_UNIX,str(control),{'op':op, **fields})['result']
                ticket = local('pair')
                self.assertEqual(len(ticket['phrase'].split()), 24)
                self.assertTrue(ticket['uri'].startswith('rtcsleeper://pair/'))
                pair = {'op':'pair','code':ticket['phrase'],'device':'Integration test Android'}
                address = ('127.0.0.1', port)
                paired = exchange(socket.AF_INET,address,pair)
                self.assertTrue(paired['ok'])
                token = paired['result']['token']
                self.assertFalse(exchange(socket.AF_INET,address,pair)['ok'])  # single use
                self.assertNotEqual(local('pair')['phrase'],ticket['phrase'])  # burned, so a new one
                devices=local('devices')
                self.assertTrue(devices['enabled'])
                self.assertEqual(len(devices['devices']),1)
                self.assertNotIn('token',json.dumps(devices))
                def request(op,payload):
                    return {'op':op,'payload':payload,'token':token,'id':uuid.uuid4().hex,'at':int(time.time())}
                def call(op,payload):
                    reply=exchange(socket.AF_INET,address,request(op,payload))
                    self.assertTrue(reply['ok'],reply.get('error'))
                    return reply['result']
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                first=devices['devices'][0]['id']
                local('allow',id=first)
                self.assertEqual(call('status',{})['status'],'idle')
                local('block',id=first)
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                local('allow',id=first)
                self.assertEqual(call('status',{})['status'],'idle')
                local('disable')
                self.assertFalse(exchange(socket.AF_INET,address,pair)['ok'])
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                local('enable')
                self.assertEqual(call('status',{})['status'],'idle')
                local('regenerate')
                self.assertFalse(exchange(socket.AF_INET,address,pair)['ok'])
                cfg={'sleep_time':'23:00','wake_time':'07:00','now':False,'days':[0,1]}
                self.assertEqual(call('schedule',cfg)['status'],'scheduled')
                self.assertEqual(call('pause',{})['status'],'paused')
                self.assertEqual(call('resume',{})['status'],'scheduled')
                self.assertEqual(call('cancel',{})['status'],'idle')
                cfg.update(now=True,days=[])
                self.assertEqual(call('schedule',cfg)['status'],'scheduled')
                repeated=request('cancel',{})
                self.assertTrue(exchange(socket.AF_INET,address,repeated)['ok'])
                self.assertFalse(exchange(socket.AF_INET,address,repeated)['ok'])
                self.assertFalse(exchange(socket.AF_INET,address,request('shutdown',{}))['ok'])
                cfg['wake_time']='73:00'
                self.assertFalse(exchange(socket.AF_INET,address,request('schedule',cfg))['ok'])
                saved=local('pair')['phrase']
                local('block',id=first)
                local('disable')
                process.terminate()
                process.wait(timeout=5)
                process.stderr.close()
                process=subprocess.Popen([str(binary),'serve','--mock'],env=env,
                                         stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
                for _ in range(100):
                    try:
                        access=local('devices')
                        break
                    except (OSError,KeyError): time.sleep(.05)
                self.assertFalse(access['enabled'])
                self.assertFalse(access['devices'][0]['allowed'])
                self.assertEqual(local('pair')['phrase'],saved)
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                local('enable')
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                local('allow',id=first)
                self.assertEqual(call('status',{})['status'],'idle')
                local('remove',id=first)
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                self.assertEqual(local('devices')['devices'],[])
                local('revoke')
                self.assertFalse(exchange(socket.AF_INET,address,request('status',{}))['ok'])
                self.assertEqual((control.stat().st_mode & 0o777),0o600)
            finally:
                process.terminate()
                try: process.wait(timeout=5)
                except subprocess.TimeoutExpired: process.kill(); process.wait()
                process.stderr.close()

if __name__=='__main__': unittest.main()
