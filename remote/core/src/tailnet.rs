//! Embedded official libtailscale (Go/tsnet), dynamically linked behind a Rust API.
use anyhow::{bail, ensure, Context, Result};
use libloading::Library;
use serde_json::Value;
use std::{
    ffi::{CStr, CString},
    io::{Read, Write},
    os::{
        fd::{FromRawFd, RawFd},
        raw::{c_char, c_int},
    },
    path::Path,
    sync::Arc,
    time::Duration,
};

pub struct TailNode {
    lib: Arc<Library>,
    id: c_int,
}
impl TailNode {
    pub fn new(library: &Path, state: &Path, name: &str) -> Result<Arc<Self>> {
        Self::create(library, state, name, None)
    }
    pub fn new_android(
        library: &Path,
        state: &Path,
        name: &str,
        network: &str,
    ) -> Result<Arc<Self>> {
        Self::create(library, state, name, Some(network))
    }
    fn create(
        library: &Path,
        state: &Path,
        name: &str,
        network: Option<&str>,
    ) -> Result<Arc<Self>> {
        std::fs::create_dir_all(state)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700))?;
        }
        // A Go c-shared runtime must stay mapped until the process exits.
        let loaded = unsafe {
            libloading::os::unix::Library::open(Some(library), libc::RTLD_NOW | libc::RTLD_NODELETE)
        }
        .context("Embedded Tailscale library is missing")?;
        let lib = Arc::new(Library::from(loaded));
        let directory = CString::new(state.to_string_lossy().as_bytes())?;
        let configured = unsafe {
            lib.get::<unsafe extern "C" fn(*const c_char) -> c_int>(b"RtcStateDir\0")?(
                directory.as_ptr(),
            )
        };
        ensure!(
            configured == 0,
            "Cannot initialize private Tailscale storage"
        );
        let id = unsafe { lib.get::<unsafe extern "C" fn() -> c_int>(b"tailscale_new\0")?() };
        let node = Arc::new(Self { lib, id });
        if let Some(snapshot) = network {
            node.update_android_network(snapshot)?;
        }
        unsafe {
            node.lib.get::<unsafe extern "C" fn(c_int)>(b"RtcQuiet\0")?(id);
        }
        node.set("tailscale_set_dir", &state.to_string_lossy())?;
        node.set("tailscale_set_hostname", name)?;
        unsafe {
            node.lib
                .get::<unsafe extern "C" fn(c_int, c_int) -> c_int>(b"tailscale_set_logfd\0")?(
                id, -1,
            );
        }
        let rc = unsafe {
            node.lib
                .get::<unsafe extern "C" fn(c_int) -> c_int>(b"tailscale_start\0")?(id)
        };
        node.check(rc)?;
        Ok(node)
    }
    pub fn update_android_network(&self, snapshot: &str) -> Result<()> {
        let snapshot = CString::new(snapshot)?;
        let rc = unsafe {
            self.lib
                .get::<unsafe extern "C" fn(c_int, *const c_char) -> c_int>(
                    b"RtcAndroidNetwork\0",
                )?(self.id, snapshot.as_ptr())
        };
        ensure!(rc == 0, "Cannot read Android network interfaces");
        Ok(())
    }
    fn check(&self, rc: c_int) -> Result<()> {
        if rc != 0 {
            let mut buf = [0 as c_char; 2048];
            unsafe {
                self.lib
                    .get::<unsafe extern "C" fn(c_int, *mut c_char, usize) -> c_int>(
                        b"tailscale_errmsg\0",
                    )?(self.id, buf.as_mut_ptr(), buf.len());
                bail!(
                    "Tailscale: {}",
                    CStr::from_ptr(buf.as_ptr()).to_string_lossy()
                );
            }
        }
        Ok(())
    }
    fn set(&self, name: &str, value: &str) -> Result<()> {
        let name = CString::new(name)?;
        let value = CString::new(value)?;
        let rc = unsafe {
            self.lib
                .get::<unsafe extern "C" fn(c_int, *const c_char) -> c_int>(
                    name.as_bytes_with_nul(),
                )?(self.id, value.as_ptr())
        };
        self.check(rc)
    }
    pub fn status(&self) -> Result<Value> {
        let mut out = std::ptr::null_mut();
        let rc = unsafe {
            self.lib
                .get::<unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int>(
                    b"tailscale_status_json\0",
                )?(self.id, &mut out)
        };
        self.check(rc)?;
        ensure!(!out.is_null(), "Empty Tailscale status");
        let result = unsafe {
            let bytes = CStr::from_ptr(out).to_bytes();
            let value = serde_json::from_slice(bytes);
            libc::free(out.cast());
            value
        };
        Ok(result?)
    }
    pub fn login(&self) -> Result<()> {
        use base64::Engine;
        let (mut addr, mut proxy, mut credential) =
            ([0 as c_char; 128], [0 as c_char; 33], [0 as c_char; 33]);
        let rc = unsafe {
            self.lib.get::<unsafe extern "C" fn(
                c_int,
                *mut c_char,
                usize,
                *mut c_char,
                *mut c_char,
            ) -> c_int>(b"tailscale_loopback\0")?(
                self.id,
                addr.as_mut_ptr(),
                addr.len(),
                proxy.as_mut_ptr(),
                credential.as_mut_ptr(),
            )
        };
        self.check(rc)?;
        let addr = unsafe { CStr::from_ptr(addr.as_ptr()) }.to_str()?;
        let credential = unsafe { CStr::from_ptr(credential.as_ptr()) }.to_str()?;
        let auth = base64::engine::general_purpose::STANDARD.encode(format!(":{credential}"));
        let mut stream = std::net::TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        write!(stream,"POST /localapi/v0/login-interactive HTTP/1.0\r\nHost: {addr}\r\nAuthorization: Basic {auth}\r\nSec-Tailscale: localapi\r\nContent-Length: 0\r\n\r\n")?;
        let mut response = String::new();
        stream.take(65536).read_to_string(&mut response)?;
        ensure!(
            response.starts_with("HTTP/1.0 2") || response.starts_with("HTTP/1.1 2"),
            "Tailscale login could not start: {}",
            response.lines().next().unwrap_or("empty response")
        );
        Ok(())
    }
    pub fn dial(&self, address: &str) -> Result<std::os::unix::net::UnixStream> {
        let addr = CString::new(address)?;
        let mut fd = -1;
        let rc = unsafe {
            self.lib.get::<unsafe extern "C" fn(
                c_int,
                *const c_char,
                *const c_char,
                *mut c_int,
            ) -> c_int>(b"tailscale_dial\0")?(
                self.id, c"tcp".as_ptr(), addr.as_ptr(), &mut fd
            )
        };
        self.check(rc)?;
        let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        stream.set_write_timeout(Some(Duration::from_secs(15)))?;
        Ok(stream)
    }
    pub fn listen(&self, port: u16) -> Result<RawFd> {
        let addr = CString::new(format!(":{port}"))?;
        let mut fd = -1;
        let rc = unsafe {
            self.lib.get::<unsafe extern "C" fn(
                c_int,
                *const c_char,
                *const c_char,
                *mut c_int,
            ) -> c_int>(b"tailscale_listen\0")?(
                self.id, c"tcp".as_ptr(), addr.as_ptr(), &mut fd
            )
        };
        self.check(rc)?;
        Ok(fd)
    }
    pub fn accept(&self, listener: RawFd) -> Result<std::os::unix::net::UnixStream> {
        let mut fd = -1;
        let rc = unsafe {
            self.lib
                .get::<unsafe extern "C" fn(c_int, *mut c_int) -> c_int>(b"tailscale_accept\0")?(
                listener, &mut fd,
            )
        };
        self.check(rc)?;
        let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        Ok(stream)
    }
}
impl Drop for TailNode {
    fn drop(&mut self) {
        unsafe {
            if let Ok(close) = self
                .lib
                .get::<unsafe extern "C" fn(c_int) -> c_int>(b"tailscale_close\0")
            {
                close(self.id);
            }
        }
    }
}
