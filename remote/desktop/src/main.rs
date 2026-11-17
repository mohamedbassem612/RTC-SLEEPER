use anyhow::{bail, ensure, Context, Result};
use base64::Engine as _;
use rtc_remote_core::{tailnet::TailNode, *};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
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
                child.kill()?;
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
    std::fs::write(&temp, bytes)?;
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(temp, path)?;
    Ok(())
}
fn reply(mut stream: impl Read + Write, body: impl FnOnce(Value) -> Result<Value>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
    if CONNECTIONS.fetch_add(1, Ordering::SeqCst) >= 32 {
        CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    struct Permit;
    impl Drop for Permit {
        fn drop(&mut self) {
            CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let _permit = Permit;
    let result = (|| {
        let mut bytes = Vec::new();
        BufReader::new((&mut stream).take(16385)).read_until(b'\n', &mut bytes)?;
        ensure!(bytes.len() <= 16384, "Request too large");
        body(serde_json::from_slice(&bytes)?)
    })();
    let out = match result {
        Ok(v) => json!({"ok":true,"result":v}),
        Err(e) => json!({"ok":false,"error":e.to_string()}),
    };
    let _ = writeln!(stream, "{out}");
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
            ["status", "login", "pair", "regenerate", "revoke", "devices", "block", "allow", "enable", "disable"].contains(&op),
            "Use serve | status | login | pair | regenerate | revoke | devices | block ID | allow ID | enable | disable"
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
    let store = if store_path.exists() {
        serde_json::from_slice(&std::fs::read(&store_path)?)?
    } else {
        Store::default()
    };
    let server = Arc::new(Mutex::new(Server {
        store,
        backend: Laptop {
            mock,
            state: json!({"status":"idle","config":null,"actual_alarm":null,"engine_running":true}),
            commands: vec![],
        },
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
                let server = server.clone();
                let node = node.clone();
                let store_path = store_path.clone();
                std::thread::spawn(move || {
                    reply(stream, |r| {
                        let status = if let Some(node) = &node {
                            node.status()?
                        } else {
                            json!({"BackendState":"Running","TailscaleIPs":["10.0.2.2"],"demo":true})
                        };
                        match r["op"].as_str().unwrap_or("") {
                            "status" => Ok(status),
                            "devices" => {
                                let s = server.lock().unwrap();
                                Ok(
                                    json!({"enabled":!s.store.remote_disabled,"devices":s.store.sessions.iter().map(|v|json!({"id":device_id(v),"name":v.device,"allowed":!s.store.blocked.contains(&device_id(v))})).collect::<Vec<_>>()}),
                                )
                            }
                            "enable" | "disable" | "block" | "allow" => {
                                let mut s = server.lock().unwrap();
                                let op = r["op"].as_str().unwrap();
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
                                }
                                private_write(&store_path, &serde_json::to_vec(&s.store)?)?;
                                Ok(json!({"updated":true}))
                            }
                            "login" => {
                                node.as_ref().context("Mock does not need login")?.login()?;
                                Ok(json!({"message":"Login requested; refresh status"}))
                            }
                            "revoke" => {
                                let mut s = server.lock().unwrap();
                                s.store.sessions.clear();
                                s.store.pairing = None;
                                s.store.blocked.clear();
                                private_write(&store_path, &serde_json::to_vec(&s.store)?)?;
                                Ok(json!({"revoked":true}))
                            }
                            "pair" | "regenerate" => {
                                let ip: Ipv4Addr = status["TailscaleIPs"]
                                    .as_array()
                                    .and_then(|v| {
                                        v.iter()
                                            .filter_map(Value::as_str)
                                            .find_map(|s| s.parse::<Ipv4Addr>().ok())
                                    })
                                    .context("Sign in to Tailscale before pairing")?;
                                let mut s = server.lock().unwrap();
                                if r["op"] == "regenerate"
                                    || s.store.pairing.as_ref().is_none_or(|t| {
                                        t.ip != ip || t.port != port || t.expires != u64::MAX
                                    })
                                {
                                    s.store.pairing = Some(PairTicket::new(ip, port));
                                }
                                let ticket = s.store.pairing.as_ref().unwrap();
                                let out = json!({"uri":ticket.uri()?,"phrase":ticket.phrase()?,"expires":null,"qr_png":base64::engine::general_purpose::STANDARD.encode(ticket.qr_png()?)});
                                private_write(&store_path, &serde_json::to_vec(&s.store)?)?;
                                Ok(out)
                            }
                            _ => bail!("Unknown local operation"),
                        }
                    })
                });
            }
        });
    }
    let handle = |stream| {
        let server = server.clone();
        let store_path = store_path.clone();
        std::thread::spawn(move || {
            reply(stream, |r| {
                let mut s = server.lock().unwrap();
                let result = s.handle(&r);
                private_write(&store_path, &serde_json::to_vec(&s.store)?)?;
                result
            })
        });
    };
    if let Some(node) = node {
        let listener = node.listen(port)?;
        loop {
            match node.accept(listener) {
                Ok(stream) => handle(stream),
                Err(e) => {
                    eprintln!("{e}");
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    } else {
        for stream in TcpListener::bind(("127.0.0.1", port))?.incoming().flatten() {
            stream.set_read_timeout(Some(Duration::from_secs(10)))?;
            let server = server.clone();
            let store_path = store_path.clone();
            std::thread::spawn(move || {
                reply(stream, |r| {
                    let mut s = server.lock().unwrap();
                    let result = s.handle(&r);
                    private_write(&store_path, &serde_json::to_vec(&s.store)?)?;
                    result
                })
            });
        }
    }
    Ok(())
}
