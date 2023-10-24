// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{segment::{
    TcpMigHeader,
    TcpMigSegment, TcpMigDefragmenter,
}, MigrationStage};
use crate::{
    inetstack::protocols::{
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ip::IpProtocol,
            ipv4::Ipv4Header, tcp::{segment::TcpHeader, peer::TcpState},
        },
    runtime::{
        fail::Fail,
        memory::{Buffer, DataBuffer},
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    },
    QDesc,
    capy_profile, capy_profile_merge_previous,
};

use crate::{capy_log, capy_log_mig};

use std::sync::Arc;
use ::std::{
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
};

//======================================================================================================================
// Structures
//======================================================================================================================

#[derive(PartialEq, Eq)]
pub enum MigrationRequestStatus {
    Ok,
    Rejected,
    PrepareMigrationAcked(QDesc),
    StateReceived(TcpState),
    MigrationCompleted,
}

pub struct ActiveMigration {
    rt: Rc<dyn NetworkRuntime>,

    local_ipv4_addr: Ipv4Addr,
    local_link_addr: MacAddress,
    remote_ipv4_addr: Ipv4Addr,
    remote_link_addr: MacAddress,
    self_udp_port: u16,
    dest_udp_port: u16,

    origin: SocketAddrV4,
    client: SocketAddrV4,

    last_sent_stage: MigrationStage,
    is_prepared: bool,

    /// QDesc representing the connection, only on the origin side.
    qd: Option<QDesc>,

    pub recv_queue: Vec<(TcpHeader, Buffer)>,

    defragmenter: TcpMigDefragmenter,
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
        self_udp_port: u16,
        dest_udp_port: u16,
        origin: SocketAddrV4,
        client: SocketAddrV4,
        qd: Option<QDesc>,
    ) -> Self {
        Self {
            rt,
            local_ipv4_addr,
            local_link_addr,
            remote_ipv4_addr,
            remote_link_addr,
            self_udp_port,
            dest_udp_port, 
            origin,
            client,
            last_sent_stage: MigrationStage::None,
            is_prepared: false,
            qd,
            recv_queue: Vec::new(),
            defragmenter: TcpMigDefragmenter::new(),
        }
    }

    pub fn is_prepared(&self) -> bool {
        self.is_prepared
    }

    pub fn qd(&self) -> Option<QDesc> {
        self.qd
    }

    pub fn process_packet(&mut self, ipv4_hdr: &Ipv4Header, hdr: TcpMigHeader, buf: Buffer) -> Result<MigrationRequestStatus, Fail> {
        #[inline]
        fn next_header(mut hdr: TcpMigHeader, next_stage: MigrationStage) -> TcpMigHeader {
            hdr.stage = next_stage;
            hdr.swap_src_dst_port();
            hdr
        }

        #[inline]
        fn empty_buffer() -> Buffer {
            Buffer::Heap(DataBuffer::empty())
        }

        match self.last_sent_stage {
            // Expect PREPARE_MIGRATION and send ACK.
            MigrationStage::None => {
                match hdr.stage {
                    MigrationStage::PrepareMigration => {
                        capy_profile_merge_previous!("prepare_ack");

                        // Decide if migration should be accepted or not and send corresponding segment.

                        // Assume always accept for now.
                        let mut hdr = next_header(hdr, MigrationStage::PrepareMigrationAck);
                        hdr.flag_load = true;
                        self.last_sent_stage = MigrationStage::PrepareMigrationAck;
                        capy_log_mig!("[TX] PREPARE_MIG_ACK ({}, {})", hdr.origin, hdr.client);
                        self.send(hdr, empty_buffer());
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION"))
                }
            },

            // Expect ACK and mark as prepared for migration. Handle failure if REJECTED.
            MigrationStage::PrepareMigration => {
                match hdr.stage {
                    MigrationStage::PrepareMigrationAck => {
                        // Change target address to actual target address.
                        /*  self.remote_ipv4_addr = ipv4_hdr.get_src_addr(); */
                        // Currently, we are running all backends on a single machine, 
                        // so we send the migration messages to the switch first, 
                        // and let the switch do the addressing to the backend.
                        // Later if backends are on different machines, we need to uncomment this line again.
                        self.dest_udp_port = hdr.get_source_udp_port();

                        // Mark migration as prepared so that it can be migrated out at the next decision point.
                        self.is_prepared = true;
                        capy_log_mig!("PREPARE_MIG_ACK => ({}, {}) is PREPARED", hdr.origin, hdr.client);
                        return Ok(MigrationRequestStatus::PrepareMigrationAcked(self.qd.expect("no qd on origin side")));
                    },
                    MigrationStage::Rejected => {
                        capy_log_mig!("REJECTED");
                        return Ok(MigrationRequestStatus::Rejected);
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION_ACK or REJECTED"))
                }
            },

            // Expect CONNECTION_STATE and send ACK.
            MigrationStage::PrepareMigrationAck => {
                match hdr.stage {
                    MigrationStage::ConnectionState => {
                        capy_profile!("migrate_ack");

                        // Handle fragmentation.
                        let (hdr, buf) = match self.defragmenter.defragment(hdr, buf) {
                            Some((hdr, buf)) => (hdr, buf),
                            None => {
                                capy_log_mig!("Receiving CONN_STATE fragments...");
                                return Ok(MigrationRequestStatus::Ok)
                            },
                        };

                        let mut state = match TcpState::deserialize(&buf) {
                            Ok(state) => state,
                            Err(..) => return Err(Fail::new(libc::EBADMSG, "invalid TCP state")),
                        };

                        // Overwrite local address.
                        state.local = SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port);

                        // eprintln!("*** Received state ***");

                        // ACK CONNECTION_STATE.
                        let hdr = next_header(hdr, MigrationStage::ConnectionStateAck);
                        self.last_sent_stage = MigrationStage::ConnectionStateAck;
                        capy_log_mig!("[TX] CONN_STATE_ACK ({}, {}) to {}:{}", self.origin, self.client, self.remote_ipv4_addr, self.dest_udp_port);
                        self.send(hdr, empty_buffer());
                        
                        return Ok(MigrationRequestStatus::StateReceived(state));
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE"))
                }
            },

            // Expect ACK and send FIN.
            MigrationStage::ConnectionState => {
                match hdr.stage {
                    MigrationStage::ConnectionStateAck => {
                        // eprintln!("*** Migration completed ***");
                        capy_log_mig!("CONN_STATE_ACK for ({}, {})", self.origin, self.client);
                        // TODO: Start closing the active migration.
                        return Ok(MigrationRequestStatus::MigrationCompleted);
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE_ACK"))
                }
            },

            // Expect FIN and close active migration.
            MigrationStage::ConnectionStateAck => {
                // TODO: Close active migration.
                capy_log_mig!("Received someting after sending CONN_STATE_ACK");
            },

            MigrationStage::Rejected => unreachable!("Target should not receive a packet after rejecting origin."),
        };
        Ok(MigrationRequestStatus::Ok)
    }

    pub fn initiate_migration(&mut self) {
        assert_eq!(self.last_sent_stage, MigrationStage::None);

        let tcpmig_hdr = TcpMigHeader::new(self.origin, self.client, 
                                                        0, 
                                                        MigrationStage::PrepareMigration, 
                                                        self.self_udp_port, 
                                                        if self.self_udp_port == 10001 { 10000 } else { 10001 });
        self.last_sent_stage = MigrationStage::PrepareMigration;
        capy_log_mig!("\n\n******* START MIGRATION *******\n[TX] PREPARE_MIG ({}, {})", self.origin, self.client);
        self.send(tcpmig_hdr, Buffer::Heap(DataBuffer::empty()));
    }

    pub fn send_connection_state(&mut self, state: TcpState) {
        assert_eq!(self.last_sent_stage, MigrationStage::PrepareMigration);

        capy_log_mig!("[TX] CONNECTION_STATE: ({}, {}) to {}:{}", self.origin, self.client, self.remote_ipv4_addr, self.dest_udp_port);
        // print the length of recv_queue here
        capy_log_mig!("Length of recv_queue: {}", state.recv_queue.len());
        
        let buf = state.serialize();
        let buf_len = buf.len();
        let tcpmig_hdr = TcpMigHeader::new(self.origin, self.client, 
                                                        0, 
                                                        MigrationStage::ConnectionState, 
                                                        self.self_udp_port, 
                                                        self.dest_udp_port); // PORT should be the sender of PREPARE_MIGRATION_ACK
        self.last_sent_stage = MigrationStage::ConnectionState;
        let data_buffer = DataBuffer::from_raw_parts(Arc::into_raw(buf) as *mut u8, buf_len).unwrap(); 
        self.send(tcpmig_hdr, Buffer::Heap(data_buffer));
    }

    pub fn buffer_packet(&mut self, tcp_hdr: TcpHeader, data: Buffer) {
        self.recv_queue.push((tcp_hdr, data));
    }

    /// Sends a TCPMig segment from local to remote.
    fn send(
        &self,
        tcpmig_hdr: TcpMigHeader,
        buf: Buffer,
    ) {
        debug!("TCPMig send {:?}", tcpmig_hdr);
        // eprintln!("TCPMig sent: {:#?}\nto {:?}:{:?}", tcpmig_hdr, self.remote_link_addr, self.remote_ipv4_addr);
        
        // Layer 4 protocol field marked as UDP because DPDK only supports standard Layer 4 protocols.
        let ip_hdr = Ipv4Header::new(self.local_ipv4_addr, self.remote_ipv4_addr, IpProtocol::UDP);

        const MTU: usize = 1500; // TEMP
        let max_fragment_size =  MTU - ip_hdr.compute_size() - tcpmig_hdr.size();
        if buf.len() / max_fragment_size > u16::MAX as usize {
            todo!("Graceful rejection of migration");
        }
        let segment = TcpMigSegment::new(
            Ethernet2Header::new(self.remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ip_hdr,
            tcpmig_hdr,
            buf,
        );
        for fragment in segment.fragments(max_fragment_size) {
            self.rt.transmit(Box::new(fragment));
        }
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================
