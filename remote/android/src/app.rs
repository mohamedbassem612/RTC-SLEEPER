use android_activity::AndroidApp;
use anyhow::{ensure, Context, Result};
use eframe::egui::{self, Color32, RichText, Vec2};
use jni::{
    objects::{JObject, JString, JValue},
    JNIEnv, JavaVM,
};
use rtc_remote_core::cache::PhoneCache;
use rtc_remote_core::presence::{dur, presence, schedule_label};
use rtc_remote_core::{tailnet::TailNode, *};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

static INPUT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static ACTIVITY: OnceLock<Mutex<Option<AndroidApp>>> = OnceLock::new();
#[cfg(debug_assertions)]
static NETWORK_TEST_STATE: OnceLock<Mutex<String>> = OnceLock::new();
#[cfg(debug_assertions)]
static CONNECTION_TEST_STATE: OnceLock<Mutex<String>> = OnceLock::new();
#[cfg(debug_assertions)]
static TEST_NODE: OnceLock<Mutex<Option<Arc<TailNode>>>> = OnceLock::new();

#[cfg(debug_assertions)]
#[no_mangle]
pub extern "system" fn Java_com_mohamedbassem_rtcsleeper_MainActivity_nativeConnectionTestState(
    env: JNIEnv,
    _: JObject,
) -> jni::sys::jstring {
    let state = CONNECTION_TEST_STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    env.new_string(state.as_str()).unwrap().into_raw()
}

#[cfg(debug_assertions)]
#[no_mangle]
pub extern "system" fn Java_com_mohamedbassem_rtcsleeper_MainActivity_nativeDialDeadlineTest(
    env: JNIEnv,
    _: JObject,
) -> jni::sys::jstring {
    let node = TEST_NODE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let started = Instant::now();
    let error = node
        .context("No test node")
        .and_then(|n| n.dial_timeout("100.127.255.254:48187", Duration::from_millis(200)))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    env.new_string(json!({"elapsed_ms":started.elapsed().as_millis(),"error":error}).to_string())
        .unwrap()
        .into_raw()
}

#[cfg(debug_assertions)]
#[no_mangle]
pub extern "system" fn Java_com_mohamedbassem_rtcsleeper_MainActivity_nativeNetworkTestState(
    env: JNIEnv,
    _: JObject,
) -> jni::sys::jstring {
    let state = NETWORK_TEST_STATE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    env.new_string(state.as_str())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn activity() -> Result<AndroidApp> {
    ACTIVITY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .context("Activity unavailable")
}

#[no_mangle]
pub extern "system" fn Java_com_mohamedbassem_rtcsleeper_MainActivity_nativeScan(
    mut env: JNIEnv,
    _: JObject,
    value: JString,
) {
    if let Ok(s) = env.get_string(&value) {
        INPUT
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(s.into());
    }
}

fn java_string(method: &str, arg: Option<&str>) -> Result<String> {
    let app = activity()?;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = if let Some(arg) = arg {
        let s = env.new_string(arg)?;
        env.call_method(
            &activity,
            method,
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[JValue::Object(&s)],
        )
    } else {
        env.call_method(&activity, method, "()Ljava/lang/String;", &[])
    };
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = env.exception_clear();
            return Err(error.into());
        }
    };
    let obj = value.l()?;
    if obj.is_null() {
        return Ok(String::new());
    }
    let string = JString::from(obj);
    let result = env.get_string(&string)?.into();
    Ok(result)
}
enum Message {
    Node(Arc<TailNode>),
    Network(Value),
    /// Result of a user-started sign in / sign out.
    NetworkOp(Value),
    NetworkInfo(Value),
    Paired(Session),
    /// value, request sequence number, started by the background poller
    Status(Value, u64, bool),
    Logs(Value),
    /// text, transient network hiccup, started by the background poller
    Error(String, bool, bool),
}
/// Typed check instead of matching error text.
fn transient(e: &anyhow::Error) -> bool {
    use std::io::ErrorKind::*;
    e.chain().any(|cause| {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return matches!(
                io.kind(),
                TimedOut | WouldBlock | ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof | Interrupted
            ) || io.raw_os_error() == Some(11);
        }
        if let Some(j) = cause.downcast_ref::<serde_json::Error>() {
            return j.is_eof();
        }
        false
    }) || {
        // The embedded Go network layer reports its own timeouts as plain text only.
        let text = e.to_string();
        text.contains("deadline") || text.contains("timed out")
    }
}
struct Mobile {
    node: Option<Arc<TailNode>>,
    session: Option<Session>,
    state: Value,
    network: Value,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    /// A user-started action (pairing, apply, pause...) is running. Shows the spinner.
    busy: bool,
    /// A silent background refresh is running. Never changes the layout or disables controls.
    polling: bool,
    seq: u64,
    applied: u64,
    error_from_poll: bool,
    seen_resume: Option<u64>,
    banner: Option<(String, Instant)>,
    settings_open: bool,
    confirm_signout: bool,
    open_when_ready: bool,
    /// Unix time of the newest laptop data held here (live or restored from the saved copy).
    data_epoch: Option<u64>,
    /// True while the numbers on screen came only from the saved copy.
    from_saved: bool,
    cache_dirty: bool,
    last_saved: Instant,
    last_form: Value,
    phrase: String,
    error: String,
    logs: String,
    now: bool,
    sh: i32,
    sm: i32,
    sp: bool,
    wh: i32,
    wm: i32,
    wp: bool,
    days: [bool; 7],
    confirm: Option<(String, Value)>,
    last_poll: Instant,
    last_ok: Option<Instant>,
    demo: bool,
}
fn call(node: Option<Arc<TailNode>>, session: &Session, r: &Value, demo: bool) -> Result<Value> {
    if demo
        && cfg!(debug_assertions)
        && (session.ip.is_loopback() || session.ip == std::net::Ipv4Addr::new(10, 0, 2, 2))
    {
        let mut stream = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(session.ip.into(), session.port),
            Duration::from_secs(5),
        )?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        exchange(&mut stream, r)
    } else {
        let node = node.context("Connect to the private network first")?;
        let mut stream = node.dial(&format!("{}:{}", session.ip, session.port))?;
        exchange(&mut stream, r)
    }
}
fn prepare_network(node: &Option<Arc<TailNode>>, demo: bool, tx: &Sender<Message>) -> Result<()> {
    if demo && cfg!(debug_assertions) {
        return Ok(());
    }
    let node = node
        .as_ref()
        .context("Private network is still starting. Try again shortly.")?;
    node.update_android_network(&java_string("networkSnapshot", None)?)?;
    let status = node.status()?;
    let state = status["BackendState"]
        .as_str()
        .unwrap_or("Starting")
        .to_string();
    let _ = tx.send(Message::NetworkInfo(status));
    ensure!(
        state == "Running",
        "Private network: {state}. Reconnect or sign in below."
    );
    Ok(())
}
impl Mobile {
    fn new(ctx: &egui::Context) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::BLACK;
        visuals.window_fill = Color32::BLACK;
        visuals.override_text_color = Some(Color32::from_gray(240));
        visuals.selection.bg_fill = Color32::from_gray(100);
        ctx.set_visuals(visuals);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(10., 14.);
        style.spacing.button_padding = Vec2::new(18., 12.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(17.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(17.));
        ctx.set_style(style);
        let (tx, rx) = mpsc::channel();
        let session = java_string("loadSession", None)
            .ok()
            .and_then(|s| serde_json::from_str::<Session>(&s).ok());
        let demo = cfg!(debug_assertions)
            && session.as_ref().is_some_and(|s| {
                s.ip.is_loopback() || s.ip == std::net::Ipv4Addr::new(10, 0, 2, 2)
            });
        let mut app = Self {
            node: None,
            session,
            state: json!({}),
            network: json!({}),
            tx,
            rx,
            busy: false,
            polling: false,
            seq: 0,
            applied: 0,
            error_from_poll: false,
            seen_resume: None,
            banner: None,
            settings_open: false,
            confirm_signout: false,
            open_when_ready: false,
            data_epoch: None,
            from_saved: false,
            cache_dirty: false,
            last_saved: Instant::now(),
            last_form: json!(null),
            phrase: String::new(),
            error: String::new(),
            logs: String::new(),
            now: false,
            sh: 11,
            sm: 0,
            sp: true,
            wh: 7,
            wm: 0,
            wp: false,
            days: [false; 7],
            confirm: None,
            last_poll: Instant::now(),
            last_ok: None,
            demo,
        };
        if app.session.is_some() {
            app.load_cache();
        }
        app.last_form = app.form_value();
        app.start_network();
        app
    }
    fn cache_path() -> Option<PathBuf> {
        activity().ok()?.internal_data_path().map(|d| d.join("phone-cache.json"))
    }
    fn form_value(&self) -> Value {
        json!({"sh":self.sh,"sm":self.sm,"sp":self.sp,"wh":self.wh,"wm":self.wm,"wp":self.wp,
               "now":self.now,"days":self.days})
    }
    fn load_cache(&mut self) {
        let Some(c) = Self::cache_path().and_then(|p| PhoneCache::read(&p).ok().flatten()) else {
            return;
        };
        let f = &c.form;
        let int = |k: &str, lo: i64, hi: i64, d: i32| {
            f[k].as_i64().map_or(d, |v| v.clamp(lo, hi) as i32)
        };
        self.sh = int("sh", 1, 12, self.sh);
        self.sm = int("sm", 0, 59, self.sm);
        self.wh = int("wh", 1, 12, self.wh);
        self.wm = int("wm", 0, 59, self.wm);
        self.sp = f["sp"].as_bool().unwrap_or(self.sp);
        self.wp = f["wp"].as_bool().unwrap_or(self.wp);
        self.now = f["now"].as_bool().unwrap_or(false);
        if let Some(days) = f["days"].as_array() {
            for (i, d) in days.iter().take(7).enumerate() {
                self.days[i] = d.as_bool().unwrap_or(false);
            }
        }
        if c.state.is_object() && c.saved > 0 {
            self.seen_resume = c.state["last_resume"].as_u64();
            self.state = c.state;
            self.data_epoch = Some(c.saved);
            self.from_saved = true;
        }
        self.logs = c.logs;
    }
    fn save_cache(&mut self) {
        self.cache_dirty = false;
        self.last_saved = Instant::now();
        if self.session.is_none() {
            return;
        }
        if let Some(path) = Self::cache_path() {
            let _ = PhoneCache {
                saved: self.data_epoch.unwrap_or(0),
                form: self.form_value(),
                state: self.state.clone(),
                logs: self.logs.clone(),
            }
            .write(&path);
        }
    }
    fn clear_saved(&mut self) {
        if let Some(path) = Self::cache_path() {
            PhoneCache::clear(&path);
        }
        self.state = json!({});
        self.logs.clear();
        self.data_epoch = None;
        self.from_saved = false;
        self.seen_resume = None;
        self.cache_dirty = false;
    }
    fn forget_laptop(&mut self) {
        let _ = java_string("storeSession", Some(""));
        self.clear_saved();
        self.session = None;
        self.last_ok = None;
    }
    fn apply_network(&mut self, v: Value) {
        if self.open_when_ready {
            if let Some(url) = v["AuthURL"].as_str().filter(|u| u.starts_with("https://login.tailscale.com/")) {
                self.open_when_ready = false;
                let _ = java_string("openBrowser", Some(url));
            }
        }
        self.network = v;
    }
    /// Sign in or out of Tailscale on this phone (runs off the UI thread).
    fn network_account(&mut self, sign_out: bool) {
        let Some(node) = self.node.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        self.error.clear();
        self.open_when_ready = !sign_out;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                if sign_out {
                    node.logout()?;
                } else {
                    node.login()?;
                }
                std::thread::sleep(Duration::from_millis(600));
                node.status()
            })();
            let _ = tx.send(match result {
                Ok(v) => Message::NetworkOp(v),
                Err(e) => Message::Error(e.to_string(), transient(&e), false),
            });
        });
    }
    fn start_network(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result: Result<Arc<TailNode>> = (|| {
                let app = activity()?;
                let dir = app
                    .internal_data_path()
                    .context("Private app storage unavailable")?;
                let library =
                    PathBuf::from(java_string("libraryDirectory", None)?).join("libtailscale.so");
                let snapshot = java_string("networkSnapshot", None)?;
                let node = TailNode::new_android(
                    &library,
                    &dir.join("tailnet"),
                    "rtc-sleeper-android",
                    &snapshot,
                )?;
                // tsnet restores saved credentials and starts login only if needed.
                Ok(node)
            })();
            let _ = tx.send(match result {
                Ok(node) => Message::Node(node),
                Err(e) => Message::Error(format!("{e:#}"), transient(&e), false),
            });
        });
    }
    fn network_status(&mut self) {
        if self.busy || self.polling {
            return;
        }
        if let Some(node) = self.node.clone() {
            self.polling = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = java_string("networkSnapshot", None)
                    .and_then(|snapshot| node.update_android_network(&snapshot))
                    .and_then(|_| node.status());
                let _ = tx.send(match result {
                    Ok(s) => Message::Network(s),
                    Err(e) => Message::Error(e.to_string(), transient(&e), true),
                });
            });
        }
    }
    fn pair(&mut self) {
        if self.busy {
            return;
        }
        let ticket = match PairTicket::parse(&self.phrase, cfg!(debug_assertions)) {
            Ok(t) => t,
            Err(e) => {
                self.error = e.to_string();
                return;
            }
        };
        self.demo = cfg!(debug_assertions)
            && (ticket.ip.is_loopback() || ticket.ip == std::net::Ipv4Addr::new(10, 0, 2, 2));
        let temp = Session {
            token: String::new(),
            device: "Android".into(),
            ip: ticket.ip,
            port: ticket.port,
        };
        let name = java_string("deviceName", None).unwrap_or_else(|_| "Android phone".into());
        let r = json!({"op":"pair","code":self.phrase,"device":name});
        let node = self.node.clone();
        let demo = self.demo;
        let tx = self.tx.clone();
        self.busy = true;
        std::thread::spawn(move || {
            let result = prepare_network(&node, demo, &tx)
                .and_then(|_| call(node, &temp, &r, demo))
                .and_then(|v| Ok(serde_json::from_value(v)?));
            let _ = tx.send(match result {
                Ok(session) => Message::Paired(session),
                Err(e) => Message::Error(e.to_string(), transient(&e), false),
            });
        });
    }
    /// User-started command: always allowed, even while a silent refresh is in flight.
    fn command(&mut self, op: &str, payload: Value) {
        self.dispatch(op, payload, false);
    }
    /// Silent refresh: skipped while anything else is running.
    fn poll(&mut self) {
        self.dispatch("status", json!({}), true);
    }
    fn dispatch(&mut self, op: &str, payload: Value, background: bool) {
        if self.busy || (background && self.polling) {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        self.seq += 1;
        let seq = self.seq;
        let r = session.request(op, payload);
        let node = self.node.clone();
        let tx = self.tx.clone();
        let demo = self.demo;
        let op = op.to_string();
        if background {
            self.polling = true;
        } else {
            self.busy = true;
            self.error.clear();
            self.error_from_poll = false;
        }
        std::thread::spawn(move || {
            let result =
                prepare_network(&node, demo, &tx).and_then(|_| call(node, &session, &r, demo));
            let _ = tx.send(match result {
                Ok(v) if op == "logs" => Message::Logs(v),
                Ok(v) => Message::Status(v, seq, background),
                Err(e) => Message::Error(e.to_string(), transient(&e), background),
            });
        });
    }
    fn configuration(&self) -> Value {
        let time = |h, m, pm| format!("{:02}:{:02}", h % 12 + if pm { 12 } else { 0 }, m);
        json!({"sleep_time":time(self.sh,self.sm,self.sp),"wake_time":time(self.wh,self.wm,self.wp),"now":self.now,"days":if self.now{Vec::new()}else{self.days.iter().enumerate().filter_map(|(i,on)|on.then_some(i)).collect::<Vec<_>>()}})
    }
    fn messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            if let Message::NetworkInfo(v) = message {
                self.network = v;
                continue;
            }
            self.last_poll = Instant::now();
            match message {
                Message::Node(node) => {
                    self.busy = false;
                    #[cfg(debug_assertions)]
                    {
                        *TEST_NODE
                            .get_or_init(Default::default)
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(node.clone());
                    }
                    self.node = Some(node);
                    self.error.clear();
                    self.last_poll = Instant::now() - Duration::from_secs(5);
                }
                Message::Network(v) => {
                    self.polling = false;
                    #[cfg(debug_assertions)]
                    {
                        *NETWORK_TEST_STATE
                            .get_or_init(Default::default)
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) =
                            v["BackendState"].as_str().unwrap_or("").into();
                    }
                    self.apply_network(v);
                }
                Message::NetworkOp(v) => {
                    self.busy = false;
                    self.polling = false;
                    self.error.clear();
                    self.apply_network(v);
                }
                Message::Paired(session) => {
                    self.busy = false;
                    match serde_json::to_string(&session)
                        .ok()
                        .and_then(|s| java_string("storeSession", Some(&s)).ok())
                    {
                        Some(_) => {
                            self.session = Some(session);
                            self.phrase.clear();
                            self.error.clear();
                            self.last_poll = Instant::now() - Duration::from_secs(5);
                        }
                        None => self.error = "Cannot securely store this device pairing".into(),
                    }
                }
                Message::Status(v, seq, background) => {
                    if background {
                        self.polling = false;
                    } else {
                        self.busy = false;
                    }
                    // A refresh that was started before a later command must not overwrite
                    // that command's newer result.
                    if seq >= self.applied {
                        self.applied = seq;
                        let new_resume = v["last_resume"].as_u64();
                        let had_status = self.last_ok.is_some() || self.from_saved;
                        let was_lost = self
                            .last_ok
                            .is_some_and(|t| t.elapsed() >= Duration::from_secs(15));
                        if had_status {
                            if new_resume.is_some_and(|n| self.seen_resume.is_none_or(|o| n > o)) {
                                self.banner =
                                    Some(("Laptop woke up and is back online".into(), Instant::now()));
                            } else if was_lost {
                                self.banner =
                                    Some(("Laptop is back online".into(), Instant::now()));
                            }
                        }
                        if new_resume.is_some() {
                            self.seen_resume = new_resume;
                        }
                        self.state = v;
                        self.data_epoch = Some(now());
                        self.from_saved = false;
                        self.cache_dirty = true;
                        self.last_ok = Some(Instant::now());
                        if !background || self.error_from_poll {
                            self.error.clear();
                            self.error_from_poll = false;
                        }
                    }
                }
                Message::Logs(v) => {
                    self.busy = false;
                    self.logs = serde_json::to_string_pretty(&v).unwrap_or_default();
                    self.cache_dirty = true;
                    self.last_ok = Some(Instant::now());
                }
                Message::Error(e, is_transient, background) => {
                    if background {
                        self.polling = false;
                    } else {
                        self.busy = false;
                    }
                    #[cfg(debug_assertions)]
                    {
                        *NETWORK_TEST_STATE
                            .get_or_init(Default::default)
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner) = format!("error: {e}");
                    }
                    if background && is_transient {
                        // A passing hiccup during a silent refresh: keep the last known status.
                        // "Connection lost" appears by itself if it lasts.
                        continue;
                    }
                    self.error = if is_transient {
                        "Laptop connection timed out or was interrupted. Refresh status to retry. No action was automatically repeated.".into()
                    } else {
                        e
                    };
                    self.error_from_poll = background;
                }
                Message::NetworkInfo(_) => unreachable!(),
            }
        }
    }
}
impl Mobile {
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let red = Color32::from_rgb(255, 150, 150);
        ui.add_space(22.);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.settings_open = false;
                self.confirm_signout = false;
            }
            ui.heading("Settings");
        });
        ui.separator();
        if !self.error.is_empty() {
            ui.colored_label(red, &self.error);
        }
        ui.heading("Private network (Tailscale)");
        let state = self.network["BackendState"]
            .as_str()
            .unwrap_or(if self.node.is_none() { "Not started" } else { "Starting" })
            .to_string();
        let running = state == "Running";
        let uid = self.network["Self"]["UserID"].as_i64().unwrap_or(0).to_string();
        let who = self.network["User"][uid.as_str()]["LoginName"].as_str().filter(|s| !s.is_empty());
        ui.label(if running {
            match who {
                Some(w) => format!("Signed in as {w}"),
                None => "Signed in".to_string(),
            }
        } else {
            format!("Not signed in ({state})")
        });
        if self.busy || self.polling {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working…");
            });
        }
        if let Some(url) = self.network["AuthURL"]
            .as_str()
            .filter(|u| u.starts_with("https://login.tailscale.com/"))
            .map(str::to_string)
        {
            if !running && ui.button("Finish signing in in the browser").clicked() {
                let _ = java_string("openBrowser", Some(&url));
            }
        }
        let idle = !self.busy && !self.polling;
        if running {
            if self.confirm_signout {
                ui.label("Sign out of Tailscale? You will need to sign in again before you can control the laptop.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_signout = false;
                    }
                    if ui.add_enabled(idle, egui::Button::new("Sign out")).clicked() {
                        self.confirm_signout = false;
                        self.network_account(true);
                    }
                });
            } else if ui.add_enabled(idle, egui::Button::new("Sign out")).clicked() {
                self.confirm_signout = true;
            }
        } else if self.node.is_none() {
            if !self.busy && ui.button("Connect private network").clicked() {
                self.start_network();
            }
        } else if ui.add_enabled(idle, egui::Button::new("Sign in")).clicked() {
            self.network_account(false);
        }
        ui.separator();
        ui.heading("Paired laptop");
        if let Some(s) = &self.session {
            ui.label(format!("Device ID: {}", device_id(s)));
            if ui.button("Forget this laptop").clicked() {
                self.forget_laptop();
                self.settings_open = false;
            }
        } else {
            ui.label("No laptop paired.");
        }
        ui.separator();
        ui.heading("Saved on this phone");
        ui.label("The schedule fields, the last laptop status and the logs are kept on this phone so they stay visible while the laptop sleeps. The laptop holds the real schedule. Tailscale only carries the connection and stores nothing.");
        if let Some(at) = self.data_epoch {
            ui.label(format!("Laptop data saved {} ago.", dur(now().saturating_sub(at))));
        }
        if ui.button("Clear saved data").clicked() {
            self.clear_saved();
        }
    }
}
fn toggle(ui: &mut egui::Ui, value: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        let (size, response) = ui.allocate_exact_size(Vec2::new(54., 30.), egui::Sense::click());
        if response.clicked() {
            *value = !*value;
        }
        let t = ui.ctx().animate_bool(response.id, *value);
        let color = if *value {
            Color32::from_gray(230)
        } else {
            Color32::from_gray(45)
        };
        ui.painter().rect_filled(size, 15., color);
        let center = egui::pos2(
            egui::lerp((size.left() + 15.)..=(size.right() - 15.), t),
            size.center().y,
        );
        ui.painter().circle_filled(
            center,
            11.,
            if *value {
                Color32::BLACK
            } else {
                Color32::WHITE
            },
        );
    });
}
fn time_fields(ui: &mut egui::Ui, name: &str, h: &mut i32, m: &mut i32, pm: &mut bool) {
    ui.label(RichText::new(name).strong());
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(h)
                .range(1..=12)
                .speed(0.15)
                .prefix("Hour "),
        );
        ui.label(":");
        ui.add(
            egui::DragValue::new(m)
                .range(0..=59)
                .speed(0.2)
                .prefix("Min "),
        );
        if ui.button(if *pm { "PM" } else { "AM" }).clicked() {
            *pm = !*pm;
        }
    });
}
impl eframe::App for Mobile {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.messages();
        let inputs = INPUT
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        for code in inputs {
            if let Some(saved) = code.strip_prefix("session:") {
                if cfg!(debug_assertions) {
                    if let Ok(session) = serde_json::from_str::<Session>(saved) {
                        if session.ip.is_loopback()
                            || session.ip == std::net::Ipv4Addr::new(10, 0, 2, 2)
                        {
                            self.demo = true;
                            self.session = Some(session);
                            self.last_poll = Instant::now() - Duration::from_secs(5);
                        }
                    }
                }
            } else if let Some(code) = code.strip_prefix("demo:") {
                if cfg!(debug_assertions) {
                    self.demo = true;
                    self.phrase = code.into();
                }
            } else {
                self.phrase = code;
            }
        }
        // Silent refresh: never while an action is running or a confirmation is open.
        let form = self.form_value();
        if form != self.last_form {
            self.last_form = form;
            self.cache_dirty = true;
        }
        if self.cache_dirty && self.last_saved.elapsed() > Duration::from_secs(2) {
            self.save_cache();
        }
        let reachable = self.last_ok.is_some_and(|t| t.elapsed() < Duration::from_secs(15));
        if !self.busy
            && !self.polling
            && self.confirm.is_none()
            && self.last_poll.elapsed() > Duration::from_secs(if reachable { 5 } else { 3 })
        {
            self.last_poll = Instant::now();
            if self.session.is_some() {
                self.poll();
            } else {
                self.network_status();
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK).inner_margin(20)).show(ctx,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{
            if self.settings_open{self.settings_ui(ui);return;}
            ui.add_space(22.);ui.horizontal(|ui|{ui.heading(RichText::new("RTC Sleeper").size(30.));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.button("Settings").clicked(){self.settings_open=true;}});});ui.label("Private remote control · Rust");ui.separator();
            if self.demo{ui.colored_label(Color32::YELLOW,"TEST MODE — simulated laptop only");}
            if self.busy{ui.horizontal(|ui|{ui.spinner();ui.label(if self.node.is_none(){"Starting private network…"}else{"Refreshing connection…"});});}
            if !self.error.is_empty(){ui.colored_label(Color32::from_rgb(255,150,150),&self.error);}
            if self.session.is_some() && !self.demo {
                let network_state=self.network["BackendState"].as_str().unwrap_or("Starting");
                if network_state!="Running" {
                    ui.label(format!("Private network: {network_state}"));
                    if ui.button("Open Settings to sign in").clicked(){self.settings_open=true;}
                    if self.node.is_none() && !self.busy && ui.button("Reconnect private network").clicked(){self.start_network();}
                }
            }
            if self.session.is_none(){
                ui.heading("1. Private network");let running=self.network["BackendState"]=="Running";ui.label(if running{"Connected to your private network"}else{"Sign in once to your Tailscale account"});
                if !running&&ui.button("Open Settings to sign in").clicked(){self.settings_open=true;}
                if self.node.is_none()&&!self.busy&&ui.button("Connect private network").clicked(){self.start_network();}
                ui.separator();ui.heading("2. Pair your laptop");ui.label("On your laptop: Settings > Remote Control > New pairing code.");
                if ui.button("Scan QR Code").clicked(){if let Err(e)=java_string("scanQr",None){self.error=e.to_string();}}
                if ui.button("Enter Pairing Phrase").clicked(){if let Err(e)=java_string("enterPhrase",None){self.error=e.to_string();}}
                ui.label("Or enter the 24-word pairing phrase:");ui.add(egui::TextEdit::multiline(&mut self.phrase).hint_text("Paste pairing phrase or scanned QR link").desired_rows(5).desired_width(f32::INFINITY));
                if ui.add_enabled(!self.busy,egui::Button::new("Pair Laptop").min_size(Vec2::new(ui.available_width(),46.))).clicked(){self.pair();}
                ui.label("A pairing code lasts 15 minutes and works once. After pairing, approve this phone on the laptop and check that the Device ID matches.");
            }else{
                let online=self.last_ok.map(|t|t.elapsed()<Duration::from_secs(15)).unwrap_or(false);let status=self.state["status"].as_str().unwrap_or("").to_string();
                let heard=self.last_ok.is_some()||self.from_saved;let (heading,detail)=presence(&self.state,online,heard,now());
                ui.heading(heading);
                if let Some(s)=&self.session{ui.small(format!("Device ID: {}",device_id(s)));}
                if !detail.is_empty(){ui.label(detail);}
                if let Some((text,at))=&self.banner{if at.elapsed()<Duration::from_secs(30){ui.colored_label(egui::Color32::from_rgb(80,200,120),text);}}
                if self.from_saved&&!online{if let Some(at)=self.data_epoch{ui.label(format!("Showing saved data from {} ago.",dur(now().saturating_sub(at))));}}
                if heard{ui.label(format!("Schedule: {}",schedule_label(&status)));}
                if let Some(w)=self.state["last_resume"].as_u64(){let mut t=format!("Last woke up: {} ago",dur(now().saturating_sub(w)));if let Some(sl)=self.state["last_sleep_at"].as_u64().filter(|sl|*sl<=w){t+=&format!(" (slept for {})",dur(w-sl));}ui.label(t);}
                if let Some(next)=self.state["next_sleep"].as_u64(){let seconds=next.saturating_sub(now());ui.label(format!("Will sleep in: {} h {} min {} s",seconds/3600,seconds/60%60,seconds%60));}
                if let Some(message)=self.state["message"].as_str().filter(|s|!s.is_empty()){ui.label(message);}
                if ui.add_enabled(!self.busy,egui::Button::new("Refresh status")).clicked(){self.command("status",json!({}));}
                ui.separator();toggle(ui,&mut self.now,"Sleep Now");ui.add_enabled_ui(!self.now,|ui|{time_fields(ui,"Sleep at",&mut self.sh,&mut self.sm,&mut self.sp);});time_fields(ui,"Wake up at",&mut self.wh,&mut self.wm,&mut self.wp);
                ui.add_enabled_ui(!self.now,|ui|{ui.label("Repeat on sleep-start days");egui::Grid::new("repeat_days").num_columns(2).spacing([20.,12.]).show(ui,|ui|{for(i,day)in["Mon","Tue","Wed","Thu","Fri","Sat","Sun"].iter().enumerate(){toggle(ui,&mut self.days[i],day);if i%2==1{ui.end_row();}}});});
                ui.add_enabled_ui(online&&!self.busy,|ui|{
                    if ui.add(egui::Button::new("Apply Schedule").min_size(Vec2::new(ui.available_width(),48.))).clicked(){let p=self.configuration();if let Err(e)=validate_operation("schedule",&p){self.error=e.to_string();}else{self.confirm=Some(("schedule".into(),p));}}
                    ui.horizontal_wrapped(|ui|{for(op,label)in[(if status=="paused"{"resume"}else{"pause"},if status=="paused"{"Resume"}else{"Pause"}),("cancel","Remove"),("logs","Logs")]{if ui.button(label).clicked(){if op=="logs"{self.command(op,json!({"limit":50}));}else{self.confirm=Some((op.into(),json!({})));}}}});
                });
                if !self.logs.is_empty(){egui::CollapsingHeader::new("Activity logs").default_open(true).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.logs).desired_rows(10).interactive(false));});}
            }
            ui.add_space(30.);
        });});
        if let Some((op, payload)) = self.confirm.clone() {
            egui::Window::new("Confirm remote action")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(if op == "schedule" && payload["now"] == true {
                        "This will put your laptop to sleep in about 5 seconds after verifying RTC."
                    } else {
                        "Apply this change to your laptop?"
                    });
                    ui.label(&op);
                    if op == "schedule" {
                        ui.label(format!(
                            "Sleep: {} · Wake: {}",
                            payload["sleep_time"], payload["wake_time"]
                        ));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.confirm = None;
                        }
                        if ui.button("Confirm").clicked() {
                            self.confirm = None;
                            self.command(&op, payload);
                        }
                    });
                });
        }
        #[cfg(debug_assertions)]
        {
            *CONNECTION_TEST_STATE
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                json!({"busy":self.busy,"status":self.state["status"],"error":self.error})
                    .to_string();
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
#[no_mangle]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Warn),
    );
    *ACTIVITY.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(app.clone());
    let options = eframe::NativeOptions {
        android_app: Some(app),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "RTC Sleeper",
        options,
        Box::new(|cc| Ok(Box::new(Mobile::new(&cc.egui_ctx)))),
    ) {
        log::error!("UI startup failed: {e}");
    }
}
