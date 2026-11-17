use android_activity::AndroidApp;
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, RichText, Vec2};
use jni::{
    objects::{JObject, JString, JValue},
    JNIEnv, JavaVM,
};
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

fn activity() -> Result<AndroidApp> {
    ACTIVITY
        .get_or_init(Default::default)
        .lock()
        .unwrap()
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
            .unwrap()
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
    Paired(Session),
    Status(Value),
    Logs(Value),
    Error(String),
}
struct Mobile {
    node: Option<Arc<TailNode>>,
    session: Option<Session>,
    state: Value,
    network: Value,
    tx: Sender<Message>,
    rx: Receiver<Message>,
    busy: bool,
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
    if demo && cfg!(debug_assertions) && session.ip == std::net::Ipv4Addr::new(10, 0, 2, 2) {
        let mut stream = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(session.ip.into(), session.port),
            Duration::from_secs(5),
        )?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        exchange(&mut stream, r)
    } else {
        let mut stream = node
            .context("Connect to the private network first")?
            .dial(&format!("{}:{}", session.ip, session.port))?;
        exchange(&mut stream, r)
    }
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
            && session
                .as_ref()
                .is_some_and(|s| s.ip == std::net::Ipv4Addr::new(10, 0, 2, 2));
        let mut app = Self {
            node: None,
            session,
            state: json!({}),
            network: json!({}),
            tx,
            rx,
            busy: false,
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
        app.start_network();
        app
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
                let node = TailNode::new(&library, &dir.join("tailnet"), "rtc-sleeper-android")?;
                let _ = node.login();
                Ok(node)
            })();
            let _ = tx.send(match result {
                Ok(node) => Message::Node(node),
                Err(e) => Message::Error(format!("{e:#}")),
            });
        });
    }
    fn network_status(&mut self) {
        if self.busy {
            return;
        }
        if let Some(node) = self.node.clone() {
            self.busy = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let result = node.status();
                let _ = tx.send(match result {
                    Ok(s) => Message::Network(s),
                    Err(e) => Message::Error(e.to_string()),
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
        self.demo = cfg!(debug_assertions) && ticket.ip == std::net::Ipv4Addr::new(10, 0, 2, 2);
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
            let result = call(node, &temp, &r, demo).and_then(|v| Ok(serde_json::from_value(v)?));
            let _ = tx.send(match result {
                Ok(session) => Message::Paired(session),
                Err(e) => Message::Error(e.to_string()),
            });
        });
    }
    fn command(&mut self, op: &str, payload: Value) {
        if self.busy {
            return;
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        let r = session.request(op, payload);
        let node = self.node.clone();
        let tx = self.tx.clone();
        let demo = self.demo;
        let op = op.to_string();
        self.busy = true;
        std::thread::spawn(move || {
            let result = call(node, &session, &r, demo);
            let _ = tx.send(match result {
                Ok(v) if op == "logs" => Message::Logs(v),
                Ok(v) => Message::Status(v),
                Err(e) => Message::Error(e.to_string()),
            });
        });
    }
    fn configuration(&self) -> Value {
        let time = |h, m, pm| format!("{:02}:{:02}", h % 12 + if pm { 12 } else { 0 }, m);
        json!({"sleep_time":time(self.sh,self.sm,self.sp),"wake_time":time(self.wh,self.wm,self.wp),"now":self.now,"days":if self.now{Vec::new()}else{self.days.iter().enumerate().filter_map(|(i,on)|on.then_some(i)).collect::<Vec<_>>()}})
    }
    fn messages(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            self.busy = false;
            match message {
                Message::Node(node) => {
                    self.node = Some(node);
                    self.error.clear();
                }
                Message::Network(v) => {
                    self.network = v;
                }
                Message::Paired(session) => {
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
                Message::Status(v) => {
                    self.state = v;
                    self.last_ok = Some(Instant::now());
                    self.error.clear();
                }
                Message::Logs(v) => {
                    self.logs = serde_json::to_string_pretty(&v).unwrap_or_default();
                    self.last_ok = Some(Instant::now());
                }
                Message::Error(e) => {
                    self.error = e;
                }
            }
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
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for code in inputs {
            if let Some(code) = code.strip_prefix("demo:") {
                if cfg!(debug_assertions) {
                    self.demo = true;
                    self.phrase = code.into();
                }
            } else {
                self.phrase = code;
            }
        }
        if !self.busy && self.last_poll.elapsed() > Duration::from_secs(3) {
            self.last_poll = Instant::now();
            if self.session.is_some() {
                self.command("status", json!({}));
            } else {
                self.network_status();
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK).inner_margin(20)).show(ctx,|ui|{egui::ScrollArea::vertical().show(ui,|ui|{
            ui.add_space(22.);ui.heading(RichText::new("RTC Sleeper").size(30.));ui.label("Private remote control · Rust");ui.separator();
            if self.demo{ui.colored_label(Color32::YELLOW,"TEST MODE — simulated laptop only");}
            if self.busy{ui.horizontal(|ui|{ui.spinner();ui.label("Connecting…");});}
            if !self.error.is_empty(){ui.colored_label(Color32::from_rgb(255,150,150),&self.error);}
            if self.session.is_none(){
                ui.heading("1. Private network");let running=self.network["BackendState"]=="Running";ui.label(if running{"Connected to your private network"}else{"Sign in once to your Tailscale account"});
                if let Some(url)=self.network["AuthURL"].as_str().filter(|s|s.starts_with("https://login.tailscale.com/")){if ui.button("Sign in to Tailscale").clicked(){let _=java_string("openBrowser",Some(url));}}
                if self.node.is_none()&&!self.busy&&ui.button("Connect private network").clicked(){self.start_network();}
                ui.separator();ui.heading("2. Pair your laptop");ui.label("On your laptop: Settings > Remote Control > New pairing code.");
                if ui.button("Scan QR Code").clicked(){if let Err(e)=java_string("scanQr",None){self.error=e.to_string();}}
                if ui.button("Enter Pairing Phrase").clicked(){if let Err(e)=java_string("enterPhrase",None){self.error=e.to_string();}}
                ui.label("Or enter the 24-word pairing phrase:");ui.add(egui::TextEdit::multiline(&mut self.phrase).hint_text("Paste pairing phrase or scanned QR link").desired_rows(5).desired_width(f32::INFINITY));
                if ui.add_enabled(!self.busy,egui::Button::new("Pair Laptop").min_size(Vec2::new(ui.available_width(),46.))).clicked(){self.pair();}
                ui.label("Pairing codes stay valid until regenerated on the laptop. The laptop controls access for each phone.");
            }else{
                let online=self.last_ok.map(|t|t.elapsed()<Duration::from_secs(15)).unwrap_or(false);let status=self.state["status"].as_str().unwrap_or("Connecting").to_string();ui.heading(&status);ui.label(if online{"Laptop online"}else{"Laptop offline or sleeping — existing RTC wake still applies"});
                if let Some(next)=self.state["next_sleep"].as_u64(){let seconds=next.saturating_sub(now());ui.label(format!("Will sleep in: {} h {} min {} s",seconds/3600,seconds/60%60,seconds%60));}
                if let Some(message)=self.state["message"].as_str().filter(|s|!s.is_empty()){ui.label(message);}
                if ui.add_enabled(!self.busy,egui::Button::new("Refresh status")).clicked(){self.command("status",json!({}));}
                ui.separator();toggle(ui,&mut self.now,"Sleep Now");ui.add_enabled_ui(!self.now,|ui|{time_fields(ui,"Sleep at",&mut self.sh,&mut self.sm,&mut self.sp);});time_fields(ui,"Wake up at",&mut self.wh,&mut self.wm,&mut self.wp);
                ui.add_enabled_ui(!self.now,|ui|{ui.label("Repeat on sleep-start days");egui::Grid::new("repeat_days").num_columns(2).spacing([20.,12.]).show(ui,|ui|{for(i,day)in["Mon","Tue","Wed","Thu","Fri","Sat","Sun"].iter().enumerate(){toggle(ui,&mut self.days[i],day);if i%2==1{ui.end_row();}}});});
                ui.add_enabled_ui(online&&!self.busy,|ui|{
                    if ui.add(egui::Button::new("Apply Schedule").min_size(Vec2::new(ui.available_width(),48.))).clicked(){let p=self.configuration();if let Err(e)=validate_operation("schedule",&p){self.error=e.to_string();}else{self.confirm=Some(("schedule".into(),p));}}
                    ui.horizontal_wrapped(|ui|{for(op,label)in[(if status=="paused"{"resume"}else{"pause"},if status=="paused"{"Resume"}else{"Pause"}),("cancel","Remove"),("logs","Logs")]{if ui.button(label).clicked(){if op=="logs"{self.command(op,json!({"limit":50}));}else{self.confirm=Some((op.into(),json!({})));}}}});
                });
                if ui.button("Forget this laptop").clicked(){let _=java_string("storeSession",Some(""));self.session=None;self.state=json!({});self.last_ok=None;}
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
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
#[no_mangle]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Warn),
    );
    *ACTIVITY.get_or_init(Default::default).lock().unwrap() = Some(app.clone());
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
