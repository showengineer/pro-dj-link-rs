use socket2::{Socket, Domain, Type, Protocol, SockAddr};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::Sender;
use anyhow::{Result, Context};
use get_if_addrs::get_if_addrs;
use crate::common::CDJDevice;

const TTL: Duration = Duration::from_secs(10);
const DEFAULT_BIND: &str = "0.0.0.0";
const DISCOVERY_PORT: u16 = 50000;
struct Cached {
    device: CDJDevice,
    last_seen: Instant,
}

/// Houdt een dedupe/cache bij, zodat je alleen verse discovery events pusht.
pub struct DeviceStore {
    cache: HashMap<(Ipv4Addr, u8), Cached>,
}

impl DeviceStore {
    pub fn new() -> Self {
        Self { cache: HashMap::new() }
    }

    /// Voegt toe of update. Returnt true als dit een nieuwe/verse melding is.
    pub fn upsert(&mut self, device: CDJDevice) -> bool {
        let key = (device.ip, device.id);
        let now = Instant::now();
        match self.cache.get_mut(&key) {
            Some(cached) => {
                if now.duration_since(cached.last_seen) > TTL {
                    cached.device = device;
                    cached.last_seen = now;
                    true
                } else {
                    cached.last_seen = now;
                    false
                }
            }
            None => {
                self.cache.insert(key, Cached { device, last_seen: now });
                true
            }
        }
    }

    /// Purge stale entries
    pub fn purge_stale(&mut self) {
        let now = Instant::now();
        self.cache.retain(|_, c| now.duration_since(c.last_seen) <= TTL);
    }
}

/// Parse announce packet, vereist ten minste 54 bytes
fn parse_announce_packet(buf: &[u8]) -> Option<CDJDevice> {
    const ANNOUNCE_LEN: usize = 54;
    if buf.len() < ANNOUNCE_LEN { return None; }
    if &buf[0..10] != b"Qspt1WmJOL" { return None; }
    if buf[10] != 0x06 { return None; }
    let raw_name = &buf[0x0C..0x0C + 20];
    let name = String::from_utf8_lossy(raw_name)
        .trim_end_matches(char::from(0))
        .to_string();
    let id = buf[0x24];
    let mac = <[u8; 6]>::try_from(&buf[0x26..0x26 + 6]).ok()?;
    let ip = Ipv4Addr::new(buf[0x2C], buf[0x2D], buf[0x2E], buf[0x2F]);
    let device_type = buf[0x34];
    Some(CDJDevice { name, id, mac, ip, device_type })
}

/// Start discovery-lus op UDP 50000. Kies interface via naam of IP.
pub async fn listen_for_devices(
    tx: Sender<CDJDevice>,
    bind_option: Option<String>
) -> Result<()> {
    // Bepaal bind IP
    let bind_ip: Ipv4Addr = if let Some(ref opt) = bind_option {
        if let Ok(ip) = opt.parse() {
            ip
        } else {
            let addrs = get_if_addrs().context("Cannot list interfaces")?;
            let entry = addrs.into_iter()
                .find(|iface| iface.name == *opt && iface.ip().is_ipv4())
                .with_context(|| format!("Interface '{}' not found or no IPv4", opt))?;
            match entry.ip() {
                IpAddr::V4(v4) => v4,
                _ => unreachable!(),
            }
        }
    } else {
        DEFAULT_BIND.parse().unwrap()
    };

    // Bouw socket2 Socket
    let socket2 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create socket2 socket")?;
    socket2.set_reuse_address(true)?;
    socket2.set_broadcast(true)?;
    // Bind en join multicast
    let sockaddr = SocketAddr::new(IpAddr::V4(bind_ip), DISCOVERY_PORT);
    socket2.bind(&SockAddr::from(sockaddr))?;
    // Converteer naar Tokio UdpSocket
    let std_socket: std::net::UdpSocket = socket2.into();
    std_socket.set_nonblocking(true)?;
    let socket = UdpSocket::bind("0.0.0.0:50000").await?;

    let mut buf = [0u8; 2048];
    let mut store = DeviceStore::new();
    loop {
        let (len, src) = socket.recv_from(&mut buf).await?;
        println!("📦 {} bytes from {}", len, src);
        if let Some(device) = parse_announce_packet(&buf[..len]) {
            if store.upsert(device.clone()) {
                println!("✨ New device: {:?}", device);
                tx.send(device).await?;
            }
        }
        store.purge_stale();
    }
}