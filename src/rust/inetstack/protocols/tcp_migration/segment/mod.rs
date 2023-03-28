// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod header;
mod defragmenter;

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::{
        ethernet2::Ethernet2Header,
        ipv4::Ipv4Header,
    },
    runtime::{
        memory::{Buffer, DataBuffer},
        network::PacketBuf,
    },
};

//==============================================================================
// Exports
//==============================================================================

pub use self::header::TCPMIG_HEADER_SIZE;
pub use header::TcpMigHeader;
pub use defragmenter::TcpMigDefragmenter;

//==============================================================================
// Structures
//==============================================================================

#[derive(Debug)]
pub struct TcpMigSegment {
    /// Ethernet header.
    ethernet2_hdr: Ethernet2Header,
    /// IPv4 header.
    ipv4_hdr: Ipv4Header,
    /// TCPMig header.
    tcpmig_hdr: TcpMigHeader,
    /// Payload
    data: Buffer,
}

/// A generator of fragments of a [TcpMigSegment].
pub struct TcpMigFragmenter {
    segment: TcpMigSegment,
    max_fragment_size: usize,
    current_fragment: u16,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TcpMigSegment {
    /// Creates a TCPMig packet.
    pub fn new(
        ethernet2_hdr: Ethernet2Header,
        ipv4_hdr: Ipv4Header,
        tcpmig_hdr: TcpMigHeader,
        data: Buffer,
    ) -> Self {
        Self {
            ethernet2_hdr,
            ipv4_hdr,
            tcpmig_hdr,
            data,
        }
    }

    pub fn fragments(self, max_fragment_size: usize) -> TcpMigFragmenter {
        TcpMigFragmenter {
            segment: self,
            max_fragment_size,
            current_fragment: 0,
        }
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// Packet Buffer Trait Implementation for TCPMig Segments
impl PacketBuf for TcpMigSegment {
    /// Computes the header size of the target TCPMig segment.
    fn header_size(&self) -> usize {
        self.ethernet2_hdr.compute_size() + self.ipv4_hdr.compute_size() + self.tcpmig_hdr.size()
    }

    /// Computes the payload size of the target TCPMig segment.
    fn body_size(&self) -> usize {
        self.data.len()
    }

    /// Serializes the header of the target TCPMig segment.
    fn write_header(&self, buf: &mut [u8]) {
        let mut cur_pos: usize = 0;
        let eth_hdr_size: usize = self.ethernet2_hdr.compute_size();
        let tcpmig_hdr_size: usize = self.tcpmig_hdr.size();
        let ipv4_payload_len: usize = tcpmig_hdr_size + self.data.len();

        // Ethernet header.
        self.ethernet2_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + eth_hdr_size)]);
        cur_pos += eth_hdr_size;

        // IPV4 header.
        let ipv4_hdr_size = self.ipv4_hdr.compute_size();
        self.ipv4_hdr
            .serialize(&mut buf[cur_pos..(cur_pos + ipv4_hdr_size)], ipv4_payload_len);
        cur_pos += ipv4_hdr_size;

        // TCPMig header.
        self.tcpmig_hdr.serialize(
            &mut buf[cur_pos..(cur_pos + tcpmig_hdr_size)],
            &self.ipv4_hdr,
            &self.data,
        );
    }

    /// Returns the payload of the target TCPMig segment.
    fn take_body(&self) -> Option<Buffer> {
        Some(self.data.clone())
    }
}

impl Iterator for TcpMigFragmenter {
    type Item = TcpMigSegment;

    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        if self.segment.data.len() == 0 && self.current_fragment != 0 {
            None
        } else {
            let ethernet2_hdr = self.segment.ethernet2_hdr.clone();
            let ipv4_hdr = self.segment.ipv4_hdr.clone();
            let mut tcpmig_hdr = self.segment.tcpmig_hdr.clone();

            let size = if self.segment.data.len() <= self.max_fragment_size {
                tcpmig_hdr.flag_next_fragment = false;
                self.segment.data.len()
            } else {
                tcpmig_hdr.flag_next_fragment = true;
                self.max_fragment_size
            };

            let data = Buffer::Heap(DataBuffer::from_slice(&self.segment.data[0..size]));
            self.segment.data.adjust(size);

            tcpmig_hdr.fragment_offset = self.current_fragment;
            self.current_fragment += 1;

            Some(TcpMigSegment::new(ethernet2_hdr, ipv4_hdr, tcpmig_hdr, data))
        }
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

/* #[cfg(test)]
mod test {
    use super::*;
    use crate::{
        inetstack::protocols::{
            ethernet2::{
                EtherType2,
                ETHERNET2_HEADER_SIZE,
            },
            ip::IpProtocol,
            ipv4::IPV4_HEADER_DEFAULT_SIZE,
        },
        runtime::{
            memory::DataBuffer,
            network::types::MacAddress,
        },
    };
    use ::std::net::Ipv4Addr;
    use std::{net::SocketAddrV4, str::FromStr};

    fn tcpmig_header() -> TcpMigHeader {
        TcpMigHeader {
            origin: SocketAddrV4::from_str("198.0.0.1:20000").unwrap(),
            remote: SocketAddrV4::from_str("18.45.32.67:19465").unwrap(),
            payload_length: 8,
            fragment_offset: 2,
            flag_load: false,
            flag_next_fragment: true,
            stage: super::super::MigrationStage::PrepareMigration,
        }
    }

    #[test]
    fn test_tcpmig_segment_serialization() {
        // Total header size.
        const HEADER_SIZE: usize = ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE + TCPMIG_HEADER_SIZE;

        // Build fake Ethernet2 header.
        let dst_addr: MacAddress = MacAddress::new([0xd, 0xe, 0xa, 0xd, 0x0, 0x0]);
        let src_addr: MacAddress = MacAddress::new([0xb, 0xe, 0xe, 0xf, 0x0, 0x0]);
        let ether_type: EtherType2 = EtherType2::Ipv4;
        let ethernet2_hdr: Ethernet2Header = Ethernet2Header::new(dst_addr, src_addr, ether_type);

        // Build fake Ipv4 header.
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::TCPMig;
        let ipv4_hdr: Ipv4Header = Ipv4Header::new(src_addr, dst_addr, protocol);

        // Build fake TCPMig header.
        let tcpmig_hdr = tcpmig_header();

        // Payload.
        let bytes: [u8; 8] = [0x0, 0x1, 0x0, 0x1, 0x0, 0x1, 0x0, 0x1];
        let data: Buffer = Buffer::Heap(DataBuffer::from_slice(&bytes));

        // Build expected header.
        let mut hdr: [u8; HEADER_SIZE] = [0; HEADER_SIZE];
        ethernet2_hdr.serialize(&mut hdr[0..ETHERNET2_HEADER_SIZE]);
        ipv4_hdr.serialize(
            &mut hdr[ETHERNET2_HEADER_SIZE..(ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE)],
            TCPMIG_HEADER_SIZE + data.len(),
        );
        tcpmig_hdr.serialize(
            &mut hdr[(ETHERNET2_HEADER_SIZE + IPV4_HEADER_DEFAULT_SIZE)..],
            &ipv4_hdr,
            &data,
        );

        // Output buffer.
        let mut buf: [u8; HEADER_SIZE] = [0; HEADER_SIZE];

        let segment = TcpMigSegment::new(ethernet2_hdr, ipv4_hdr, tcpmig_hdr, data);

        // Do it.
        segment.write_header(&mut buf);
        assert_eq!(buf, hdr);
    }

    #[test]
    fn test_tcpmig_fragmentation() {
        // Build fake Ethernet2 header.
        let dst_addr: MacAddress = MacAddress::new([0xd, 0xe, 0xa, 0xd, 0x0, 0x0]);
        let src_addr: MacAddress = MacAddress::new([0xb, 0xe, 0xe, 0xf, 0x0, 0x0]);
        let ether_type: EtherType2 = EtherType2::Ipv4;
        let ethernet2_hdr: Ethernet2Header = Ethernet2Header::new(dst_addr, src_addr, ether_type);

        // Build fake Ipv4 header.
        let src_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 1);
        let dst_addr: Ipv4Addr = Ipv4Addr::new(198, 0, 0, 2);
        let protocol: IpProtocol = IpProtocol::TCPMig;
        let ipv4_hdr: Ipv4Header = Ipv4Header::new(src_addr, dst_addr, protocol);

        // Build fake TCPMig header.
        let tcpmig_hdr = tcpmig_header();

        // Payload.
        let bytes: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let data: Buffer = Buffer::Heap(DataBuffer::from_slice(&bytes));

        let segment = TcpMigSegment::new(ethernet2_hdr, ipv4_hdr, tcpmig_hdr, data);

        let fragments: Vec<TcpMigSegment> = segment.fragments(3).collect();

        use std::ops::Deref;

        assert_eq!(fragments.len(), 3);

        assert!(fragments[0].tcpmig_hdr.flag_next_fragment);
        assert_eq!(fragments[0].tcpmig_hdr.fragment_offset, 0);
        assert_eq!(fragments[0].data.deref(), &[1, 2, 3]);

        assert!(fragments[1].tcpmig_hdr.flag_next_fragment);
        assert_eq!(fragments[1].tcpmig_hdr.fragment_offset, 1);
        assert_eq!(fragments[1].data.deref(), &[4, 5, 6]);

        assert!(!fragments[2].tcpmig_hdr.flag_next_fragment);
        assert_eq!(fragments[2].tcpmig_hdr.fragment_offset, 2);
        assert_eq!(fragments[2].data.deref(), &[7, 8]);
    }
}
 */