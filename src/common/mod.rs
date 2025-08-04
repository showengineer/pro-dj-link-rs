use std::net::Ipv4Addr;

/// General device descriptor for CDJs/ProDJlink devices
#[derive(Debug, Clone)]
pub struct CDJDevice {
    /// Name of the Player
    pub name: String,
    /// CDJ player ID
    pub id: u8,
    /// MAC Address of the player
    pub mac: [u8; 6],
    /// IP Address of the player
    pub ip: Ipv4Addr,
    /// Device type of the player
    pub device_type: u8,
}