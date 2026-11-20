use anyhow::{bail, ensure, Context, Result};
use base64::Engine as _;
use rtc_remote_core::{tailnet::TailNode, *};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

struct Laptop {
    mock: bool,
    state: Value,
    commands: Vec<String>,
}
impl Backend for Laptop {
    fn execute(&mut self, op: &str, p: &Value, device: &str) -> Result<Value> {
        validate_operation(op, p)?;
        if self.mock {
            self.commands.push(op.into());
            match op {
                "status" => {}
                "logs" => return Ok(json!(self.commands)),
                "schedule" => {
                    self.state = json!({"status":"scheduled","config":p,"actual_alarm":now()+7200,"expected_alarm":now()+7200,"next_sleep":now()+3600,"next_wake":now()+7200,"engine_running":true});
                }
                "pause" => self.state["status"] = json!("paused"),
                "resume" => self.state["status"] = json!("scheduled"),
                "cancel" => self.state = json!({"status":"idle","config":null,"actual_alarm":null}),
                _ => bail!("Unsupported"),
            };
            return Ok(self.state.clone());
        }
        let mut args = vec![
            "--actor".to_string(),
            "user".into(),
            "--agent-name".into(),
            format!("Android: {device}"),
            op.into(),
        ];
        if op == "schedule" {
            if p["now"] == true {
                args.push("--now".into());
            } else {
                args.extend([
                    "--sleep-at".into(),
                    p["sleep_time"].as_str().unwrap().into(),
                ]);
            }
            args.extend(["--wake-at".into(), p["wake_time"].as_str().unwrap().into()]);
            let names = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
            let days = p["days"].as_array().unwrap();
            if !days.is_empty() {
                args.extend([
                    "--repeat".into(),
                    days.iter()
                        .map(|d| names[d.as_u64().unwrap() as usize])
                        .collect::<Vec<_>>()
                        .join(","),
                ]);
            }
        } else if op == "logs" {
            args.extend([
                "--limit".into(),
                p["limit"].as_u64().unwrap_or(50).to_string(),
            ]);
        }
        let mut child = Command::new("/usr/local/bin/sleep-scheduler")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        // Drain both pipes while the child runs, including large log responses.
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            stdout.take(1_048_577).read_to_end(&mut out).map(|_| out)
        });
        let errors = std::thread::spawn(move || {
            let mut out = Vec::new();
            stderr.take(65536).read_to_end(&mut out).map(|_| out)
        });
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if Instant::now() > deadline {
                // Kill the whole group: sudo and the root helper are grandchildren.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                child.wait()?;
                bail!("Command timed out; refresh status before another action");
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        child.wait()?;
        let output = reader
            .join()
            .map_err(|_| anyhow::anyhow!("Output reader failed"))??;
        let _ = errors.join();
        ensure!(
            output.len() <= 1_048_576,
            "Log response too large; choose a smaller limit"
        );
        let reply: Value = serde_json::from_slice(&output)?;
        ensure!(
            reply["ok"] == true,
            "{}",
            reply["error"]
                .as_str()
                .unwrap_or("Scheduler rejected the command")
        );
        Ok(reply["result"].clone())
    }
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&temp, path)?;
    if let Some(dir) = path.parent() {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}
/// A panic in one handler must not make every later request fail.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn persist(path: &Path, store: &Store, before: &[u8]) -> Result<()> {
    let after = serde_json::to_vec(store)?;
    if after != before {
        private_write(path, &after)?;
    }
    Ok(())
}
/// Reads one request line with a hard size limit and a hard total deadline,
/// so a peer that trickles bytes cannot hold a connection slot.
fn read_request(stream: &mut impl Read) -> Result<Vec<u8>> {
    const LIMIT: usize = 16384;
    const TOTAL: Duration = Duration::from_secs(5);
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if let Some(i) = bytes.iter().position(|b| *b == b'\n') {
            bytes.truncate(i);
            break;
        }
        ensure!(bytes.len() <= LIMIT, "Request too large");
        ensure!(start.elapsed() < TOTAL, "Request timed out");
    }
    ensure!(bytes.len() <= LIMIT, "Request too large");
    Ok(bytes)
}
// Separate budgets: a flood from the network can never lock out the local CLI/GUI.
static REMOTE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
static LOCAL_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
fn reply(
    mut stream: impl Read + Write,
    counter: &'static AtomicUsize,
    cap: usize,
    body: impl FnOnce(Value) -> Result<Value>,
) {
    if counter.fetch_add(1, Ordering::SeqCst) >= cap {
        counter.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    struct Permit(&'static AtomicUsize);
    impl Drop for Permit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let _permit = Permit(counter);
    let result = (|| {
        let bytes = read_request(&mut stream)?;
        body(serde_json::from_slice(&bytes)?)
    })();
    let out = match result {
        Ok(v) => json!({"ok":true,"result":v}),
        Err(e) => json!({"ok":false,"error":e.to_string()}),
    };
    let _ = writeln!(stream, "{out}");
}
/// Placeholder: commands run through `Laptop` outside the server lock.
struct Unused;
impl Backend for Unused {
    fn execute(&mut self, _: &str, _: &Value, _: &str) -> Result<Value> {
        bail!("Commands are executed outside the server lock")
    }
}
fn serve_remote(
    server: &Mutex<Server<Unused>>,
    laptop: &Mutex<Laptop>,
    store_path: &Path,
    r: Value,
) -> Result<Value> {
    // Authorization and pairing hold the lock only briefly.
    let authorized = {
        let mut s = lock(server);
        let before = serde_json::to_vec(&s.store)?;
        let result = s.authorize(&r);
        persist(store_path, &s.store, &before)?;
        result?
    };
    // The slow part (up to 45 s) runs without the server lock, so blocking a
    // device, disabling access or revoking works even while a command hangs.
    match authorized {
        Authorized::Done(v) => Ok(v),
        Authorized::Run {
            op,
            payload,
            device,
        } => lock(laptop).execute(&op, &payload, &device),
    }
}
fn root() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".config"))
        .join("rtc-sleeper-remote")
}
fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let dir = root();
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let socket = dir.join("control.sock");
    if args.first().map(String::as_str) != Some("serve") {
        let op = args.first().map(String::as_str).unwrap_or("status");
        ensure!(
            ["status", "login", "pair", "regenerate", "revoke", "devices", "block", "allow", "remove", "enable", "disable"].contains(&op),
            "Use serve | status | login | pair | regenerate | revoke | devices | block ID | allow ID | remove ID | enable | disable"
        );
        let mut stream = UnixStream::connect(&socket)
            .context("Remote service is not running. Start rtc-sleeper-remote serve")?;
        stream.set_read_timeout(Some(Duration::from_secs(8)))?;
        let out = exchange(&mut stream, &json!({"op":op,"id":args.get(1)}))?;
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    let mock = args.iter().any(|s| s == "--mock");
    let port = if mock {
        std::env::var("RTC_MOCK_PORT")
            .ok()
            .map(|v| v.parse::<u16>())
            .transpose()?
            .unwrap_or(48188)
    } else {
        PORT
    };
    let store_path = dir.join(if mock {
        "mock-sessions.json"
    } else {
        "sessions.json"
    });
    let mut store = if store_path.exists() {
        match std::fs::read(&store_path)
            .map_err(anyhow::Error::from)
            .and_then(|b| Ok(serde_json::from_slice::<Store>(&b)?))
        {
            Ok(store) => store,
            Err(e) => {
                // Do not crash-loop on a damaged file: keep it aside and start clean.
                let backup = store_path.with_extension(format!("corrupt-{}", now()));
                eprintln!("Stored pairings unreadable ({e}); moved to {backup:?}. Pair phones again.");
                std::fs::rename(&store_path, &backup)?;
                Store::default()
            }
        }
    } else {
        Store::default()
    };
    // Pairing codes from older versions never expired. Drop them.
    if store.pairing.as_ref().is_some_and(|t| t.expires > now() + PAIR_TTL + 60) {
        store.pairing = None;
    }
    let server = Arc::new(Mutex::new(Server {
        store,
        backend: Unused,
        allow_demo: mock,
    }));
    let laptop = Arc::new(Mutex::new(Laptop {
        mock,
        state: json!({"status":"idle","config":null,"actual_alarm":null,"engine_running":true}),
        commands: vec![],
    }));
    let node = if mock {
        None
    } else {
        let library = std::env::current_exe()?
            .parent()
            .unwrap()
            .join("libtailscale.so");
        Some(TailNode::new(
            &library,
            &dir.join("tailnet"),
            "rtc-sleeper-laptop",
        )?)
    };
    if socket.exists() {
        ensure!(
            UnixStream::connect(&socket).is_err(),
            "Remote service already running"
        );
        std::fs::remove_file(&socket)?;
    }
    let local = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    {
        let server = server.clone();
        let node = node.clone();
        let store_path = store_path.clone();
        std::thread::spawn(move || {
            for stream in local.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let server = server.clone();
                let node = node.clone();
                let store_path = store_path.clone();
                std::thread::spawn(move || {
                    reply(stream, &LOCAL_CONNECTIONS, 8, |r| {
                        let op = r["op"].as_str().unwrap_or("").to_string();
                        let status = || -> Result<Value> {
                            Ok(if let Some(node) = &node {
                                node.status()?
                            } else {
                                json!({"BackendState":"Running","TailscaleIPs":["10.0.2.2"],"demo":true})
                            })
                        };
                        match op.as_str() {
                            "status" => status(),
                            "devices" => {
                                let s = lock(&server);
                                Ok(
                                    json!({"enabled":!s.store.remote_disabled,"devices":s.store.sessions.iter().map(|v|json!({"id":device_id(v),"name":v.device,"allowed":!s.store.blocked.contains(&device_id(v))})).collect::<Vec<_>>()}),
                                )
                            }
                            "enable" | "disable" | "block" | "allow" | "remove" => {
                                let mut s = lock(&server);
                                let before = serde_json::to_vec(&s.store)?;
                                if op == "enable" || op == "disable" {
                                    s.store.remote_disabled = op == "disable";
                                } else {
                                    let id = r["id"].as_str().context("Provide a device ID")?;
                                    ensure!(
                                        s.store.sessions.iter().any(|v| device_id(v) == id),
                                        "Unknown device"
                                    );
                                    s.store.blocked.retain(|v| v != id);
                                    if op == "block" {
                                        s.store.blocked.push(id.into());
                                    }
                                    if op == "remove" {
                                        s.store.sessions.retain(|v| device_id(v) != id);
                                    }
                                }
                                persist(&store_path, &s.store, &before)?;
                                Ok(json!({"updated":true}))
                            }
                            "login" => {
                                node.as_ref().context("Mock does not need login")?.login()?;
                                Ok(json!({"message":"Login requested; refresh status"}))
                            }
                            "revoke" => {
                                let mut s = lock(&server);
                                let before = serde_json::to_vec(&s.store)?;
                                s.store.sessions.clear();
                                s.store.pairing = None;
                                s.store.blocked.clear();
                                persist(&store_path, &s.store, &before)?;
                                Ok(json!({"revoked":true}))
                            }
                            "pair" | "regenerate" => {
                                let status = status()?;
                                let ip: Ipv4Addr = status["TailscaleIPs"]
                                    .as_array()
                                    .and_then(|v| {
                                        v.iter()
                                            .filter_map(Value::as_str)
                                            .find_map(|s| s.parse::<Ipv4Addr>().ok())
                                    })
                                    .context("Sign in to Tailscale before pairing")?;
                                let mut s = lock(&server);
                                let before = serde_json::to_vec(&s.store)?;
                                let t = now();
                                if op == "regenerate"
                                    || s.store.pairing.as_ref().is_none_or(|p| {
                                        p.ip != ip || p.port != port || p.expires <= t + 30
                                    })
                                {
                                    s.store.pairing = Some(PairTicket::new(ip, port));
                                }
                                let ticket = s.store.pairing.as_ref().unwrap();
                                let out = json!({"uri":ticket.uri()?,"phrase":ticket.phrase()?,"expires":ticket.expires,"qr_png":base64::engine::general_purpose::STANDARD.encode(ticket.qr_png()?)});
                                persist(&store_path, &s.store, &before)?;
                                Ok(out)
                            }
                            _ => bail!("Unknown local operation"),
                        }
                    })
                });
            }
        });
    }
    fn handle<S: Read + Write + Send + 'static>(
        stream: S,
        server: &Arc<Mutex<Server<Unused>>>,
        laptop: &Arc<Mutex<Laptop>>,
        store_path: &Path,
    ) {
        let (server, laptop, store_path) = (server.clone(), laptop.clone(), store_path.to_path_buf());
        std::thread::spawn(move || {
            reply(stream, &REMOTE_CONNECTIONS, 32, |r| {
                serve_remote(&server, &laptop, &store_path, r)
            })
        });
    }
    if let Some(node) = node {
        let listener = node.listen(port)?;
        loop {
            match node.accept(listener) {
                Ok(stream) => handle(stream, &server, &laptop, &store_path),
                Err(e) => {
                    eprintln!("{e}");
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    } else {
        for stream in TcpListener::bind(("127.0.0.1", port))?.incoming().flatten() {
            stream.set_read_timeout(Some(Duration::from_secs(10)))?;
            handle(stream, &server, &laptop, &store_path);
        }
    }
    Ok(())
}
