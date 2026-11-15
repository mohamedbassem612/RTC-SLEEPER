#!/usr/bin/python3
import datetime as dt
import json
import math
from pathlib import Path
import threading
import time
import gi
gi.require_version('Gtk', '3.0')
gi.require_foreign('cairo')
from gi.repository import Gtk, Gdk, GLib
import client

PREFS = Path.home() / '.config/sleep-scheduler/preferences.json'
T = {
'en': {
 'tagline':'Sleep peacefully. Wake on time.', 'settings':'Settings', 'ready':'Ready to schedule',
 'active':'Schedule active', 'paused':'Schedule paused', 'conflict':'Schedule conflict', 'error':'Needs attention',
 'sleep_now':'Sleep now', 'sleep_now_hint':'Use the wake time below, then put your PC to sleep.',
 'sleep_at':'Sleep at', 'wake_at':'Wake up at', 'repeat':'Repeat', 'once':'One time',
 'duration':'Sleep duration', 'schedule':'Schedule sleep', 'clear':'Clear', 'scheduled_trigger':'Scheduled trigger',
 'will_sleep':'Will sleep in', 'time_until':'Time until sleep', 'scheduled':'Sleep scheduled',
 'verified':'RTC verified', 'edit':'Edit schedule', 'pause':'Pause', 'resume':'Resume schedule',
 'remove':'Remove', 'next_action':'Next action', 'back':'Back', 'language':'Language',
 'language_hint':'Choose the language used throughout the app.', 'logs':'Logs',
 'logs_hint':'See requests, triggers, resume events and RTC conflicts.',
 'rtc':'RTC wake alarm', 'none':'No alarm set', 'engine':'Scheduling engine',
 'running':'Running', 'stopped':'Stopped', 'refresh':'Refresh', 'log_empty':'No events yet.',
 'paused_hint':'Sleep and this schedule’s wake alarm are paused.', 'conflict_hint':'Wake alarm changed. Scheduled sleep is blocked.',
 'sleeping':'Entering sleep', 'author':'Created by', 'now':'Now', 'today':'Today', 'tomorrow':'Tomorrow',
 'repeat_hint':'Selected days refer to when sleep begins.', 'all_days':'Every day',
 'settings_note':'Your schedule stays active when this window is closed.', 'idle_note':'Closing this window keeps your schedule active.',
 'preview':'Preview mode — scheduling is unavailable until installation.', 'retry':'Try again',
 'replace_title':'Replace an external RTC alarm?',
 'replace_hint':'Another program has set a wake alarm. Replace that alarm with this schedule? This will change its wake time.',
 'replace':'Replace alarm', 'cancel':'Cancel', 'saved':'Schedule verified and saved.',
 'reset':'Previous sleep cycle completed and reset.', 'model_note':'Agent and model labels in logs are self-declared; the OS user is recorded separately.',
 'days':['Mon','Tue','Wed','Thu','Fri','Sat','Sun'],
},
'ar': {
 'tagline':'سكون هادئ. إيقاظ في الموعد.', 'settings':'الإعدادات', 'ready':'جاهز للجدولة',
 'active':'الجدول مفعّل', 'paused':'الجدول معلّق', 'conflict':'تعارض في الجدول', 'error':'يحتاج مراجعة',
 'sleep_now':'السكون الآن', 'sleep_now_hint':'حدد وقت الإيقاظ بالأسفل ثم أدخل الجهاز في السكون.',
 'sleep_at':'السكون الساعة', 'wake_at':'الإيقاظ الساعة', 'repeat':'التكرار', 'once':'مرة واحدة',
 'duration':'مدة السكون', 'schedule':'جدولة السكون', 'clear':'مسح', 'scheduled_trigger':'تفاصيل الجدول',
 'will_sleep':'السكون خلال', 'time_until':'الوقت المتبقي للسكون', 'scheduled':'تمت جدولة السكون',
 'verified':'تم التحقق من RTC', 'edit':'تعديل الجدول', 'pause':'تعليق', 'resume':'استئناف الجدول',
 'remove':'إلغاء الجدول', 'next_action':'الإجراء القادم', 'back':'رجوع', 'language':'اللغة',
 'language_hint':'اختار لغة واجهة البرنامج.', 'logs':'السجلات',
 'logs_hint':'طلبات المستخدم والـAI والتنفيذ والاستئناف وتعارضات RTC.',
 'rtc':'منبّه إيقاظ RTC', 'none':'لا يوجد منبّه', 'engine':'محرك الجدولة',
 'running':'يعمل', 'stopped':'متوقف', 'refresh':'تحديث', 'log_empty':'لا توجد سجلات بعد.',
 'paused_hint':'السكون ومنبّه إيقاظ هذا الجدول معلّقان.', 'conflict_hint':'منبّه الإيقاظ تغيّر؛ تم منع السكون المجدول.',
 'sleeping':'جارٍ دخول السكون', 'author':'أنشأه', 'now':'الآن', 'today':'اليوم', 'tomorrow':'بكرة',
 'repeat_hint':'الأيام المحددة هي أيام بداية السكون.', 'all_days':'كل يوم',
 'settings_note':'غلق النافذة يترك الجدول مفعّلًا.', 'idle_note':'غلق النافذة يترك الجدول مفعّلًا.',
 'preview':'وضع المعاينة — الجدولة غير متاحة قبل التثبيت.', 'retry':'حاول مجددًا',
 'replace_title':'استبدال منبّه RTC خارجي؟',
 'replace_hint':'برنامج آخر ضبط منبّه إيقاظ. هل تريد استبداله بمنبّه هذا الجدول؟ سيتغيّر وقت الإيقاظ الخاص به.',
 'replace':'استبدال المنبّه', 'cancel':'إلغاء', 'saved':'تم حفظ الجدول والتحقق من المنبّه.',
 'reset':'انتهت دورة السكون السابقة وتمت إعادة الضبط.',
 'model_note':'اسم الـagent والموديل بيانات يقدّمها صاحب الطلب؛ يُسجّل مستخدم النظام بشكل منفصل.',
 'days':['إثن','ثلا','أرب','خمي','جمع','سبت','أحد'],
}}


def css(widget, name):
    widget.get_style_context().add_class(name)
    return widget


def label(text='', style=None, x=0):
    w = Gtk.Label(label=text)
    w.set_xalign(x)
    if style:
        css(w, style)
    return w


def button(text, callback, style=None):
    w = Gtk.Button(label=text)
    if style:
        css(w, style)
    w.connect('clicked', callback)
    return w


def duration(seconds, lang='en', seconds_on=False):
    seconds = max(0, int(seconds or 0))
    hours, rem = divmod(seconds, 3600)
    minutes, secs = divmod(rem, 60)
    if seconds_on:
        return f'{hours:02d}:{minutes:02d}:{secs:02d}'
    if lang == 'ar':
        return f'{hours} س {minutes} د' if hours else f'{minutes} دقيقة'
    return f'{hours} h {minutes:02d} min' if hours else f'{minutes} min'


class Symbol(Gtk.DrawingArea):
    def __init__(self, kind, size=45):
        super().__init__()
        self.kind = kind
        self.set_size_request(size, size)
        self.connect('draw', self.draw)

    def draw(self, widget, cr):
        cr.scale(self.get_allocated_width()/48, self.get_allocated_height()/48)
        cr.set_source_rgb(.95,.95,.95)
        cr.set_line_width(2.6)
        cr.set_line_cap(1)
        cr.set_line_join(1)
        if self.kind == 'moon':
            cr.arc(24,24,19,.2,5.0)
            cr.arc_negative(32,16,16,4.4,1.75)
            cr.close_path(); cr.fill()
        elif self.kind == 'bed':
            cr.move_to(7,12); cr.line_to(7,38); cr.move_to(7,31); cr.line_to(42,31)
            cr.move_to(42,24); cr.line_to(42,38); cr.move_to(8,23); cr.line_to(41,23)
            cr.stroke(); cr.rectangle(11,17,9,5); cr.fill()
            cr.rectangle(23,17,16,7); cr.fill()
        elif self.kind == 'alarm':
            cr.arc(24,26,14,0,math.tau); cr.stroke()
            cr.move_to(24,18); cr.line_to(24,26); cr.line_to(30,29)
            cr.move_to(10,8); cr.line_to(5,14); cr.move_to(38,8); cr.line_to(43,14)
            cr.move_to(15,39); cr.line_to(11,44); cr.move_to(33,39); cr.line_to(37,44); cr.stroke()
        else:
            cr.arc(24,27,15,0,math.tau); cr.stroke()
            cr.move_to(24,20); cr.line_to(24,28); cr.line_to(30,31)
            cr.move_to(19,5); cr.line_to(29,5); cr.move_to(24,5); cr.line_to(24,11); cr.stroke()
        return False


class TimePicker(Gtk.Box):
    def __init__(self, value):
        super().__init__(spacing=7)
        self.set_direction(Gtk.TextDirection.LTR)
        hour, minute = map(int, value.split(':'))
        self.hour = Gtk.SpinButton.new_with_range(1,12,1)
        self.hour.set_value(hour % 12 or 12)
        self.hour.set_numeric(True)
        self.hour.set_wrap(True)
        self.minute = Gtk.SpinButton.new_with_range(0,59,1)
        self.minute.set_value(minute)
        self.minute.set_numeric(True)
        self.minute.set_wrap(True)
        self.minute.connect('output', self.format_minute)
        self.period = Gtk.ComboBoxText()
        for p in ('AM','PM'):
            self.period.append_text(p)
        self.period.set_active(1 if hour >=12 else 0)
        for w in (self.hour, label(':'), self.minute, self.period):
            self.pack_start(w,False,False,0)

    @staticmethod
    def format_minute(spin):
        spin.set_text(f'{spin.get_value_as_int():02d}')
        return True

    def value(self):
        self.hour.update(); self.minute.update()
        h = self.hour.get_value_as_int()%12 + 12*self.period.get_active()
        return f'{h:02d}:{self.minute.get_value_as_int():02d}'

    def changed(self, callback):
        self.hour.connect('value-changed', callback)
        self.minute.connect('value-changed', callback)
        self.period.connect('changed', callback)


class Ring(Gtk.DrawingArea):
    def __init__(self):
        super().__init__()
        self.progress = 0
        self.set_size_request(326,326)
        self.connect('draw', self.draw)

    def draw(self, widget, cr):
        w,h = self.get_allocated_width(), self.get_allocated_height()
        r=min(w,h)/2-13
        cr.set_line_width(9)
        cr.set_line_cap(1)
        cr.set_source_rgb(.16,.16,.16)
        cr.arc(w/2,h/2,r,0,math.tau); cr.stroke()
        cr.set_source_rgb(1,1,1)
        cr.arc(w/2,h/2,r,-math.pi/2,-math.pi/2+math.tau*max(.006,min(1,self.progress)))
        cr.stroke()
        return False


class Window(Gtk.Window):
    def __init__(self, preview=False):
        super().__init__(title='Sleep Scheduler')
        self.preview = preview
        self.set_default_size(1000,720)
        self.set_size_request(1000,720)
        self.set_resizable(False)
        self.set_position(Gtk.WindowPosition.CENTER)
        self.set_icon_from_file(str(Path(__file__).with_name('icon.svg')))
        self.connect('destroy', lambda *_: Gtk.main_quit() if Gtk.main_level() else None)
        self.settings = {'language':'en'}
        if PREFS.exists():
            try:
                self.settings.update(json.loads(PREFS.read_text()))
            except ValueError:
                pass
        self.lang = self.settings.get('language','en')
        if self.lang not in T:
            self.lang='en'
        self.busy=False
        self.editing=False
        self.previous_cycle=None
        self.last_state={}
        self.config=dict(sleep_time='23:00',wake_time='07:00',now=False,days=[])
        self.build()
        self.poll()
        GLib.timeout_add_seconds(1,self.poll)

    def tr(self,key):
        return T[self.lang].get(key,key)

    def date_time(self,stamp):
        if not stamp:
            return '—'
        value=dt.datetime.fromtimestamp(stamp)
        today=dt.date.today()
        day=self.tr('today') if value.date()==today else self.tr('tomorrow') if value.date()==today+dt.timedelta(days=1) else value.strftime('%a %d %b')
        return day+' · '+value.strftime('%I:%M %p').lstrip('0')

    def build(self):
        old=self.get_child()
        if old:
            self.remove(old)
            old.destroy()
        root=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=16)
        root.set_border_width(28)
        root.set_direction(Gtk.TextDirection.RTL if self.lang=='ar' else Gtk.TextDirection.LTR)
        css(root,'root')
        self.add(root)
        header=Gtk.Box(spacing=14)
        header.pack_start(Symbol('moon',52),False,False,0)
        titles=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=4)
        titles.pack_start(label('Sleep Scheduler','title'),False,False,0)
        titles.pack_start(label(self.tr('tagline'),'subtitle'),False,False,0)
        header.pack_start(titles,True,True,0)
        self.pill=label(self.tr('ready'),'pill')
        header.pack_end(button('⚙  '+self.tr('settings'),lambda *_:self.show_page('settings'),'flat'),False,False,0)
        header.pack_end(self.pill,False,False,0)
        root.pack_start(header,False,False,0)
        self.stack=Gtk.Stack()
        self.stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        self.stack.set_transition_duration(330)
        self.stack.set_homogeneous(True)
        root.pack_start(self.stack,True,True,0)
        self.make_editor()
        self.make_dashboard()
        self.make_settings()
        self.make_logs()
        self.notice=label('','muted')
        self.notice.set_line_wrap(True)
        self.notice.set_max_width_chars(110)
        self.notice.set_size_request(-1,34)
        root.pack_end(self.notice,False,False,0)
        self.show_all()
        self.now_hint.set_no_show_all(True)
        self.days_box.set_no_show_all(True)
        self.editor_changed()
        self.show_page('editor')

    def make_editor(self):
        page=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=12)
        self.stack.add_named(page,'editor')
        row=css(Gtk.Box(spacing=16),'card')
        row.pack_start(Symbol('bed'),False,False,0)
        text=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=4)
        text.pack_start(label(self.tr('sleep_now'),'section-title'),False,False,0)
        text.pack_start(label(self.tr('sleep_now_hint'),'muted'),False,False,0)
        row.pack_start(text,True,True,0)
        self.now_toggle=Gtk.Switch(active=self.config['now'])
        self.now_toggle.set_valign(Gtk.Align.CENTER)
        self.now_toggle.connect('notify::active',self.editor_changed)
        row.pack_end(self.now_toggle,False,False,0)
        page.pack_start(row,False,False,0)
        for field,key,symbol in [('sleep','sleep_at','moon'),('wake','wake_at','alarm')]:
            row=css(Gtk.Box(spacing=16),'card')
            row.pack_start(Symbol(symbol,39),False,False,0)
            row.pack_start(label(self.tr(key),'section-title'),True,True,0)
            picker=TimePicker(self.config[field+'_time'])
            picker.changed(self.editor_changed)
            row.pack_end(picker,False,False,0)
            setattr(self,field+'_picker',picker)
            setattr(self,field+'_row',row)
            page.pack_start(row,False,False,0)
        repeat=Gtk.Box(spacing=12)
        self.repeat_toggle=Gtk.CheckButton(label=self.tr('repeat'))
        self.repeat_toggle.set_active(bool(self.config['days']))
        self.repeat_toggle.connect('toggled',self.editor_changed)
        repeat.pack_start(self.repeat_toggle,False,False,0)
        self.days_box=Gtk.Box(spacing=5)
        self.day_buttons=[]
        for n,day in enumerate(T[self.lang]['days']):
            b=css(Gtk.ToggleButton(label=day),'day')
            b.set_active(n in self.config['days'])
            b.connect('toggled',self.editor_changed)
            self.day_buttons.append(b)
            self.days_box.pack_start(b,False,False,0)
        repeat.pack_start(self.days_box,True,True,0)
        self.every_day=button(self.tr('all_days'),self.select_all,'flat')
        repeat.pack_end(self.every_day,False,False,0)
        page.pack_start(repeat,False,False,0)
        summary=css(Gtk.Box(spacing=18),'summary')
        summary.pack_start(Symbol('duration',49),False,False,0)
        text=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=4)
        text.pack_start(label(self.tr('duration'),'muted'),False,False,0)
        self.duration_label=label('—','value'); css(self.duration_label,'accent')
        text.pack_start(self.duration_label,False,False,0)
        summary.pack_start(text,False,False,0)
        self.range_label=label('','muted',1)
        self.range_label.set_line_wrap(True)
        summary.pack_end(self.range_label,True,True,0)
        page.pack_start(summary,False,False,0)
        self.now_hint=label(self.tr('now'),'warning')
        page.pack_start(self.now_hint,False,False,0)
        actions=Gtk.Box(spacing=14)
        self.submit_button=button(self.tr('schedule'),self.submit,'primary')
        actions.pack_start(self.submit_button,True,True,0)
        actions.pack_end(button(self.tr('clear'),self.clear_editor),False,False,0)
        page.pack_end(actions,False,False,0)

    def select_all(self,*_):
        self.repeat_toggle.set_active(True)
        for b in self.day_buttons:
            b.set_active(True)
        self.editor_changed()

    def current_config(self):
        return dict(sleep_time=self.sleep_picker.value(),wake_time=self.wake_picker.value(),
                    now=self.now_toggle.get_active(),days=[n for n,b in enumerate(self.day_buttons) if b.get_active()] if self.repeat_toggle.get_active() and not self.now_toggle.get_active() else [])

    def editor_changed(self,*_):
        if not hasattr(self,'day_buttons'):
            return
        now=self.now_toggle.get_active()
        self.sleep_row.set_sensitive(not now)
        self.sleep_row.set_opacity(0.35 if now else 1.0)
        self.repeat_toggle.set_sensitive(not now)
        repeating=self.repeat_toggle.get_active() and not now
        self.days_box.set_visible(repeating)
        self.every_day.set_visible(repeating)
        self.submit_button.set_label(self.tr('sleep_now') if now else self.tr('schedule'))
        self.submit_button.set_sensitive(not self.preview and not self.busy)
        try:
            from engine import next_cycle,validate_config
            c=self.current_config()
            if repeating and not c['days']:
                self.range_label.set_text(self.tr('repeat_hint'))
                self.submit_button.set_sensitive(False)
                return
            sleep,wake=next_cycle(validate_config(c))
            self.duration_label.set_text(duration(wake-sleep,self.lang))
            self.range_label.set_text((self.tr('now') if now else self.date_time(sleep))+'  →  '+self.date_time(wake))
            self.now_hint.set_visible(False)
        except Exception as exc:
            self.duration_label.set_text('—')
            self.range_label.set_text(str(exc))
            self.submit_button.set_sensitive(False)

    def make_dashboard(self):
        page=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=18)
        self.stack.add_named(page,'dashboard')
        upper=Gtk.Box(spacing=18)
        upper.set_valign(Gtk.Align.CENTER)
        page.pack_start(upper,True,True,0)
        card=css(Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=14),'card')
        card.set_size_request(248,-1)
        card.set_valign(Gtk.Align.CENTER)
        card.pack_start(label(self.tr('scheduled_trigger'),'section-title'),False,False,0)
        self.detail={}
        for name,key in [('sleep','sleep_at'),('wake','wake_at'),('duration','duration'),('remaining','will_sleep')]:
            box=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=4)
            box.pack_start(label(self.tr(key),'muted'),False,False,0)
            v=label('—','section-title')
            if name=='remaining': css(v,'accent')
            v.set_line_wrap(True)
            self.detail[name]=v
            if name=='remaining': self.remaining_box=box
            box.pack_start(v,False,False,0)
            card.pack_start(box,False,False,0)
            if name!='remaining': card.pack_start(Gtk.Separator(),False,False,0)
        upper.pack_start(card,False,False,0)
        overlay=Gtk.Overlay()
        self.ring=Ring()
        overlay.add(self.ring)
        center=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=10)
        center.set_valign(Gtk.Align.CENTER); center.set_halign(Gtk.Align.CENTER)
        icon=Symbol('bed',45); icon.set_halign(Gtk.Align.CENTER)
        center.pack_start(icon,False,False,0)
        self.main_status=label(self.tr('scheduled'),'section-title',.5)
        center.pack_start(self.main_status,False,False,0)
        self.verified=label('✓ '+self.tr('verified'),'green',.5)
        center.pack_start(self.verified,False,False,0)
        center.pack_start(label(self.tr('time_until'),'muted',.5),False,False,0)
        self.countdown=label('00:00:00','value',.5); css(self.countdown,'accent')
        self.countdown.set_direction(Gtk.TextDirection.LTR)
        center.pack_start(self.countdown,False,False,0)
        overlay.add_overlay(center)
        upper.pack_start(overlay,True,True,0)
        actions=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=16)
        actions.set_valign(Gtk.Align.CENTER); actions.set_size_request(205,-1)
        actions.pack_start(button(self.tr('edit'),self.edit_schedule,'primary'),False,False,0)
        self.pause_button=button(self.tr('pause'),self.pause_resume)
        actions.pack_start(self.pause_button,False,False,0)
        self.remove_button=button(self.tr('remove'),lambda *_:self.perform('cancel',{}))
        actions.pack_start(self.remove_button,False,False,0)
        self.author_label=label('','muted')
        self.author_label.set_line_wrap(True); self.author_label.set_max_width_chars(23)
        actions.pack_start(self.author_label,False,False,0)
        upper.pack_end(actions,False,False,0)
        self.next_action=css(label('','pill',.5),'idle-pill')
        page.pack_end(self.next_action,False,False,0)

    def make_settings(self):
        page=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=18)
        self.stack.add_named(page,'settings')
        row=Gtk.Box(spacing=12)
        row.pack_start(button('‹  '+self.tr('back'),lambda *_:self.back(),'flat'),False,False,0)
        row.pack_start(label(self.tr('settings'),'section-title'),False,False,0)
        page.pack_start(row,False,False,0)
        card=css(Gtk.Box(spacing=14),'card')
        text=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=5)
        text.pack_start(label(self.tr('language'),'section-title'),False,False,0)
        text.pack_start(label(self.tr('language_hint'),'muted'),False,False,0)
        card.pack_start(text,True,True,0)
        combo=Gtk.ComboBoxText()
        combo.append('en','English'); combo.append('ar','العربية')
        combo.set_active_id(self.lang)
        combo.connect('changed',self.change_language)
        card.pack_end(combo,False,False,0)
        page.pack_start(card,False,False,0)
        card=css(Gtk.Box(spacing=14),'card')
        text=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=5)
        text.pack_start(label(self.tr('logs'),'section-title'),False,False,0)
        text.pack_start(label(self.tr('logs_hint'),'muted'),False,False,0)
        card.pack_start(text,True,True,0)
        card.pack_end(button(self.tr('logs')+'  ›',lambda *_:self.open_logs()),False,False,0)
        page.pack_start(card,False,False,0)
        card=css(Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=12),'card')
        self.engine_label=label('','green')
        self.rtc_label=label('','muted')
        self.rtc_label.set_line_wrap(True)
        card.pack_start(self.engine_label,False,False,0)
        card.pack_start(self.rtc_label,False,False,0)
        page.pack_start(card,False,False,0)
        page.pack_end(label(self.tr('settings_note'),'muted',.5),False,False,0)

    def make_logs(self):
        page=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=14)
        self.stack.add_named(page,'logs')
        row=Gtk.Box(spacing=12)
        row.pack_start(button('‹  '+self.tr('back'),lambda *_:self.show_page('settings'),'flat'),False,False,0)
        row.pack_start(label(self.tr('logs'),'section-title'),True,True,0)
        row.pack_end(button(self.tr('refresh'),lambda *_:self.open_logs()),False,False,0)
        page.pack_start(row,False,False,0)
        scroll=Gtk.ScrolledWindow()
        self.log_view=Gtk.TextView()
        self.log_view.set_editable(False); self.log_view.set_cursor_visible(False)
        self.log_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.log_view.set_left_margin(14); self.log_view.set_right_margin(14)
        self.log_view.set_top_margin(12); self.log_view.set_bottom_margin(12)
        self.log_view.set_direction(Gtk.TextDirection.LTR)
        scroll.add(self.log_view)
        page.pack_start(scroll,True,True,0)
        note=label(self.tr('model_note'),'muted')
        note.set_line_wrap(True)
        page.pack_end(note,False,False,0)

    def change_language(self,combo):
        lang=combo.get_active_id()
        if lang==self.lang:
            return
        self.config=self.current_config()
        self.lang=lang
        self.settings['language']=lang
        PREFS.parent.mkdir(parents=True,exist_ok=True)
        PREFS.write_text(json.dumps(self.settings))
        self.build(); self.show_page('settings'); self.poll()

    def show_page(self,name):
        self.stack.set_visible_child_name(name)

    def back(self):
        self.show_page('dashboard' if self.last_state.get('status') in ('scheduled','paused','conflict','error','sleeping') and self.last_state.get('config') else 'editor')

    def open_logs(self):
        records=client.events(120)
        pieces=[]
        for e in records:
            a=e.get('actor',{})
            who=('AI Agent' if a.get('kind')=='agent' else 'System' if a.get('kind')=='system' else 'User')
            who+=' · '+a.get('name','')
            if a.get('model'): who+=' · '+a['model']
            who+=' · '+a.get('source','')
            if 'uid' in a: who+=f" · UID {a['uid']}"
            detail={k:v for k,v in e.items() if k not in ('time','action','actor','result')}
            pieces.append(f"{e['time']}   {e['action']}   [{e.get('result','ok')}]\n{who}\n"+(json.dumps(detail,ensure_ascii=False) if detail else '')+'\n')
        self.log_view.get_buffer().set_text('\n'.join(pieces) or self.tr('log_empty'))
        self.show_page('logs')

    def edit_schedule(self,*_):
        self.config=self.last_state.get('config') or self.config
        self.editing=True
        self.load_config(self.config)
        self.show_page('editor')

    def load_config(self,c):
        self.now_toggle.set_active(c['now'])
        for picker,key in [(self.sleep_picker,'sleep_time'),(self.wake_picker,'wake_time')]:
            h,m=map(int,c[key].split(':'))
            picker.hour.set_value(h%12 or 12); picker.minute.set_value(m); picker.period.set_active(h//12)
        self.repeat_toggle.set_active(bool(c['days']))
        for n,b in enumerate(self.day_buttons): b.set_active(n in c['days'])
        self.editor_changed()

    def clear_editor(self,*_):
        if self.last_state.get('config'):
            self.perform('cancel',{})
        else:
            self.load_config(dict(sleep_time='23:00',wake_time='07:00',now=False,days=[]))
            self.notice.set_text('')

    def submit(self,*_):
        self.perform('schedule',dict(config=self.current_config(),replace_alarm=False))

    def pause_resume(self,*_):
        op='resume' if self.last_state.get('status')=='paused' else 'pause'
        self.perform(op,dict(replace_alarm=False) if op=='resume' else {})

    def perform(self,op,payload):
        if self.busy or self.preview:
            return
        self.busy=True
        self.submit_button.set_sensitive(False)
        self.pause_button.set_sensitive(False); self.remove_button.set_sensitive(False)
        self.notice.set_text('…')
        def worker():
            try:
                state=client.request(op,payload,source='gui')
                GLib.idle_add(self.performed,op,payload,state,None)
            except Exception as exc:
                GLib.idle_add(self.performed,op,payload,None,str(exc))
        threading.Thread(target=worker,daemon=True).start()

    def performed(self,op,payload,state,error):
        self.busy=False
        self.submit_button.set_sensitive(True)
        self.pause_button.set_sensitive(True); self.remove_button.set_sensitive(True)
        if error:
            self.notice.set_text(error)
            if 'RTC_CONFLICT:' in error and op in ('schedule','resume'):
                dialog=Gtk.MessageDialog(transient_for=self,modal=True,message_type=Gtk.MessageType.WARNING,
                                         buttons=Gtk.ButtonsType.NONE,text=self.tr('replace_title'))
                dialog.format_secondary_text(self.tr('replace_hint')+'\n'+error)
                dialog.add_button(self.tr('cancel'),Gtk.ResponseType.CANCEL)
                dialog.add_button(self.tr('replace'),Gtk.ResponseType.OK)
                response=dialog.run(); dialog.destroy()
                if response==Gtk.ResponseType.OK:
                    payload['replace_alarm']=True
                    self.perform(op,payload)
            return False
        self.editing=False
        self.notice.set_text(self.tr('saved') if op=='schedule' else '')
        self.last_state=state
        self.previous_cycle=state.get('cycle')
        self.show_page('editor' if op=='cancel' else 'dashboard')
        if op=='cancel':
            self.load_config(dict(sleep_time='23:00',wake_time='07:00',now=False,days=[]))
        self.poll()
        return False

    def poll(self):
        try:
            state=client.status()
            previous=self.last_state
            self.last_state=state
            status=state.get('status','idle')
            page=self.stack.get_visible_child_name()
            if not self.editing and page not in ('settings','logs'):
                if state.get('config') and status!='idle':
                    self.show_page('dashboard')
                elif previous.get('config') and not state.get('config'):
                    self.load_config(dict(sleep_time='23:00',wake_time='07:00',now=False,days=[]))
                    self.show_page('editor')
                    self.notice.set_text(self.tr('reset') if state.get('last_resume') else '')
            key={'scheduled':'active','sleeping':'sleeping','paused':'paused','conflict':'conflict','error':'error'}.get(status,'ready')
            self.pill.set_text('●  '+self.tr(key))
            if status in ('idle','paused','error','conflict'):
                css(self.pill,'idle-pill')
            else:
                self.pill.get_style_context().remove_class('idle-pill')
            self.engine_label.set_text(self.tr('engine')+' · '+self.tr('running' if state.get('engine_running') else 'stopped'))
            alarm=state.get('actual_alarm')
            self.rtc_label.set_text(self.tr('rtc')+' · '+(self.date_time(alarm) if alarm else self.tr('none')))
            if state.get('config'):
                sleep,wake=state.get('next_sleep'),state.get('next_wake')
                remaining=max(0,(sleep or time.time())-time.time())
                is_now=state['config'].get('now')
                self.remaining_box.set_visible(not is_now)
                self.detail['sleep'].set_text(self.tr('now') if is_now else self.date_time(sleep))
                self.detail['wake'].set_text(self.date_time(wake))
                self.detail['duration'].set_text(duration((wake or 0)-(sleep or 0),self.lang))
                self.detail['remaining'].set_text('—' if status!='scheduled' or is_now else duration(remaining,self.lang,True))
                self.countdown.set_text(duration(remaining,self.lang,True) if status=='scheduled' else '—')
                self.main_status.set_text(self.tr('scheduled' if status=='scheduled' else key))
                verified=state.get('rtc_readable') and alarm==state.get('expected_alarm') and alarm is not None
                self.verified.set_text(('✓ '+self.tr('verified')) if verified else self.tr('paused_hint' if status=='paused' else 'conflict' if status=='conflict' else 'error'))
                self.verified.set_max_width_chars(25); self.verified.set_line_wrap(True)
                self.pause_button.set_label(self.tr('resume') if status=='paused' else self.tr('pause'))
                self.pause_button.set_sensitive(not self.busy and status!='sleeping')
                self.remove_button.set_sensitive(not self.busy and status!='sleeping')
                a=state.get('actor') or {}
                self.author_label.set_text(self.tr('author')+'\n'+('AI Agent · ' if a.get('kind')=='agent' else '')+a.get('name','User'))
                self.next_action.set_text(self.tr('next_action')+' · '+(self.tr('sleep_at')+' '+self.date_time(sleep) if status=='scheduled' else self.tr(key)))
                total=max(1,(sleep or 0)-(state.get('scheduled_at') or state.get('updated',sleep or 0)))
                self.ring.progress=max(0,1-min(1,remaining/total)) if status=='scheduled' else 0
                self.ring.queue_draw()
            if state.get('message') and status in ('conflict','error') and not self.busy:
                self.notice.set_text(self.tr('conflict_hint') if status=='conflict' else state['message'])
            elif self.preview:
                self.notice.set_text(self.tr('preview'))
            elif page=='dashboard' and not self.busy and status not in ('conflict','error'):
                self.notice.set_text(self.tr('idle_note'))
        except Exception as exc:
            self.notice.set_text(str(exc))
        return True


def main():
    provider=Gtk.CssProvider()
    provider.load_from_path(str(Path(__file__).with_name('style.css')))
    Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(),provider,Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
    Gtk.Settings.get_default().set_property('gtk-application-prefer-dark-theme',True)
    window=Window(preview=not Path(client.HELPER).exists())
    window.show_all()
    window.editor_changed()
    Gtk.main()


if __name__=='__main__':
    main()
