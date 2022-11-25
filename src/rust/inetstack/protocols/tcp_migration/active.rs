// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{segment::{
    TcpMigHeader,
    TcpMigSegment,
}, MigrationStage};
use crate::{
    inetstack::protocols::{
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ip::IpProtocol,
            ipv4::Ipv4Header, tcp::{migration::TcpState, segment::TcpHeader, TcpPeer},
        },
    runtime::{
        fail::Fail,
        memory::{Buffer, DataBuffer},
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    },
};
use std::collections::VecDeque;
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
};

#[cfg(feature = "profiler")]
use crate::timer;

//======================================================================================================================
// Constants
//======================================================================================================================



//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(PartialEq, Eq)]
pub enum MigrationRequestStatus {
    Accepted,
    Rejected,
}

pub struct ActiveMigration {
    rt: Rc<dyn NetworkRuntime>,

    local_ipv4_addr: Ipv4Addr,
    local_link_addr: MacAddress,
    remote_ipv4_addr: Ipv4Addr,
    remote_link_addr: MacAddress,

    origin: SocketAddrV4,
    remote: SocketAddrV4,

    last_sent_stage: MigrationStage,
    is_prepared: bool,

    recv_queue: VecDeque<(Ipv4Header, TcpHeader, Buffer)>,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

impl ActiveMigration {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        remote_ipv4_addr: Ipv4Addr,
        remote_link_addr: MacAddress,
        origin: SocketAddrV4,
        remote: SocketAddrV4,
    ) -> Self {
        Self {
            rt,
            local_ipv4_addr,
            local_link_addr,
            remote_ipv4_addr,
            remote_link_addr,
            origin,
            remote,
            last_sent_stage: MigrationStage::None,
            is_prepared: false,
            recv_queue: VecDeque::new(),
        }
    }

    pub fn is_prepared(&self) -> bool {
        self.is_prepared
    }

    pub fn process_packet(&mut self, tcp_peer: &mut TcpPeer, hdr: TcpMigHeader, buf: Buffer) -> Result<MigrationRequestStatus, Fail> {
        #[inline]
        fn next_header(mut hdr: TcpMigHeader, next_stage: MigrationStage) -> TcpMigHeader {
            hdr.stage = next_stage;
            hdr
        }

        #[inline]
        fn empty_buffer() -> Buffer { Buffer::Heap(DataBuffer::empty()) }

        match self.last_sent_stage {
            // Target

            // Expect PREPARE_MIGRATION and send ACK.
            MigrationStage::None => {
                match hdr.stage {
                    MigrationStage::PrepareMigration => {
                        // Decide if migration should be accepted or not and send corresponding segment.

                        // Assume always accept for now.
                        let mut hdr = next_header(hdr, MigrationStage::PrepareMigrationAck);
                        hdr.flag_load = true;
                        self.last_sent_stage = MigrationStage::PrepareMigrationAck;
                        self.send(hdr, empty_buffer());
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION"))
                }
            },

            // Expect CONNECTION_STATE and send ACK.
            MigrationStage::PrepareMigrationAck => {
                match hdr.stage {
                    MigrationStage::ConnectionState => {
                        let mut state = match TcpState::deserialize(&buf) {
                            Ok(state) => state,
                            Err(..) => return Err(Fail::new(libc::EBADMSG, "invalid TCP state")),
                        };

                        // Overwrite local address.
                        state.local = SocketAddrV4::new(self.local_ipv4_addr, self.origin.port());

                        // Migrate in TCP state, allocate FD, allow accept() to return new FD.
                        eprintln!("*** Migrating in state ***");

                        // ACK CONNECTION_STATE.
                        let hdr = next_header(hdr, MigrationStage::ConnectionStateAck);
                        self.last_sent_stage = MigrationStage::ConnectionStateAck;
                        self.send(hdr, empty_buffer());
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE"))
                }
            },

            // Expect FIN and close active migration. 
            MigrationStage::ConnectionStateAck => {
                // TODO: Close active migration.
            },

            MigrationStage::Rejected => unreachable!("Target should not receive a packet after rejecting origin."),

            // Origin

            // Expect ACK and send CONNECTION_STATE. Handle failure if REJECTED.
            MigrationStage::PrepareMigration => {
                match hdr.stage {
                    MigrationStage::PrepareMigrationAck => {
                        // Mark migration as prepared so that it can be migrated out at the next decision point.
                        self.is_prepared = true;
                    },
                    MigrationStage::Rejected => {
                        return Ok(MigrationRequestStatus::Rejected);
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION_ACK or REJECTED"))
                }
            },

            // Expect ACK and send FIN.
            MigrationStage::ConnectionState => {
                match hdr.stage {
                    MigrationStage::ConnectionStateAck => {
                        eprintln!("*** Migration completed ***");

                        // TODO: Start closing the active migration.
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE_ACK"))
                }
            },
        };
        Ok(MigrationRequestStatus::Accepted)
    }

    pub fn initiate_migration(&mut self) {
        assert_eq!(self.last_sent_stage, MigrationStage::None);

        let tcpmig_hdr = TcpMigHeader::new(self.origin, self.remote, 0, MigrationStage::PrepareMigration);
        self.last_sent_stage = MigrationStage::PrepareMigration;
        self.send(tcpmig_hdr, Buffer::Heap(DataBuffer::empty()));
    }

    pub fn send_connection_state(&mut self, state: TcpState) {
        assert_eq!(self.last_sent_stage, MigrationStage::PrepareMigration);

        let buf = match state.serialize() {
            Ok(buf) => buf,
            Err(e) => panic!("TCPState serialisation failed: {}", e),
        };

        let tcpmig_hdr = TcpMigHeader::new(self.origin, self.remote, 0, MigrationStage::ConnectionState);
        self.last_sent_stage = MigrationStage::ConnectionState;
        self.send(tcpmig_hdr, Buffer::Heap(DataBuffer::from_slice(&buf)));
    }

    pub fn buffer_packet(&mut self, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: Buffer) {
        self.recv_queue.push_back((ip_hdr, tcp_hdr, buf));
    }

    /// Sends a TCPMig segment from local to remote.
    fn send(
        &self,
        tcpmig_hdr: TcpMigHeader,
        buf: Buffer,
    ) {
        debug!("TCPMig send {:?}", tcpmig_hdr);

        let segment = TcpMigSegment::new(
            Ethernet2Header::new(self.remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(self.local_ipv4_addr, self.remote_ipv4_addr, IpProtocol::TCPMig),
            tcpmig_hdr,
            buf,
        );
        eprintln!("TCPMig sent: {:#?}", segment);
        self.rt.transmit(Box::new(segment));
    }
}