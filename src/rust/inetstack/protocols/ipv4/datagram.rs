// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    inetstack::protocols::ip::IpProtocol,
    runtime::{
        fail::Fail,
        memory::Buffer,
    },
};
use ::byteorder::{
    ByteOrder,
    NetworkEndian,
};
use ::libc::{
    EBADMSG,
    ENOTSUP,
};
use ::std::{
    convert::{
        TryFrom,
        TryInto,
    },
    net::Ipv4Addr,
};

//==============================================================================
// Constants
//==============================================================================

/// Default size of IPv4 Headers (in bytes).
pub const IPV4_HEADER_DEFAULT_SIZE: usize = IPV4_DATAGRAM_MIN_SIZE as usize;

/// Minimum size for an IPv4 datagram (in bytes).
const IPV4_DATAGRAM_MIN_SIZE: u16 = 20;

/// Minimum size for an IPv4 datagram (in bytes).
const IPV4_HEADER_MIN_SIZE: u16 = IPV4_DATAGRAM_MIN_SIZE;

/// IPv4 header length when no options are present (in 32-bit words).
const IPV4_IHL_NO_OPTIONS: u8 = (IPV4_HEADER_MIN_SIZE as u8) / 4;

/// Default time to live value.
const DEFAULT_IPV4_TTL: u8 = 255;

/// Version number for IPv4.
const IPV4_VERSION: u8 = 4;

/// IPv4 Control Flag: Datagram has evil intent (see RFC 3514).
const IPV4_CTRL_FLAG_EVIL: u8 = 0x4;

/// IPv4 Control Flag: Don't Fragment.
const IPV4_CTRL_FLAG_DF: u8 = 0x2;

/// IPv4 Control Flag: More Fragments.
const IPV4_CTRL_FLAG_MF: u8 = 0x1;

//==============================================================================
// Structures
//==============================================================================

/// IPv4 Datagram Header
#[derive(Debug, Copy, Clone)]
pub struct Ipv4Header {
    /// Internet header version (4 bits).
    version: u8,
    /// Internet Header Length. (4 bits).
    ihl: u8,
    /// Differentiated Services Code Point (6 bits).
    dscp: u8,
    /// Explicit Congestion Notification (2 bits).
    ecn: u8,
    /// Total length of the packet including header and data (16 bits).
    #[allow(unused)]
    total_length: u16,
    /// Used to identify the datagram to which a fragment belongs (16 bits).
    identification: u16,
    /// Control flags (3 bits).
    flags: u8,
    /// Fragment offset indicates where in the datagram this fragment belongs to (13 bits).
    fragment_offset: u16,
    /// Time to Live indicates the maximum remaining time the datagram is allowed to be in the network (8 bits).
    ttl: u8,
    /// Protocol used in the data portion of the datagram (8 bits).
    protocol: IpProtocol,
    /// Header-only checksum for error detection (16 bits).
    #[allow(unused)]
    header_checksum: u16,
    // Source IP address (32 bits).
    src_addr: Ipv4Addr,
    /// Destination IP address (32 bits).
    dst_addr: Ipv4Addr,
}

//==============================================================================
// Associated Functions
//==============================================================================

/// Associated Functions for IPv4 Headers
impl Ipv4Header {
    /// Instantiates an empty IPv4 header.
    pub fn new(src_addr: Ipv4Addr, dst_addr: Ipv4Addr, protocol: IpProtocol) -> Self {
        Self {
            version: IPV4_VERSION,
            ihl: IPV4_IHL_NO_OPTIONS,
            dscp: 0,
            ecn: 0,
            total_length: IPV4_HEADER_MIN_SIZE,
            identification: 0,
            flags: IPV4_CTRL_FLAG_DF,
            fragment_offset: 0,
            ttl: DEFAULT_IPV4_TTL,
            protocol,
            header_checksum: 0,
            src_addr,
            dst_addr,
        }
    }

    /// Computes the size of the target IPv4 header.
    pub fn compute_size(&self) -> usize {
        IPV4_HEADER_MIN_SIZE as usize
    }

    /// Parses a buffer into an IPv4 header and payload.
    pub fn parse(mut buf: Buffer) -> Result<(Self, Buffer), Fail> {
        // The datagram should be as big as the header.
        if buf.len() < (IPV4_DATAGRAM_MIN_SIZE as usize) {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too small"));
        }

        // IP version number.
        let version: u8 = buf[0] >> 4;
        if version != IPV4_VERSION {
            return Err(Fail::new(ENOTSUP, "unsupported IP version"));
        }

        // Internet header length.
        let ihl: u8 = buf[0] & 0xF;
        let hdr_size: usize = (ihl as usize) << 2;
        if hdr_size < (IPV4_HEADER_MIN_SIZE as usize) {
            return Err(Fail::new(EBADMSG, "ipv4 IHL is too small"));
        }
        if buf.len() < hdr_size {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too small to fit in header"));
        }
        let hdr_buf: &[u8] = &buf[..hdr_size];

        // Differentiated services code point.
        let dscp: u8 = hdr_buf[1] >> 2;
        if dscp != 0 {
            warn!("ignoring dscp field (dscp={:?})", dscp);
        }

        // Explicit congestion notification.
        let ecn: u8 = hdr_buf[1] & 3;
        if ecn != 0 {
            warn!("ignoring ecn field (ecn={:?})", ecn);
        }

        // Total length.
        let total_length: u16 = NetworkEndian::read_u16(&hdr_buf[2..4]);
        if (total_length as usize) < hdr_size {
            return Err(Fail::new(EBADMSG, "ipv4 datagram smaller than header"));
        }
        // NOTE: there may be padding bytes in the buffer.
        if (total_length as usize) > buf.len() {
            return Err(Fail::new(EBADMSG, "ipv4 datagram size mismatch"));
        }

        // Identification (Id).
        //
        // Note: We had a (now removed) bug here in that we were _requiring_ all incoming datagrams to have an Id field
        // of zero.  This was horribly misguided.  With the exception of datagramss where the DF (don't fragment) flag
        // is set, all IPv4 datagrams are _required_ to have a (temporally) unique identification field for datagrams
        // with the same source, destination, and protocol.  Thus we should expect most datagrams to have a non-zero Id.
        let identification: u16 = NetworkEndian::read_u16(&hdr_buf[4..6]);

        // Control flags.
        //
        // Note: We had a (now removed) bug here in that we were _requiring_ all incoming datagrams to have the DF
        // (don't fragment) bit set.  This appears to be because we don't support fragmentation (yet anyway).  But the
        // lack of a set DF bit doesn't make a datagram a fragment.  So we should accept datagrams regardless of the
        // setting of this bit.
        let flags: u8 = hdr_buf[6] >> 5;
        // Don't accept evil datagrams (see RFC 3514).
        if flags & IPV4_CTRL_FLAG_EVIL != 0 {
            return Err(Fail::new(EBADMSG, "ipv4 datagram is marked as evil"));
        }

        // TODO: drop this check once we support fragmentation.
        if flags & IPV4_CTRL_FLAG_MF != 0 {
            warn!("fragmentation is not supported flags={:?}", flags);
            return Err(Fail::new(ENOTSUP, "ipv4 fragmentation is not supported"));
        }

        // Fragment offset.
        let fragment_offset: u16 = NetworkEndian::read_u16(&hdr_buf[6..8]) & 0x1fff;
        // TODO: drop this check once we support fragmentation.
        if fragment_offset != 0 {
            warn!("fragmentation is not supported offset={:?}", fragment_offset);
            return Err(Fail::new(ENOTSUP, "ipv4 fragmentation is not supported"));
        }

        // Time to live.
        let time_to_live: u8 = hdr_buf[8];
        if time_to_live == 0 {
            return Err(Fail::new(EBADMSG, "ipv4 datagram too old"));
        }

        // Protocol.
        let protocol: IpProtocol = IpProtocol::try_from(hdr_buf[9])?;

        // Header checksum.
        let header_checksum: u16 = NetworkEndian::read_u16(&hdr_buf[10..12]);
        if header_checksum == 0xffff {
            return Err(Fail::new(EBADMSG, "ipv4 checksum invalid"));
        }
        if header_checksum != Self::compute_checksum(hdr_buf) {
            return Err(Fail::new(EBADMSG, "ipv4 checksum mismatch"));
        }

        // Source address.
        let src_addr: Ipv4Addr = Ipv4Addr::from(NetworkEndian::read_u32(&hdr_buf[12..16]));

        // Destination address.
        let dst_addr: Ipv4Addr = Ipv4Addr::from(NetworkEndian::read_u32(&hdr_buf[16..20]));

        // Truncate datagram.
        let padding_bytes: usize = buf.len() - total_length as usize;
        buf.adjust(hdr_size);
        buf.trim(padding_bytes);

        let header: Ipv4Header = Self {
            version,
            ihl,
            dscp,
            ecn,
            total_length,
            identification,
            flags,
            fragment_offset,
            ttl: time_to_live,
            protocol,
            header_checksum,
            src_addr,
            dst_addr,
        };

        Ok((header, buf))
    }

    /// Serializes the target IPv4 header.
    pub fn serialize(&self, buf: &mut [u8], payload_len: usize) {
        let buf: &mut [u8; (IPV4_HEADER_MIN_SIZE as usize)] = buf.try_into().expect("buffer to small");

        // Version + IHL.
        buf[0] = (self.version << 4) | self.ihl;

        // DSCP + ECN.
        buf[1] = (self.dscp << 2) | (self.ecn & 3);

        // Total Length.
        NetworkEndian::write_u16(&mut buf[2..4], IPV4_HEADER_MIN_SIZE + (payload_len as u16));

        // Identification.
        NetworkEndian::write_u16(&mut buf[4..6], self.identification);

        // Flags and Fragment Offset.
        NetworkEndian::write_u16(
            &mut buf[6..8],
            (self.flags as u16) << 13 | self.fragment_offset & 0x1fff,
        );

        // Time to Live.
        buf[8] = self.ttl;

        // Protocol.
        buf[9] = self.protocol as u8;

        // Skip the checksum (bytes 10..12) until we finish writing the header.

        // Source Address.
        buf[12..16].copy_from_slice(&self.src_addr.octets());

        // Destination Address.
        buf[16..20].copy_from_slice(&self.dst_addr.octets());

        // Header Checksum.
        let checksum: u16 = Self::compute_checksum(buf);
        NetworkEndian::write_u16(&mut buf[10..12], checksum);
    }

    /// Returns the source address field stored in the target IPv4 header.
    pub fn get_src_addr(&self) -> Ipv4Addr {
        self.src_addr
    }

    /// Returns the destination address field stored in the target IPv4 header.
    pub fn get_dest_addr(&self) -> Ipv4Addr {
        self.dst_addr
    }

    /// Returns the protocol field stored in the target IPv4 header.
    pub fn get_protocol(&self) -> IpProtocol {
        self.protocol
    }

    /// Computes the checksum of the target IPv4 header.
    pub fn compute_checksum(buf: &[u8]) -> u16 {
        let mut state: u32 = 0xffffu32;
        for i in 0..5 {
            state += NetworkEndian::read_u16(&buf[(2 * i)..(2 * i + 2)]) as u32;
        }
        // Skip the 5th u16 since octets 10-12 are the header checksum, whose value should be zero when
        // computing a checksum.
        for i in 6..10 {
            state += NetworkEndian::read_u16(&buf[(2 * i)..(2 * i + 2)]) as u32;
        }
        while state > 0xffff {
            state -= 0xffff;
        }
        !state as u16
    }

    #[cfg(feature = "capybara-switch")]
    pub fn set_dest_addr(&mut self, addr: Ipv4Addr) {
        self.dst_addr = addr;
    }
}
