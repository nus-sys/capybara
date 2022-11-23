// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::ipv4::Ipv4Header,
    runtime::{
        fail::Fail,
        memory::{
            Buffer,
            DataBuffer,
        },
    },
};
use super::super::MigrationStage;
use ::byteorder::{
    ByteOrder,
    NetworkEndian,
};
use ::libc::EBADMSG;
use ::std::convert::TryInto;
use std::net::{SocketAddrV4, Ipv4Addr};

//==============================================================================
// Constants
//==============================================================================

/// Size of a TCPMig header (in bytes).
pub const TCPMIG_HEADER_SIZE: usize = 24;

const FLAG_LOAD_BIT: u8 = 0;
const FLAG_PREPARE_MIGRATION_BIT: u8 = 1;
const FLAG_ACK_BIT: u8 = 2;
const FLAG_PAYLOAD_STATE_BIT: u8 = 3;

//==============================================================================
// Structures
//==============================================================================

//
//  Header format:
//  
//  Offset  Size    Data
//  0       4       Origin IP
//  4       2       Origin Port
//  6       4       Target IP
//  10      2       Target Port
//  12      4       Remote IP
//  16      2       Remote Port
//  18      2       Payload Length
//  20      1       Flags
//  21      1       Migration Stage
//  22      2       Checksum
//
//  TOTAL 24
//
//
//  Flags format:
//  Bit number      Flag
//  0               LOAD - Instructs the switch to load the entry into the migration tables.
//  1               PREPARE_MIGRATION - Instructs `server_target` to prepare itself to receive a migrated connection.
//  2               ACK - Notifies `server_origin` that its previous request has been acknowledged and completed.
//  3               PAYLOAD_STATE - The payload of this segment contains the TCP state.
//  4-7             Zero (unused)
//
// The flags are listed in decreasing priority.
//  

#[derive(Debug)]
pub struct TcpMigHeader {
    /// Client-facing address of the origin server.
    pub origin: SocketAddrV4,
    /// Client-facing address of the target server.
    pub target: SocketAddrV4,
    /// Client's address.
    pub remote: SocketAddrV4,
    pub payload_length: u16,

    pub flag_load: bool,
    pub flag_prepare_migration: bool,
    pub flag_ack: bool,
    pub flag_payload_state: bool,

    pub stage: MigrationStage,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TcpMigHeader {
    /// Creates a TcpMigration header.
    pub fn new(origin: SocketAddrV4, target: SocketAddrV4, remote: SocketAddrV4, payload_length: u16, stage: MigrationStage) -> Self {
        Self {
            origin,
            target,
            remote,
            payload_length,
            flag_load: false,
            flag_prepare_migration: false,
            flag_ack: false,
            flag_payload_state: false,
            stage,
        }
    }

    /// Returns the size of the TcpMigration header (in bytes).
    pub fn size(&self) -> usize {
        TCPMIG_HEADER_SIZE
    }

    /// Parses a byte slice into a TcpMigration header.
    pub fn parse_from_slice<'a>(
        ipv4_hdr: &Ipv4Header,
        buf: &'a [u8],
    ) -> Result<(Self, &'a [u8]), Fail> {
        // Malformed header.
        if buf.len() < TCPMIG_HEADER_SIZE {
            return Err(Fail::new(EBADMSG, "TCPMig segment too small"));
        }

        // Deserialize buffer.
        let hdr_buf: &[u8] = &buf[..TCPMIG_HEADER_SIZE];
        let origin = SocketAddrV4::new(
            Ipv4Addr::new(hdr_buf[0], hdr_buf[1], hdr_buf[2], hdr_buf[3]),
            NetworkEndian::read_u16(&hdr_buf[4..6]),
        );
        let target = SocketAddrV4::new(
            Ipv4Addr::new(hdr_buf[6], hdr_buf[7], hdr_buf[8], hdr_buf[9]),
            NetworkEndian::read_u16(&hdr_buf[10..12]),
        );
        let remote = SocketAddrV4::new(
            Ipv4Addr::new(hdr_buf[12], hdr_buf[13], hdr_buf[14], hdr_buf[15]),
            NetworkEndian::read_u16(&hdr_buf[16..18]),
        );

        let payload_length = NetworkEndian::read_u16(&hdr_buf[18..20]);

        // Flags.
        let flags = hdr_buf[20];
        let flag_load = (flags & (1 << FLAG_LOAD_BIT)) != 0;
        let flag_prepare_migration = (flags & (1 << FLAG_PREPARE_MIGRATION_BIT)) != 0;
        let flag_ack = (flags & (1 << FLAG_ACK_BIT)) != 0;
        let flag_payload_state = (flags & (1 << FLAG_PAYLOAD_STATE_BIT)) != 0;

        let stage: MigrationStage = match hdr_buf[21].try_into() {
            Ok(stage) => stage,
            Err(e) => return Err(Fail::new(EBADMSG, &format!("Invalid TCPMig stage: {}", e))),
        };

        // Checksum payload.
        let payload_buf: &[u8] = &buf[TCPMIG_HEADER_SIZE..];
        let checksum: u16 = NetworkEndian::read_u16(&hdr_buf[22..24]);
        if checksum != Self::checksum(&ipv4_hdr, hdr_buf, payload_buf) {
            return Err(Fail::new(EBADMSG, "TCPMig checksum mismatch"));
        }

        let header = Self {
            origin,
            target,
            remote,
            payload_length,
            flag_load,
            flag_prepare_migration,
            flag_ack,
            flag_payload_state,
            stage,
        };

        Ok((header, &buf[TCPMIG_HEADER_SIZE..]))
    }

    /// Parses a buffer into a TcpMigration header.
    pub fn parse(ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<(Self, Buffer), Fail> {
        match Self::parse_from_slice(ipv4_hdr, &buf) {
            Ok((hdr, bytes)) => Ok((hdr, Buffer::Heap(DataBuffer::from_slice(bytes)))),
            Err(e) => Err(e),
        }
    }

    /// Serializes the target TcpMigration header.
    pub fn serialize(&self, buf: &mut [u8], ipv4_hdr: &Ipv4Header, data: &[u8]) {
        let fixed_buf: &mut [u8; TCPMIG_HEADER_SIZE] = (&mut buf[..TCPMIG_HEADER_SIZE]).try_into().unwrap();

        fixed_buf[0..4].copy_from_slice(&self.origin.ip().octets());
        NetworkEndian::write_u16(&mut fixed_buf[4..6], self.origin.port());
        fixed_buf[6..10].copy_from_slice(&self.target.ip().octets());
        NetworkEndian::write_u16(&mut fixed_buf[10..12], self.target.port());
        fixed_buf[12..16].copy_from_slice(&self.remote.ip().octets());
        NetworkEndian::write_u16(&mut fixed_buf[16..18], self.remote.port());

        NetworkEndian::write_u16(&mut fixed_buf[18..20], data.len() as u16);
        fixed_buf[20] = self.serialize_flags();
        fixed_buf[21] = self.stage as u8;

        let checksum = Self::checksum(ipv4_hdr, fixed_buf, data);
        NetworkEndian::write_u16(&mut fixed_buf[22..24], checksum);
    }

    #[inline(always)]
    fn serialize_flags(&self) -> u8 {
        (if self.flag_load {1} else {0} << FLAG_LOAD_BIT)
        | (if self.flag_prepare_migration {1} else {0} << FLAG_PREPARE_MIGRATION_BIT)
        | (if self.flag_ack {1} else {0} << FLAG_ACK_BIT)
        | (if self.flag_payload_state {1} else {0} << FLAG_PAYLOAD_STATE_BIT)
    }

    /// Computes the checksum of a TcpMigration segment.
    fn checksum(ipv4_hdr: &Ipv4Header, migration_hdr: &[u8], data: &[u8]) -> u16 {
        let data_chunks_rem = data.chunks_exact(2).remainder();
        let data_chunks_rem: [u8; 2] = match data_chunks_rem.len() {
            0 => [0, 0],
            1 => [data_chunks_rem[0], 0],
            _ => unreachable!()
        };

        ipv4_hdr.get_src_addr().octets().chunks_exact(2)
        .chain(ipv4_hdr.get_dest_addr().octets().chunks_exact(2))
        .chain(migration_hdr[0..22].chunks_exact(2)) // ignore checksum field
        .chain(data.chunks_exact(2))
        .chain(data_chunks_rem.chunks_exact(2))
        .fold(0, |sum: u16, e| sum.wrapping_add(NetworkEndian::read_u16(e)))
        .wrapping_neg()
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod test {
    use crate::inetstack::protocols::ip::IpProtocol;
    use super::*;
    use ::std::net::Ipv4Addr;
    use std::str::FromStr;

    /// Builds a fake Ipv4 Header.
    fn ipv4_header() -> Ipv4Header {
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::TCPMig;
        Ipv4Header::new(src_addr, dst_addr, protocol)
    }

    fn tcpmig_header() -> TcpMigHeader {
        TcpMigHeader {
            origin: SocketAddrV4::from_str("198.0.0.1:20000").unwrap(),
            target: SocketAddrV4::from_str("198.0.0.2:20000").unwrap(),
            remote: SocketAddrV4::from_str("18.45.32.67:19465").unwrap(),
            payload_length: 8,
            flag_load: false,
            flag_prepare_migration: true,
            flag_ack: false,
            flag_payload_state: false,
            stage: MigrationStage::PrepareMigration,
        }
    }

    /// Tests Checksum
    #[test]
    fn test_tcpmig_header_checksum() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let hdr: &[u8] = &[
            198, 0, 0, 1, 0x4e, 0x20, // origin
            198, 0, 0, 2, 0x4e, 0x20, // target
            18, 45, 32, 67, 0x4c, 0x09, // remote
            0, 8, // payload length
            0b00000010, // flags
            1,
            0, 0
        ];

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        let checksum = TcpMigHeader::checksum(&ipv4_hdr, hdr, &data);
        assert_eq!(checksum, 0xcb34);
    }

    /// Tests TCPMig serialization.
    #[test]
    fn test_tcpmig_header_serialization() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let hdr = tcpmig_header();
        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Output buffer.
        let mut buf: [u8; TCPMIG_HEADER_SIZE] = [0; TCPMIG_HEADER_SIZE];

        hdr.serialize(&mut buf, &ipv4_hdr, &data);
        assert_eq!(buf, [
            198, 0, 0, 1, 0x4e, 0x20, // origin
            198, 0, 0, 2, 0x4e, 0x20, // target
            18, 45, 32, 67, 0x4c, 0x09, // remote
            0, 8, // payload length
            0b00000010, // flags
            1,
            0xcb, 0x34
        ]);
    }

    /// Tests TCPMig parsing.
    #[test]
    fn test_tcpmig_header_parsing() {
        // Build fake IPv4 header.
        let ipv4_hdr: Ipv4Header = ipv4_header();

        // Build fake TCPMig header.
        let origin = SocketAddrV4::from_str("198.0.0.1:20000").unwrap();
        let target = SocketAddrV4::from_str("198.0.0.2:20000").unwrap();
        let remote = SocketAddrV4::from_str("18.45.32.67:19465").unwrap();
        let hdr: [u8; TCPMIG_HEADER_SIZE] = [
            198, 0, 0, 1, 0x4e, 0x20, // origin
            198, 0, 0, 2, 0x4e, 0x20, // target
            18, 45, 32, 67, 0x4c, 0x09, // remote
            0, 8, // payload length
            0b00000010, // flags
            1,
            0xcb, 0x34
        ];

        // Payload.
        let data: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];

        // Input buffer.
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&hdr);
        buf.extend_from_slice(&data);

        match TcpMigHeader::parse_from_slice(&ipv4_hdr, &buf) {
            Ok((hdr, buf)) => {
                assert_eq!(hdr.origin, origin);
                assert_eq!(hdr.target, target);
                assert_eq!(hdr.remote, remote);
                assert_eq!(hdr.payload_length, 8);
                assert_eq!(hdr.flag_load, false);
                assert_eq!(hdr.flag_prepare_migration, true);
                assert_eq!(hdr.flag_ack, false);
                assert_eq!(hdr.flag_payload_state, false);
                assert_eq!(hdr.stage, MigrationStage::PrepareMigration);
                assert_eq!(buf.len(), 8);
            },
            Err(e) => {
                assert!(false, "{:?}", e);
            },
        }
    }
}
