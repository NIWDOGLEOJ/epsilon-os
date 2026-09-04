//! In-Kernel Virtual Loopback Network Stack for AegisOS
//!
//! Provides IPv4 packet framing (RFC 791), UDP datagram transport (RFC 768),
//! internet checksum validation, virtual loopback network device (127.0.0.1),
//! and high-level non-blocking UdpSocket abstractions.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::InterruptGuard;

/// IPv4 4-octet Internet Address
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    pub const LOOPBACK: Ipv4Address = Ipv4Address([127, 0, 0, 1]);
    pub const BROADCAST: Ipv4Address = Ipv4Address([255, 255, 255, 255]);
    pub const ANY: Ipv4Address = Ipv4Address([0, 0, 0, 0]);

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == [255, 255, 255, 255]
    }

    pub fn to_string(&self) -> String {
        format!("{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

/// IPv4 Packet Header (RFC 791, 20 bytes standard without options)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub tos: u8,
    pub total_len: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8, // 17 = UDP
    pub checksum: u16,
    pub src_ip: Ipv4Address,
    pub dst_ip: Ipv4Address,
}

impl Ipv4Header {
    pub const PROTO_UDP: u8 = 17;

    pub fn new(src_ip: Ipv4Address, dst_ip: Ipv4Address, protocol: u8, payload_len: u16) -> Self {
        let total_len = 20 + payload_len;
        let mut header = Self {
            version_ihl: 0x45, // IPv4, 5 x 32-bit words (20 bytes)
            tos: 0,
            total_len,
            identification: 0x1337,
            flags_fragment: 0x4000, // Don't fragment
            ttl: 64,
            protocol,
            checksum: 0,
            src_ip,
            dst_ip,
        };

        // Compute RFC 791 header checksum
        let raw = header.serialize();
        header.checksum = calculate_checksum(&raw);
        header
    }

    pub fn serialize(&self) -> [u8; 20] {
        let mut bytes = [0u8; 20];
        bytes[0] = self.version_ihl;
        bytes[1] = self.tos;
        bytes[2..4].copy_from_slice(&self.total_len.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.identification.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.flags_fragment.to_be_bytes());
        bytes[8] = self.ttl;
        bytes[9] = self.protocol;
        bytes[10..12].copy_from_slice(&self.checksum.to_be_bytes());
        bytes[12..16].copy_from_slice(&self.src_ip.0);
        bytes[16..20].copy_from_slice(&self.dst_ip.0);
        bytes
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        let version_ihl = data[0];
        if (version_ihl >> 4) != 4 {
            return None; // Not IPv4
        }

        let ihl = (version_ihl & 0x0F) as usize * 4;
        if ihl < 20 || data.len() < ihl {
            return None;
        }

        let tos = data[1];
        let total_len = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags_fragment = u16::from_be_bytes([data[6], data[7]]);
        let ttl = data[8];
        let protocol = data[9];
        let checksum = u16::from_be_bytes([data[10], data[11]]);
        let src_ip = Ipv4Address([data[12], data[13], data[14], data[15]]);
        let dst_ip = Ipv4Address([data[16], data[17], data[18], data[19]]);

        Some(Self {
            version_ihl,
            tos,
            total_len,
            identification,
            flags_fragment,
            ttl,
            protocol,
            checksum,
            src_ip,
            dst_ip,
        })
    }
}

/// UDP Datagram Header (RFC 768, 8 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub fn new(src_port: u16, dst_port: u16, payload_len: u16) -> Self {
        Self {
            src_port,
            dst_port,
            length: 8 + payload_len,
            checksum: 0, // Optional in IPv4 UDP
        }
    }

    pub fn serialize(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0..2].copy_from_slice(&self.src_port.to_be_bytes());
        bytes[2..4].copy_from_slice(&self.dst_port.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.length.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.checksum.to_be_bytes());
        bytes
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let length = u16::from_be_bytes([data[4], data[5]]);
        let checksum = u16::from_be_bytes([data[6], data[7]]);
        Some(Self {
            src_port,
            dst_port,
            length,
            checksum,
        })
    }
}

/// Computes RFC 791 Ones' Complement Internet Checksum over arbitrary byte slices.
pub fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        let word = (data[i] as u32) << 8;
        sum = sum.wrapping_add(word);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Virtual Loopback Network Device
pub struct LoopbackDevice {
    pub rx_queue: VecDeque<Vec<u8>>,
    pub total_rx_packets: u64,
    pub total_tx_packets: u64,
    pub total_bytes: u64,
}

impl LoopbackDevice {
    pub const fn new() -> Self {
        Self {
            rx_queue: VecDeque::new(),
            total_rx_packets: 0,
            total_tx_packets: 0,
            total_bytes: 0,
        }
    }

    /// Transmits a network frame. If addressed to loopback (127.x.x.x) or broadcast,
    /// it is immediately placed into the local receive queue.
    pub fn transmit(&mut self, frame: &[u8]) {
        self.total_tx_packets += 1;
        self.total_bytes += frame.len() as u64;

        if let Some(ip_hdr) = Ipv4Header::parse(frame) {
            if ip_hdr.dst_ip.is_loopback() || ip_hdr.dst_ip.is_broadcast() {
                self.rx_queue.push_back(frame.to_vec());
                self.total_rx_packets += 1;
            }
        }
    }

    /// Dequeues the next received packet, if available.
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        self.rx_queue.pop_front()
    }
}

pub static LOOPBACK_DEVICE: Mutex<LoopbackDevice> = Mutex::new(LoopbackDevice::new());

/// High-level User Datagram Protocol (UDP) Socket abstraction.
pub struct UdpSocket {
    pub local_port: u16,
}

impl UdpSocket {
    pub fn bind(port: u16) -> Self {
        Self { local_port: port }
    }

    /// Constructs an IPv4 + UDP packet and transmits it via the loopback device.
    pub fn send_to(
        &self,
        dst_ip: Ipv4Address,
        dst_port: u16,
        payload: &[u8],
    ) -> Result<usize, &'static str> {
        let ip_hdr = Ipv4Header::new(
            Ipv4Address::LOOPBACK,
            dst_ip,
            Ipv4Header::PROTO_UDP,
            8 + payload.len() as u16,
        );
        let udp_hdr = UdpHeader::new(self.local_port, dst_port, payload.len() as u16);

        let mut packet = Vec::with_capacity(20 + 8 + payload.len());
        packet.extend_from_slice(&ip_hdr.serialize());
        packet.extend_from_slice(&udp_hdr.serialize());
        packet.extend_from_slice(payload);

        let _guard = InterruptGuard::acquire();
        LOOPBACK_DEVICE.lock().transmit(&packet);

        Ok(payload.len())
    }

    /// Non-blocking receive for datagrams addressed to this socket's local port.
    pub fn recv_from(&self) -> Option<(Ipv4Address, u16, Vec<u8>)> {
        let _guard = InterruptGuard::acquire();
        let mut dev = LOOPBACK_DEVICE.lock();

        // Search queue for a packet matching our local port
        let mut matched_idx: Option<usize> = None;
        let mut result: Option<(Ipv4Address, u16, Vec<u8>)> = None;

        for (idx, packet) in dev.rx_queue.iter().enumerate() {
            if let Some(ip_hdr) = Ipv4Header::parse(packet) {
                if ip_hdr.protocol == Ipv4Header::PROTO_UDP && packet.len() >= 28 {
                    if let Some(udp_hdr) = UdpHeader::parse(&packet[20..]) {
                        if udp_hdr.dst_port == self.local_port || udp_hdr.dst_port == 0 {
                            let payload = packet[28..].to_vec();
                            result = Some((ip_hdr.src_ip, udp_hdr.src_port, payload));
                            matched_idx = Some(idx);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(idx) = matched_idx {
            dev.rx_queue.remove(idx);
        }

        result
    }
}
