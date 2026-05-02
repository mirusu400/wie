extern crate std;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use std::{
    io::{ErrorKind, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        mpsc::{Receiver, TryRecvError, channel},
    },
    thread,
    time::Duration,
};

use hashbrown::HashMap;
use spin::Mutex;

pub const M_E_NONE: i32 = 0;
pub const M_E_ERROR: i32 = -1;
pub const M_E_BADFD: i32 = -2;
pub const M_E_INPROGRESS: i32 = -7;
pub const M_E_INVALID: i32 = -9;
pub const M_E_NOTCONN: i32 = -14;
pub const M_E_WOULDBLOCK: i32 = -19;

enum SocketState {
    Idle,
    Connecting(Receiver<std::io::Result<TcpStream>>),
    Connected(TcpStream),
    Closed,
}

struct SocketEntry {
    state: SocketState,
}

struct DnsEntry {
    rx: Receiver<Option<u32>>,
}

struct NetworkInner {
    sockets: HashMap<i32, SocketEntry>,
    dns_pending: HashMap<u32, DnsEntry>,
    dns_overrides: HashMap<String, u32>,
    /// When the game connects to loopback, rewrite this port to its
    /// mapped value before the real `connect()`. Used to redirect HTTP
    /// (port 80) traffic to an unprivileged mock server.
    loopback_port_redirects: HashMap<u16, u16>,
    next_fd: i32,
    next_dns_id: u32,
}

#[derive(Clone)]
pub struct Network {
    inner: Arc<Mutex<NetworkInner>>,
}

impl Network {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(NetworkInner {
                sockets: HashMap::new(),
                dns_pending: HashMap::new(),
                dns_overrides: HashMap::new(),
                loopback_port_redirects: HashMap::new(),
                next_fd: 3,
                next_dns_id: 1,
            })),
        }
    }

    /// Force `hostname` to resolve to `ip` (host-order IPv4). Used for routing
    /// emulated apps to a local mock server.
    pub fn add_dns_override(&self, hostname: &str, ip: u32) {
        self.inner.lock().dns_overrides.insert(hostname.to_string(), ip);
    }

    /// Map `original_port` -> `mapped_port` for any connect targeting
    /// 127.0.0.0/8. Lets a mock server bind on an unprivileged port even when
    /// the guest URL specifies port 80.
    pub fn add_loopback_port_redirect(&self, original: u16, mapped: u16) {
        self.inner.lock().loopback_port_redirects.insert(original, mapped);
    }

    pub fn resolve_start(&self, hostname: &str) -> u32 {
        let mut inner = self.inner.lock();
        let id = inner.next_dns_id;
        inner.next_dns_id += 1;

        if let Ok(ipv4) = hostname.parse::<Ipv4Addr>() {
            let (tx, rx) = channel();
            let _ = tx.send(Some(u32::from_be_bytes(ipv4.octets())));
            inner.dns_pending.insert(id, DnsEntry { rx });
            return id;
        }

        if let Some(&ip) = inner.dns_overrides.get(hostname) {
            tracing::info!("DNS override {hostname} -> {:#x}", ip);
            let (tx, rx) = channel();
            let _ = tx.send(Some(ip));
            inner.dns_pending.insert(id, DnsEntry { rx });
            return id;
        }

        let host_owned = hostname.to_string();
        let (tx, rx) = channel();
        thread::spawn(move || {
            let target = format!("{host_owned}:0");
            let result = target.to_socket_addrs().ok().and_then(|mut iter| {
                iter.find_map(|sa| match sa.ip() {
                    IpAddr::V4(v4) => Some(u32::from_be_bytes(v4.octets())),
                    _ => None,
                })
            });
            let _ = tx.send(result);
        });
        inner.dns_pending.insert(id, DnsEntry { rx });
        id
    }

    /// `Some(Some(ip))` ready, `Some(None)` failed, `None` still pending.
    pub fn resolve_poll(&self, id: u32) -> Option<Option<u32>> {
        let mut inner = self.inner.lock();
        let entry = inner.dns_pending.get(&id)?;
        match entry.rx.try_recv() {
            Ok(result) => {
                inner.dns_pending.remove(&id);
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                inner.dns_pending.remove(&id);
                Some(None)
            }
        }
    }

    pub fn socket_create(&self) -> i32 {
        let mut inner = self.inner.lock();
        let fd = inner.next_fd;
        inner.next_fd += 1;
        inner.sockets.insert(fd, SocketEntry { state: SocketState::Idle });
        fd
    }

    pub fn socket_connect_start(&self, fd: i32, addr: u32, port: u16) -> Result<(), i32> {
        let mut inner = self.inner.lock();
        let ipv4 = Ipv4Addr::from(addr.to_be_bytes());
        let effective_port = if ipv4.is_loopback() {
            inner.loopback_port_redirects.get(&port).copied().unwrap_or(port)
        } else {
            port
        };
        if effective_port != port {
            tracing::info!("loopback connect redirect {ipv4}:{port} -> {ipv4}:{effective_port}");
        }

        let entry = inner.sockets.get_mut(&fd).ok_or(M_E_BADFD)?;
        match entry.state {
            SocketState::Idle => {}
            _ => return Err(M_E_ERROR),
        }
        let sock_addr = SocketAddr::from((IpAddr::V4(ipv4), effective_port));
        let (tx, rx) = channel();
        thread::spawn(move || {
            let result = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(15));
            let _ = tx.send(result);
        });
        entry.state = SocketState::Connecting(rx);
        Ok(())
    }

    pub fn socket_connect_poll(&self, fd: i32) -> Option<Result<(), i32>> {
        let mut inner = self.inner.lock();
        let entry = inner.sockets.get_mut(&fd)?;
        let SocketState::Connecting(ref rx) = entry.state else {
            return Some(Err(M_E_NOTCONN));
        };
        match rx.try_recv() {
            Ok(Ok(stream)) => {
                if let Err(e) = stream.set_nonblocking(true) {
                    tracing::warn!("set_nonblocking failed on fd {fd}: {e}");
                    entry.state = SocketState::Closed;
                    return Some(Err(M_E_ERROR));
                }
                entry.state = SocketState::Connected(stream);
                Some(Ok(()))
            }
            Ok(Err(e)) => {
                tracing::warn!("connect failed on fd {fd}: {e}");
                entry.state = SocketState::Closed;
                Some(Err(M_E_ERROR))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                entry.state = SocketState::Closed;
                Some(Err(M_E_ERROR))
            }
        }
    }

    pub fn socket_write(&self, fd: i32, data: &[u8]) -> Result<usize, i32> {
        let mut inner = self.inner.lock();
        let entry = inner.sockets.get_mut(&fd).ok_or(M_E_BADFD)?;
        let SocketState::Connected(ref mut stream) = entry.state else {
            return Err(M_E_NOTCONN);
        };
        match stream.write(data) {
            Ok(0) if data.is_empty() => Ok(0),
            Ok(0) => Err(M_E_ERROR),
            Ok(n) => Ok(n),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Err(M_E_WOULDBLOCK),
            Err(e) => {
                tracing::warn!("write failed on fd {fd}: {e}");
                Err(M_E_ERROR)
            }
        }
    }

    /// `Ok(0)` indicates EOF; `Err(M_E_WOULDBLOCK)` indicates no data yet.
    pub fn socket_read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
        let mut inner = self.inner.lock();
        let entry = inner.sockets.get_mut(&fd).ok_or(M_E_BADFD)?;
        let SocketState::Connected(ref mut stream) = entry.state else {
            return Err(M_E_NOTCONN);
        };
        match stream.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == ErrorKind::WouldBlock => Err(M_E_WOULDBLOCK),
            Err(e) => {
                tracing::warn!("read failed on fd {fd}: {e}");
                Err(M_E_ERROR)
            }
        }
    }

    /// True if the socket has buffered data, EOF, or is in error.
    /// Used to drive level-triggered `MC_netSetReadCB`.
    pub fn socket_read_ready(&self, fd: i32) -> bool {
        let mut inner = self.inner.lock();
        let Some(entry) = inner.sockets.get_mut(&fd) else {
            return true;
        };
        let SocketState::Connected(ref stream) = entry.state else {
            return true;
        };
        let mut peek_buf = [0u8; 1];
        match stream.peek(&mut peek_buf) {
            Ok(_) => true,
            Err(e) if e.kind() == ErrorKind::WouldBlock => false,
            Err(_) => true,
        }
    }

    pub fn socket_close(&self, fd: i32) -> Result<(), i32> {
        let mut inner = self.inner.lock();
        if inner.sockets.remove(&fd).is_some() { Ok(()) } else { Err(M_E_BADFD) }
    }

    pub fn dns_overrides(&self) -> Vec<(String, u32)> {
        let inner = self.inner.lock();
        inner.dns_overrides.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}
