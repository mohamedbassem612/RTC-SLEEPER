import datetime as dt
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('engine',Path(__file__).resolve().parents[1]/'engine.py')
e=importlib.util.module_from_spec(spec); spec.loader.exec_module(e)
ACTOR=dict(kind='agent',name='Test agent',model='test-model',source='cli')
CFG=dict(sleep_time='23:00',wake_time='07:00',now=False,days=[])

class EngineTests(unittest.TestCase):
    def setUp(self):
        self.folder=tempfile.TemporaryDirectory()
        self.root=Path(self.folder.name)
        self.rtc=self.root/'rtc'; self.rtc.write_text('')
        self.patches=[patch.object(e,'ROOT',self.root),patch.object(e,'RTC',self.rtc)]
        for p in self.patches:p.start()
        self.tz=os.environ.get('TZ');os.environ['TZ']='Africa/Cairo';time.tzset()
        self.commands=[]
        def fake(args):
            self.commands.append(args)
            if args[:3]==['/usr/sbin/rtcwake','-m','no']:
                self.rtc.write_text(args[-1])
            elif args[:3]==['/usr/sbin/rtcwake','-m','disable']:
                self.rtc.write_text('')
            return ''
        self.runner=patch.object(e,'run',side_effect=fake);self.runner.start()
    def tearDown(self):
        self.runner.stop()
        for p in self.patches:p.stop()
        if self.tz is None:os.environ.pop('TZ',None)
        else:os.environ['TZ']=self.tz
        time.tzset(); self.folder.cleanup()
    def request(self,op,p=None):
        return e.control(dict(op=op,payload=p or {},actor=ACTOR))
    def schedule(self,c=None):
        return self.request('schedule',dict(config=c or CFG,replace_alarm=False))
    def test_validation_and_injection(self):
        for value in ('24:00','7:00','08:00\n','$(reboot)'):
            with self.assertRaises(ValueError):e.validate_config(dict(CFG,wake_time=value))
        for days in ([True],[7],[1,1],['1']):
            with self.assertRaises(ValueError):e.validate_config(dict(CFG,days=days))
        with self.assertRaises(ValueError):self.request('poweroff')
        self.assertEqual(self.commands,[])
    def test_overnight_dates(self):
        now=dt.datetime(2026,11,15,9).timestamp()
        sleep,wake=e.next_cycle(CFG,now)
        self.assertEqual(dt.datetime.fromtimestamp(sleep),dt.datetime(2026,11,15,23))
        self.assertEqual(dt.datetime.fromtimestamp(wake),dt.datetime(2026,11,16,7))
    def test_repeat_weekdays(self):
        now=dt.datetime(2026,11,15,9).timestamp() # Sunday
        sleep,_=e.next_cycle(dict(CFG,days=[0]),now)
        self.assertEqual(dt.datetime.fromtimestamp(sleep),dt.datetime(2026,11,16,23))
    def test_dst_skips_nonexistent_time(self):
        os.environ['TZ']='America/New_York';time.tzset()
        sleep,_=e.next_cycle(dict(CFG,sleep_time='02:30'),dt.datetime(2026,3,8,0).timestamp())
        self.assertEqual(dt.datetime.fromtimestamp(sleep),dt.datetime(2026,3,9,2,30))
    def test_rtc_is_verified(self):
        state=self.schedule()
        self.assertEqual(state['expected_alarm'],int(self.rtc.read_text()))
        self.assertEqual(state['status'],'scheduled')
        self.assertFalse(any('suspend' in x for x in self.commands))
    def test_external_alarm_is_never_overwritten_implicitly(self):
        self.rtc.write_text('9999999999')
        with self.assertRaisesRegex(ValueError,'RTC_CONFLICT'):self.schedule()
        self.assertEqual(self.rtc.read_text(),'9999999999')
        self.assertEqual(self.commands,[])
    def test_override_during_schedule_blocks_suspend(self):
        state=self.schedule();state['next_sleep']=time.time()-1;e.save(state)
        self.rtc.write_text('9999999999')
        e.tick()
        self.assertEqual(e.read_state()['status'],'conflict')
        self.assertFalse(any('suspend' in x for x in self.commands))
        self.request('cancel')
        self.assertEqual(self.rtc.read_text(),'9999999999')
    def test_pause_clears_owned_alarm(self):
        self.schedule();self.request('pause')
        self.assertEqual(self.rtc.read_text(),'')
        self.assertEqual(e.read_state()['status'],'paused')
        self.request('resume',dict(replace_alarm=False))
        self.assertEqual(e.read_state()['status'],'scheduled')
    def test_resume_resets_one_time_schedule_even_if_woken_early(self):
        self.schedule();e.resumed()
        state=e.read_state()
        self.assertEqual(state['status'],'idle')
        self.assertIsNone(state['config'])
        self.assertEqual(self.rtc.read_text(),'')
    def test_repeat_does_not_sleep_twice_on_same_day(self):
        state=self.schedule(dict(CFG,days=list(range(7))))
        old=dt.datetime.fromtimestamp(state['next_sleep']).date()
        e.resumed()
        state=e.read_state()
        self.assertEqual(state['status'],'scheduled')
        self.assertGreater(dt.datetime.fromtimestamp(state['next_sleep']).date(),old)
    def test_missed_deadline_does_not_suspend(self):
        state=self.schedule();state['next_sleep']=time.time()-180;e.save(state)
        e.tick()
        self.assertEqual(e.read_state()['status'],'idle')
        self.assertFalse(any('suspend' in x for x in self.commands))
    def test_suspend_failure_clears_own_alarm(self):
        state=self.schedule();state['next_sleep']=time.time()-1;e.save(state)
        original=e.run.side_effect
        def fail(args):
            if args==['/usr/bin/systemctl','suspend']:raise RuntimeError('inhibited')
            return original(args)
        e.run.side_effect=fail
        e.tick()
        self.assertEqual(e.read_state()['status'],'error')
        self.assertEqual(self.rtc.read_text(),'')
    def test_audit_records_creator_and_system_trigger(self):
        state=self.schedule();state['next_sleep']=time.time()-1;e.save(state)
        e.tick()
        records=[json.loads(x) for x in (self.root/'events.jsonl').read_text().splitlines()]
        trigger=next(x for x in records if x['action']=='sleep_triggered')
        self.assertEqual(trigger['actor']['kind'],'system')
        self.assertEqual(trigger['created_by']['kind'],'agent')
        self.assertEqual(trigger['created_by']['model'],'test-model')
    def test_sleep_now_has_verified_wake_and_grace(self):
        state=self.schedule(dict(CFG,now=True))
        self.assertGreaterEqual(state['next_sleep'],int(time.time())+3)
        self.assertEqual(state['next_wake'],int(self.rtc.read_text()))
    def test_no_shutdown_command_anywhere(self):
        self.schedule();self.request('pause');self.request('resume',dict(replace_alarm=False));self.request('cancel')
        self.assertFalse(any('poweroff' in str(x) or 'shutdown' in str(x) for x in self.commands))

if __name__=='__main__':unittest.main()
