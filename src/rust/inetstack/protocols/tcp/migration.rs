//==============================================================================
// Imports
//==============================================================================

use super::established::UnackedSegment;
use crate::{
    inetstack::protocols::tcp::SeqNumber,
    runtime::memory::Buffer,
};
use byteorder::{NetworkEndian, ByteOrder};
use std::net::Ipv4Addr;
use ::std::{
    collections::VecDeque,
    net::SocketAddrV4,
};

//==============================================================================
// Structures
//==============================================================================

/// State needed for TCP Migration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TcpState {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,

    pub reader_next: SeqNumber,
    pub receive_next: SeqNumber,

    pub recv_queue: VecDeque<Buffer>,

    pub seq_no: SeqNumber,
    pub send_next: SeqNumber,
    pub send_window: u32,
    pub send_window_last_update_seq: SeqNumber,
    pub send_window_last_update_ack: SeqNumber,
    pub window_scale: u8,
    pub mss: usize,
    pub unacked_queue: VecDeque<UnackedSegment>,
    pub unsent_queue: VecDeque<Buffer>,

    pub receiver_window_size: u32,
    pub receiver_window_scale: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpMigrationHeader {
    pub origin: SocketAddrV4,
    pub dest: SocketAddrV4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpMigrationSegment {
    pub header: TcpMigrationHeader,
    pub state: TcpState,
}

//==============================================================================
// Associated FUnctions
//==============================================================================

impl TcpState {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,

        reader_next: SeqNumber,
        receive_next: SeqNumber,
        recv_queue: VecDeque<Buffer>,

        seq_no: SeqNumber,
        send_next: SeqNumber,
        send_window: u32,
        send_window_last_update_seq: SeqNumber,
        send_window_last_update_ack: SeqNumber,
        window_scale: u8,
        mss: usize,
        unacked_queue: VecDeque<UnackedSegment>,
        unsent_queue: VecDeque<Buffer>,

        receiver_window_size: u32,
        receiver_window_scale: u32,
    ) -> Self {
        Self {
            local,
            remote,

            reader_next,
            receive_next,
            recv_queue,

            seq_no,
            send_next,
            send_window,
            send_window_last_update_seq,
            send_window_last_update_ack,
            window_scale,
            mss,
            unacked_queue,
            unsent_queue,

            receiver_window_size,
            receiver_window_scale,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, serde_json::Error> {
        Ok(serde_json::to_string_pretty(self)?.as_bytes().to_vec())
    }

    pub fn deserialize(serialized: &[u8]) -> Result<Self, serde_json::Error> {
        // TODO: Check if having all `UnackedSegment` timestamps as `None` affects anything.
        serde_json::from_slice::<TcpState>(serialized)
    }
}

impl TcpMigrationHeader {
    /// TcpMigrationHeader size in bytes.
    const SIZE: usize = 20;

    pub fn new(origin: SocketAddrV4, dest: SocketAddrV4) -> Self {
        Self { origin, dest }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = [0; Self::SIZE];
        NetworkEndian::write_u32(&mut bytes[0..4], 0xCAFEDEAD);
        bytes[4..8].copy_from_slice(&self.origin.ip().octets());
        NetworkEndian::write_u16(&mut bytes[8..10], self.origin.port());
        bytes[10..14].copy_from_slice(&self.dest.ip().octets());
        NetworkEndian::write_u16(&mut bytes[14..16], self.dest.port());
        bytes[16] = bytes[4..].iter().fold(0, |sum, e| sum + e).wrapping_neg();

        assert_eq!(bytes[4..].iter().fold(0, |sum, e| sum + e), 0);

        bytes.to_vec()
    }

    /// Panics if slice is not long enough, or if the header is not in the right format.
    pub fn deserialize(serialized: &[u8]) -> Result<Self, &str> {
        if serialized.len() < Self::SIZE { panic!("Serialized TcpMigrationHeader not long enough.") }

        if NetworkEndian::read_u32(serialized) != 0xCAFEDEAD { Err("Magic number (0xCAFEDEAD) not found") }
        else if serialized[4..Self::SIZE].iter().fold(0, |sum, e| sum + e) != 0 { Err("Invalid checksum") }
        else { Ok(Self {
            origin: SocketAddrV4::new(
                Ipv4Addr::new(serialized[4], serialized[5], serialized[6], serialized[7]),
                NetworkEndian::read_u16(&serialized[8..10]),
            ),
            dest: SocketAddrV4::new(
                Ipv4Addr::new(serialized[10], serialized[11], serialized[12], serialized[13]),
                NetworkEndian::read_u16(&serialized[14..16]),
            ),
        })}
    }
}

impl TcpMigrationSegment {
    pub fn new(header: TcpMigrationHeader, state: TcpState) -> Self {
        Self { header, state }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = self.header.serialize();
        bytes.extend_from_slice(&self.state.serialize()?);
        Ok(bytes)
    }

    pub fn deserialize(serialized: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self{
            header: TcpMigrationHeader::deserialize(serialized)?,
            state: TcpState::deserialize(&serialized[TcpMigrationHeader::SIZE..])?,
        })
    }
}