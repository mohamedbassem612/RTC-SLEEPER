pub mod presence;
pub mod tailnet;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    net::Ipv4Addr,
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

pub const PORT: u16 = 48187;
/// A pairing code is valid for this many seconds and works once.
pub const PAIR_TTL: u64 = 900;
pub const MAX_DEVICES: usize = 16;
pub const MAX_PENDING: usize = 4;
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn random_hex(bytes: usize) -> String {
    let mut b = vec![0; bytes];
    OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
pub fn same(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}
pub fn tail_ip(ip: Ipv4Addr) -> bool {
    let a = ip.octets();
    a[0] == 100 && (64..=127).contains(&a[1])
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairTicket {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub secret: String,
    pub expires: u64,
}
impl PairTicket {
    pub fn new(ip: Ipv4Addr, port: u16) -> Self {
        Self {
            ip,
            port,
            secret: random_hex(16),
            expires: now() + PAIR_TTL,
        }
    }
    fn bytes(&self) -> Result<Vec<u8>> {
        let mut b = vec![1];
        b.extend(self.ip.octets());
        b.extend(self.port.to_be_bytes());
        let secret = hex::decode(&self.secret)?;
        ensure!(secret.len() == 16, "Invalid pairing secret");
        b.extend(secret);
        b.extend(self.expires.to_be_bytes());
        let digest = Sha256::digest(&b);
        b.extend(&digest[..2]);
        Ok(b)
    }
    pub fn uri(&self) -> Result<String> {
        Ok(format!(
            "rtcsleeper://pair/{}",
            URL_SAFE_NO_PAD.encode(self.bytes()?)
        ))
    }
    pub fn phrase(&self) -> Result<String> {
        let b = self.bytes()?;
        let words = bip39::Language::English.word_list();
        Ok((0..24)
            .map(|i| {
                let mut n = 0usize;
                for bit in i * 11..(i + 1) * 11 {
                    n = (n << 1) | ((b[bit / 8] >> (7 - bit % 8)) & 1) as usize;
                }
                words[n]
            })
            .collect::<Vec<_>>()
            .join(" "))
    }
    pub fn parse(code: &str, allow_demo: bool) -> Result<Self> {
        let b = if let Some(s) = code.trim().strip_prefix("rtcsleeper://pair/") {
            URL_SAFE_NO_PAD.decode(s)?
        } else {
            let list = bip39::Language::English.word_list();
            let s = code.to_lowercase();
            let words = s.split_whitespace().collect::<Vec<_>>();
            ensure!(words.len() == 24, "Enter all 24 pairing words");
            let mut b = vec![0u8; 33];
            for (i, w) in words.iter().enumerate() {
                let n = list
                    .binary_search(w)
                    .map_err(|_| anyhow::anyhow!("Unknown pairing word"))?;
                for j in 0..11 {
                    let bit = i * 11 + j;
                    b[bit / 8] |= (((n >> (10 - j)) & 1) as u8) << (7 - bit % 8);
                }
            }
            b
        };
        ensure!(b.len() == 33 && b[0] == 1, "Unsupported pairing code");
        ensure!(
            Sha256::digest(&b[..31])[..2] == b[31..],
            "Pairing code checksum failed"
        );
        let ip = Ipv4Addr::new(b[1], b[2], b[3], b[4]);
        let port = u16::from_be_bytes([b[5], b[6]]);
        ensure!(
            tail_ip(ip) || (allow_demo && (ip.is_loopback() || ip == Ipv4Addr::new(10, 0, 2, 2))),
            "Code must point to a private Tailscale device"
        );
        let expires = u64::from_be_bytes(b[23..31].try_into()?);
        ensure!(expires > now(), "Pairing code expired; generate a new one");
        ensure!(port > 0, "Invalid port");
        Ok(Self {
            ip,
            port,
            secret: hex::encode(&b[7..23]),
            expires,
        })
    }
    pub fn qr_png(&self) -> Result<Vec<u8>> {
        let qr = qrcode::QrCode::new(self.uri()?.as_bytes())?;
        let img = qr
            .render::<image::Luma<u8>>()
            .min_dimensions(320, 320)
            .build();
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img).write_to(&mut out, image::ImageFormat::Png)?;
        Ok(out.into_inner())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub device: String,
    pub ip: Ipv4Addr,
    pub port: u16,
}
impl Session {
    pub fn request(&self, op: &str, payload: Value) -> Value {
        json!({"op":op,"payload":payload,"token":self.token,"at":now(),"id":random_hex(16)})
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub sessions: Vec<Session>,
    /// Replay window. Memory only: never written to disk.
    #[serde(skip)]
    pub seen: Vec<(String, u64)>,
    #[serde(default)]
    pub pairing: Option<PairTicket>,
    #[serde(default)]
    pub blocked: Vec<String>,
    #[serde(default)]
    pub remote_disabled: bool,
}
pub fn device_id(session: &Session) -> String {
    hex::encode(Sha256::digest(session.token.as_bytes()))[..16].to_string()
}
pub trait Backend: Send {
    fn execute(&mut self, op: &str, payload: &Value, device: &str) -> Result<Value>;
}
pub struct Server<B: Backend> {
    pub store: Store,
    pub backend: B,
    /// Accept loopback / emulator pairing tickets (mock mode only).
    pub allow_demo: bool,
}
pub enum Authorized {
    /// Fully handled while the caller holds the server lock (pairing).
    Done(Value),
    /// Authorized command. Run it WITHOUT holding the server lock.
    Run {
        op: String,
        payload: Value,
        device: String,
    },
}
impl<B: Backend> Server<B> {
    pub fn handle(&mut self, r: &Value) -> Result<Value> {
        match self.authorize(r)? {
            Authorized::Done(v) => Ok(v),
            Authorized::Run {
                op,
                payload,
                device,
            } => self.backend.execute(&op, &payload, &device),
        }
    }
    pub fn authorize(&mut self, r: &Value) -> Result<Authorized> {
        ensure!(
            !self.store.remote_disabled,
            "Remote access is disabled on the laptop"
        );
        let op = r["op"].as_str().context("Missing operation")?;
        if op == "pair" {
            let supplied = PairTicket::parse(
                r["code"].as_str().context("Missing pairing code")?,
                self.allow_demo,
            )?;
            let ticket = self
                .store
                .pairing
                .clone()
                .context("No active pairing code; generate a new one on the laptop")?;
            let t = now();
            ensure!(
                ticket.expires > t
                    && ticket.expires <= t + PAIR_TTL + 60
                    && supplied.ip == ticket.ip
                    && supplied.port == ticket.port
                    && supplied.expires == ticket.expires
                    && same(&supplied.secret, &ticket.secret),
                "Pairing code rejected or expired; generate a new one"
            );
            let device = r["device"].as_str().context("Missing device name")?;
            ensure!(
                !device.is_empty() && device.len() <= 80 && !device.chars().any(char::is_control),
                "Invalid device name"
            );
            // A phone that pairs again replaces its own earlier unapproved entry.
            // Approved devices are never evicted by a new pairing.
            let waiting = self.store.blocked.clone();
            self.store
                .sessions
                .retain(|s| !(s.device == device && waiting.contains(&device_id(s))));
            let live = self
                .store
                .sessions
                .iter()
                .map(device_id)
                .collect::<HashSet<_>>();
            self.store.blocked.retain(|id| live.contains(id));
            let pending = self
                .store
                .sessions
                .iter()
                .filter(|s| self.store.blocked.contains(&device_id(s)))
                .count();
            ensure!(
                pending < MAX_PENDING,
                "Too many devices are waiting for approval; remove them on the laptop"
            );
            ensure!(
                self.store.sessions.len() < MAX_DEVICES,
                "Device limit reached; remove unused devices on the laptop"
            );
            let session = Session {
                token: random_hex(32),
                device: device.into(),
                ip: ticket.ip,
                port: ticket.port,
            };
            self.store.blocked.push(device_id(&session));
            self.store.sessions.push(session.clone());
            // Single use: the code is burned as soon as it pairs a phone.
            self.store.pairing = None;
            return Ok(Authorized::Done(serde_json::to_value(session)?));
        }
        let token = r["token"].as_str().context("Not paired")?;
        let session = self
            .store
            .sessions
            .iter()
            .find(|s| same(&s.token, token))
            .context("Device not authorized")?
            .clone();
        ensure!(
            !self.store.blocked.contains(&device_id(&session)),
            "Waiting for laptop approval, or device access is blocked. Use Settings > Remote Control > Allow selected on the laptop."
        );
        let at = r["at"].as_u64().context("Missing command timestamp")?;
        ensure!(
            now().abs_diff(at) <= 30,
            "Command expired; refresh and retry explicitly"
        );
        let id = r["id"].as_str().context("Missing request id")?;
        ensure!(
            id.len() == 32 && hex::decode(id).is_ok(),
            "Invalid request id"
        );
        self.store.seen.retain(|(_, at)| now().abs_diff(*at) <= 60);
        ensure!(
            !self.store.seen.iter().any(|(seen, _)| seen == id),
            "Duplicate request blocked"
        );
        validate_operation(op, &r["payload"])?;
        self.store.seen.push((id.into(), at));
        Ok(Authorized::Run {
            op: op.into(),
            payload: r["payload"].clone(),
            device: session.device,
        })
    }
}
pub fn validate_operation(op: &str, p: &Value) -> Result<()> {
    ensure!(p.is_object(), "Payload must be an object");
    match op {
        "status" | "pause" | "cancel" => {
            ensure!(p.as_object().unwrap().is_empty(), "Unexpected fields")
        }
        "logs" => {
            let o = p.as_object().unwrap();
            ensure!(o.keys().all(|k| k == "limit"), "Unexpected log fields");
            if let Some(v) = o.get("limit") {
                let n = v.as_u64().context("Invalid log limit")?;
                ensure!((1..=2000).contains(&n), "Invalid log limit");
            }
        }
        "resume" => ensure!(
            p.as_object().unwrap().is_empty(),
            "Unexpected resume fields"
        ),
        "schedule" => {
            let fields = p
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            ensure!(
                fields == HashSet::from(["sleep_time", "wake_time", "now", "days"]),
                "Invalid schedule fields"
            );
            for k in ["sleep_time", "wake_time"] {
                let s = p[k].as_str().context("Invalid time")?;
                ensure!(
                    s.len() == 5
                        && s.as_bytes()[2] == b':'
                        && s.bytes()
                            .enumerate()
                            .all(|(i, b)| i == 2 || b.is_ascii_digit()),
                    "Use HH:MM time"
                );
                ensure!(
                    s[..2].parse::<u8>()? < 24 && s[3..].parse::<u8>()? < 60,
                    "Invalid hour or minute"
                );
            }
            ensure!(p["now"].is_boolean(), "Invalid Sleep Now value");
            let days = p["days"].as_array().context("Invalid repeat days")?;
            let mut seen = HashSet::new();
            for day in days {
                let d = day.as_u64().context("Invalid weekday")?;
                ensure!(d < 7 && seen.insert(d), "Invalid repeat weekday");
            }
            ensure!(
                !p["now"].as_bool().unwrap() || days.is_empty(),
                "Sleep Now cannot repeat"
            );
            ensure!(
                p["now"] == true || p["sleep_time"] != p["wake_time"],
                "Sleep and wake times must differ"
            );
        }
        _ => bail!("Unsupported operation"),
    }
    Ok(())
}
pub fn exchange<S: Read + Write>(stream: &mut S, r: &Value) -> Result<Value> {
    stream.write_all(&serde_json::to_vec(r)?)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut bytes = Vec::new();
    BufReader::new(stream.take(1_048_577)).read_until(b'\n', &mut bytes)?;
    ensure!(bytes.len() <= 1_048_576, "Response too large");
    let reply: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        reply["ok"] == true,
        "{}",
        reply["error"].as_str().unwrap_or("Remote request failed")
    );
    Ok(reply["result"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        writes: usize,
    }
    impl Backend for Fake {
        fn execute(&mut self, op: &str, _: &Value, _: &str) -> Result<Value> {
            self.writes += 1;
            Ok(json!({"status":op}))
        }
    }
    fn server() -> Server<Fake> {
        Server {
            store: Store {
                pairing: Some(PairTicket::new("100.80.1.2".parse().unwrap(), PORT)),
                ..Store::default()
            },
            backend: Fake { writes: 0 },
            allow_demo: false,
        }
    }
    #[test]
    fn qr_and_phrase_same_ticket() {
        let t = server().store.pairing.unwrap();
        for code in [t.uri().unwrap(), t.phrase().unwrap()] {
            let p = PairTicket::parse(&code, false).unwrap();
            assert_eq!(p.secret, t.secret);
            assert_eq!(p.ip, t.ip);
        }
        assert!(t.qr_png().unwrap().starts_with(b"\x89PNG"));
        let pixels = image::load_from_memory(&t.qr_png().unwrap())
            .unwrap()
            .to_luma8();
        let mut scanner = quircs::Quirc::default();
        let code = scanner
            .identify(
                pixels.width() as usize,
                pixels.height() as usize,
                pixels.as_raw(),
            )
            .next()
            .unwrap()
            .unwrap();
        let decoded = String::from_utf8(code.decode().unwrap().payload).unwrap();
        assert_eq!(PairTicket::parse(&decoded, false).unwrap().secret, t.secret);
    }
    #[test]
    fn checksum_and_external_ip_rejected() {
        let mut t = server().store.pairing.unwrap();
        t.ip = "8.8.8.8".parse().unwrap();
        assert!(PairTicket::parse(&t.uri().unwrap(), false).is_err());
        assert!(PairTicket::parse("hello world", false).is_err());
        t.expires = now() - 1;
        assert!(PairTicket::parse(&t.uri().unwrap(), true).is_err());
    }
    #[test]
    fn single_use_pairing_access_control_and_replay() {
        let mut s = server();
        let code = s.store.pairing.as_ref().unwrap().phrase().unwrap();
        let req = json!({"op":"pair","code":code,"device":"Android test"});
        let session: Session = serde_json::from_value(s.handle(&req).unwrap()).unwrap();
        assert!(s.handle(&req).is_err(), "code must be single use");
        assert!(s.handle(&session.request("status", json!({}))).is_err());
        s.store.blocked.clear();
        s.store.remote_disabled = true;
        assert!(s.handle(&session.request("status", json!({}))).is_err());
        s.store.remote_disabled = false;
        let req = session.request("status", json!({}));
        assert!(s.handle(&req).is_ok());
        assert!(s.handle(&req).is_err());
        let mut stale = session.request("pause", json!({}));
        stale["at"] = json!(now() - 60);
        assert!(s.handle(&stale).is_err());
        assert_eq!(s.backend.writes, 1);
        let mut bad = session.request("cancel", json!({}));
        bad["token"] = json!("bad");
        assert!(s.handle(&bad).is_err());
    }
    fn pair_request(s: &Server<Fake>, name: &str) -> Value {
        let code = s.store.pairing.as_ref().unwrap().phrase().unwrap();
        json!({"op":"pair","code":code,"device":name})
    }
    #[test]
    fn expired_and_legacy_codes_rejected() {
        let mut s = server();
        let mut t = s.store.pairing.clone().unwrap();
        t.expires = now() - 1;
        s.store.pairing = Some(t.clone());
        assert!(s.handle(&pair_request(&s, "Phone")).is_err());
        t.expires = u64::MAX; // legacy never-expiring ticket from an old store
        s.store.pairing = Some(t);
        assert!(s.handle(&pair_request(&s, "Phone")).is_err());
        assert!(s.store.sessions.is_empty());
    }
    #[test]
    fn pending_devices_capped_and_deduplicated() {
        let mut s = server();
        let mut first = None;
        for i in 0..MAX_PENDING {
            s.store.pairing = Some(PairTicket::new("100.80.1.2".parse().unwrap(), PORT));
            let v = s.handle(&pair_request(&s, &format!("Phone {i}"))).unwrap();
            first.get_or_insert(v);
        }
        s.store.pairing = Some(PairTicket::new("100.80.1.2".parse().unwrap(), PORT));
        assert!(s.handle(&pair_request(&s, "One too many")).is_err());
        // Same name pairing again replaces its own unapproved entry.
        assert!(s.handle(&pair_request(&s, "Phone 0")).is_ok());
        assert_eq!(s.store.sessions.len(), MAX_PENDING);
        assert_eq!(s.store.blocked.len(), MAX_PENDING);
    }
    #[test]
    fn approved_device_not_evicted_by_same_name() {
        let mut s = server();
        let v = s.handle(&pair_request(&s, "Pixel")).unwrap();
        let session: Session = serde_json::from_value(v).unwrap();
        s.store.blocked.clear();
        s.store.pairing = Some(PairTicket::new("100.80.1.2".parse().unwrap(), PORT));
        s.handle(&pair_request(&s, "Pixel")).unwrap();
        assert!(s.store.sessions.iter().any(|x| x.token == session.token));
        assert_eq!(s.store.sessions.len(), 2);
    }
    #[test]
    fn authorize_never_runs_backend() {
        let mut s = server();
        let v = s.handle(&pair_request(&s, "Pixel")).unwrap();
        let session: Session = serde_json::from_value(v).unwrap();
        s.store.blocked.clear();
        let a = s.authorize(&session.request("status", json!({}))).unwrap();
        assert!(matches!(a, Authorized::Run { .. }));
        assert_eq!(s.backend.writes, 0);
    }
    #[test]
    fn validate_inputs_and_whitelist() {
        for p in [
            json!({"sleep_time":"73:00","wake_time":"07:00","now":false,"days":[]}),
            json!({"sleep_time":"23:00","wake_time":"07:00","now":true,"days":[1]}),
        ] {
            assert!(validate_operation("schedule", &p).is_err());
        }
        assert!(validate_operation("shutdown", &json!({})).is_err());
        assert!(validate_operation("resume", &json!({"replace_alarm":true})).is_err());
        assert!(validate_operation("logs", &json!({"limit":"5"})).is_err());
        assert!(validate_operation("logs", &json!({"limit":0})).is_err());
        assert!(validate_operation("logs", &json!({})).is_ok());
    }
}
