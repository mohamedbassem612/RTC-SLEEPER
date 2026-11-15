import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import gui
from gi.repository import Gtk,Gdk

class InterfaceTests(unittest.TestCase):
    def test_fixed_size_language_and_repeat_controls(self):
        provider=Gtk.CssProvider()
        provider.load_from_path(str(Path(gui.__file__).with_name('style.css')))
        Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(),provider,Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        state=dict(status='idle',config=None,next_sleep=None,next_wake=None,expected_alarm=None,
                   actual_alarm=None,rtc_readable=True,engine_running=True,message='')
        with tempfile.TemporaryDirectory() as folder, patch.object(gui,'PREFS',Path(folder)/'preferences.json'),patch.object(gui.client,'status',return_value=state):
            w=gui.Window(preview=True)
            w.check_resize();w.size_allocate(w.get_allocation())
            self.assertEqual(tuple(w.get_size()),(1000,720))
            self.assertEqual(w.lang,'en')
            w.select_all()
            self.assertEqual(w.current_config()['days'],list(range(7)))
            w.now_toggle.set_active(True)
            self.assertFalse(w.sleep_row.get_sensitive())
            self.assertEqual(w.current_config()['days'],[])
            class Combo:
                def get_active_id(self):return 'ar'
            w.change_language(Combo())
            w.check_resize();w.size_allocate(w.get_allocation())
            self.assertEqual(w.lang,'ar')
            self.assertEqual(w.stack.get_visible_child_name(),'settings')
            self.assertEqual(tuple(w.get_size()),(1000,720))
            w.destroy()

if __name__=='__main__':unittest.main()
