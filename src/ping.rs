//! Measuring network latency (ping) to a target host.
//!
//! Two methods are supported:
//!   * `icmp` - ICMP echo request sent over a raw socket (works without
//!     administrator rights on Windows 10 1709+).
//!   * `tcp`  - time needed to establish a TCP connection to `host:port`.
//!     Used as a fallback when ICMP is not available.

use std::mem;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PingMode {
    Icmp,
    Tcp,
}

impl PingMode {
    pub fn parse(s: &str) -> Option<PingMode> {
        match s.to_ascii_lowercase().as_str() {
            "icmp" => Some(PingMode::Icmp),
            "tcp" => Some(PingMode::Tcp),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub mode: PingMode,
}

impl Target {
    pub fn new(host: &str, port: u16, mode: PingMode) -> Target {
        Target {
            host: host.to_string(),
            port,
            mode,
        }
    }

    pub fn label(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Result of a single latency probe.
#[derive(Clone, Copy, Debug)]
pub struct PingResult {
    pub rtt_ms: Option<f32>,
}

// ---------------------------------------------------------------------------
// Minimal Winsock bindings for raw ICMP echo.
// ---------------------------------------------------------------------------

const AF_INET: i32 = 2;
const SOCK_RAW: i32 = 3;
const IPPROTO_ICMP: i32 = 1;
const SOL_SOCKET: i32 = 1;
const SO_RCVTIMEO: i32 = 0x1002;
const INVALID_SOCKET: usize = usize::MAX;

type SOCKET = usize;

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TimeVal {
    tv_sec: i64,
    tv_usec: i64,
}

#[link(name = "ws2_32")]
extern "system" {
    fn WSAStartup(version: u16, data: *mut u8) -> i32;
    fn socket(af: i32, ty: i32, proto: i32) -> SOCKET;
    fn sendto(
        s: SOCKET,
        buf: *const u8,
        len: i32,
        flags: i32,
        to: *const SockAddrIn,
        tolen: i32,
    ) -> i32;
    fn recvfrom(
        s: SOCKET,
        buf: *mut u8,
        len: i32,
        flags: i32,
        from: *mut SockAddrIn,
        fromlen: *mut i32,
    ) -> i32;
    fn setsockopt(s: SOCKET, level: i32, name: i32, val: *const u8, len: u32) -> i32;
    fn closesocket(s: SOCKET) -> i32;
}

fn winsock_ready() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static READY: AtomicBool = AtomicBool::new(false);
    if READY.load(Ordering::Relaxed) {
        return true;
    }
    unsafe {
        let mut data = [0u8; 512];
        let r = WSAStartup(0x0202, data.as_mut_ptr());
        if r != 0 {
            return false;
        }
    }
    READY.store(true, Ordering::Relaxed);
    true
}

fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
}

/// Internet checksum as used by ICMP.
fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(2);
    for c in &mut chunks {
        sum += u16::from_be_bytes([c[0], c[1]]) as u32;
    }
    if let Some(&b) = chunks.remainder().first() {
        sum += (b as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// ICMP echo request over a raw socket. Returns the round trip time.
fn icmp_ping(host: &str, timeout: Duration) -> Option<f32> {
    if !winsock_ready() {
        return None;
    }

    let ip = resolve(host, 0)?;
    let octets = match ip {
        SocketAddr::V4(v4) => v4.ip().octets(),
        SocketAddr::V6(_) => return None,
    };

    unsafe {
        let s = socket(AF_INET, SOCK_RAW, IPPROTO_ICMP);
        if s == INVALID_SOCKET {
            return None;
        }

        let tv = TimeVal {
            tv_sec: timeout.as_secs() as i64,
            tv_usec: (timeout.subsec_millis() * 1000) as i64,
        };
        setsockopt(
            s,
            SOL_SOCKET,
            SO_RCVTIMEO,
            &tv as *const _ as *const u8,
            mem::size_of::<TimeVal>() as u32,
        );

        let dest = SockAddrIn {
            sin_family: AF_INET as u16,
            sin_port: 0,
            sin_addr: u32::from_le_bytes(octets),
            sin_zero: [0; 8],
        };

        // ICMP echo request: type 8, code 0, checksum, id, sequence, payload
        let id: u16 = (std::process::id() & 0xffff) as u16;
        let seq: u16 = 1;
        let mut packet = vec![0u8; 12 + 16];
        packet[0] = 8;
        packet[1] = 0;
        packet[4..6].copy_from_slice(&id.to_be_bytes());
        packet[6..8].copy_from_slice(&seq.to_be_bytes());
        packet[8..24].copy_from_slice(b"gameoverlay-icmp");

        let csum = checksum(&packet);
        packet[2..4].copy_from_slice(&csum.to_be_bytes());

        let start = Instant::now();
        let sent = sendto(
            s,
            packet.as_ptr(),
            packet.len() as i32,
            0,
            &dest as *const SockAddrIn,
            mem::size_of::<SockAddrIn>() as i32,
        );
        if sent < 0 {
            closesocket(s);
            return None;
        }

        let mut buf = [0u8; 2048];
        let mut from = SockAddrIn {
            sin_family: 0,
            sin_port: 0,
            sin_addr: 0,
            sin_zero: [0; 8],
        };
        let mut from_len = mem::size_of::<SockAddrIn>() as i32;

        let result = loop {
            let n = recvfrom(
                s,
                buf.as_mut_ptr(),
                buf.len() as i32,
                0,
                &mut from as *mut SockAddrIn,
                &mut from_len,
            );
            if n <= 0 {
                break None;
            }
            let data = &buf[..n as usize];

            // On Windows a raw socket delivers the full IP packet.
            let icmp = if data.len() > 20 && (data[0] >> 4) == 4 {
                let ihl = ((data[0] & 0x0f) as usize) * 4;
                if ihl > 0 && data.len() >= ihl + 8 {
                    &data[ihl..]
                } else {
                    continue;
                }
            } else {
                data
            };

            // echo reply = type 0
            if icmp[0] != 0 {
                continue;
            }
            if icmp.len() >= 8 && icmp[4..6] != id.to_be_bytes() {
                continue;
            }
            break Some(start.elapsed().as_secs_f32() * 1000.0);
        };

        closesocket(s);
        result
    }
}

/// TCP connect latency, works for every user.
fn tcp_ping(host: &str, port: u16, timeout: Duration) -> Option<f32> {
    let addr = resolve(host, port)?;
    let start = Instant::now();
    let stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    let elapsed = start.elapsed();
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Some(elapsed.as_secs_f32() * 1000.0)
}

/// Probe the target once. ICMP is tried first when requested, with an
/// automatic fallback to TCP so the overlay always shows something.
pub fn probe(target: &Target, timeout: Duration) -> PingResult {
    let mut rtt = match target.mode {
        PingMode::Icmp => icmp_ping(&target.host, timeout),
        PingMode::Tcp => None,
    };

    if rtt.is_none() {
        rtt = tcp_ping(&target.host, target.port, timeout);
    }

    match rtt {
        Some(ms) => PingResult { rtt_ms: Some(ms) },
        None => PingResult { rtt_ms: None },
    }
}

// keeps the `HANDLE` import meaningful for future use of the IP Helper API
#[allow(dead_code)]
type UnusedHandle = HANDLE;
