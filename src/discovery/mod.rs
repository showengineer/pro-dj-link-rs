use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::Sender;
use anyhow::Result;
use crate::common::CDJDevice;

const TTL: Duration = Duration::from_secs(10);

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
                    cached.last_seen = now; // refresh
                    false
                }
            }
            None => {
                self.cache.insert(key, Cached { device, last_seen: now });
                true
            }
        }
    }

    pub fn purge_stale(&mut self) {
        let now = Instant::now();
        self.cache.retain(|_, c| now.duration_since(c.last_seen) <= TTL);
    }
}

/// Parse announce packet van 54 bytes
fn parse_announce_packet(buf: &[u8]) -> Option<CDJDevice> {
    if buf.len() != 54 { return None; }
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

/// Start discovery loop op UDP 50000. Stuurt alleen nieuwe/verse devices over `tx`.
pub async fn listen_for_devices(tx: Sender<CDJDevice>) -> Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:50000").await?;
    socket.set_broadcast(true)?;
    let mut buf = [0u8; 1024];
    let mut store = DeviceStore::new();

    loop {
        let (len, _src) = socket.recv_from(&mut buf).await?;
        if let Some(device) = parse_announce_packet(&buf[..len]) {
            if store.upsert(device.clone()) {
                tx.send(device).await?;
            }
        }
        store.purge_stale();
    }
}