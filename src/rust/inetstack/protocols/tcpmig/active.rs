// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    constants::{SWITCH_IP, SWITCH_MAC},
    peer::user_connection::MigrateOut,
    segment::{
        MigrationStage,
        TcpMigDefragmenter,
        TcpMigHeader,
        TcpMigSegment,
    },
    TcpmigReceiveStatus,
};
use crate::{
    capy_profile,
    capy_profile_merge_previous,
    capy_time_log,
    inetstack::protocols::{
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::IpProtocol,
        ipv4::Ipv4Header,
        tcp::{
            peer::{
                state::TcpState,
                TcpMigContext,
            },
            segment::TcpHeader,
        },
        tcpmig::segment::MAX_FRAGMENT_SIZE,
    },
    runtime::{
        fail::Fail,
        memory::{
            Buffer,
            DataBuffer,
        },
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    },
    QDesc,
};

use crate::capy_log_mig;

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

    /// QDesc representing the connection, only on the origin side.
    qd: Option<QDesc>,

    recv_queue: Vec<(TcpHeader, Buffer)>,

    defragmenter: TcpMigDefragmenter,

    configured_state_size: usize,

    mtu: usize,
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
        mtu: usize,
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
            qd,
            recv_queue: Vec::new(),
            defragmenter: TcpMigDefragmenter::new(),
            configured_state_size: std::env::var("CONFIGURED_STATE_SIZE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0), // Use 0 as the default value if parsing or fetching fails
            mtu,
        }
    }

    pub fn process_packet(
        &mut self,
        ipv4_hdr: &Ipv4Header,
        hdr: TcpMigHeader,
        buf: Buffer,
        ctx: TcpMigContext,
    ) -> Result<TcpmigReceiveStatus, Fail> {
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
                        // capy_profile_merge_previous!("prepare_ack");

                        // Decide if migration should be accepted or not and send corresponding segment.
                        // if ctx.is_under_load {
                        //     let mut hdr = next_header(hdr, MigrationStage::Rejected);
                        //     self.last_sent_stage = MigrationStage::Rejected;
                        //     capy_log_mig!("[TX] MIG_REJECTED ({}, {})", hdr.origin, hdr.client);
                        //     capy_time_log!("SEND_MIG_REJECTED,({})", hdr.client);
                        //     self.send(hdr, empty_buffer());
                        //     return Ok(TcpmigReceiveStatus::SentReject);
                        // } else {
                        let mut hdr = next_header(hdr, MigrationStage::PrepareMigrationAck);
                        hdr.flag_load = true;
                        self.last_sent_stage = MigrationStage::PrepareMigrationAck;
                        capy_log_mig!("[TX] PREPARE_MIG_ACK ({}, {})", hdr.origin, hdr.client);
                        capy_time_log!("SEND_PREPARE_MIG_ACK,({}),[{}:{}->{}]", hdr.client, self.local_ipv4_addr, self.self_udp_port, hdr.origin);
                        self.send(hdr, empty_buffer());
                        return Ok(TcpmigReceiveStatus::Ok);
                        // }
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION")),
                }
            },

            // Expect ACK and mark as prepared for migration. Handle failure if REJECTED.
            MigrationStage::PrepareMigration => {
                match hdr.stage {
                    MigrationStage::PrepareMigrationAck => {
                        // Change target address to actual target address.
                        self.remote_ipv4_addr = ipv4_hdr.get_src_addr();
                        self.dest_udp_port = hdr.get_source_udp_port();
                        capy_time_log!("RECV_PREPARE_MIG_ACK,({}),[{}:{}->{}:{}]", hdr.client, self.remote_ipv4_addr, self.dest_udp_port, self.local_ipv4_addr, self.self_udp_port);

                        capy_log_mig!("PREPARE_MIG_ACK => ({}, {}) is PREPARED", hdr.origin, hdr.client);
                        return Ok(TcpmigReceiveStatus::PrepareMigrationAcked(
                            self.qd.expect("no qd on origin side"),
                        ));
                    },
                    MigrationStage::Rejected => {
                        capy_time_log!("RECV_REJECTED,({})", hdr.client);
                        return Ok(TcpmigReceiveStatus::Rejected(hdr.origin, hdr.client));
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected PREPARE_MIGRATION_ACK or REJECTED")),
                }
            },

            // Expect CONNECTION_STATE and send ACK.
            MigrationStage::PrepareMigrationAck => {
                match hdr.stage {
                    MigrationStage::ConnectionState => {
                        // capy_profile!("migrate_ack");

                        // Handle fragmentation.
                        let (hdr, mut buf) = match self.defragmenter.defragment(hdr, buf) {
                            Some((hdr, buf)) => (hdr, buf),
                            None => {
                                capy_log_mig!("Receiving CONN_STATE fragments...");
                                return Ok(TcpmigReceiveStatus::Ok);
                            },
                        };
                        capy_time_log!("RECV_STATE,({}),[{}->{}:{}]", self.client, hdr.origin, self.local_ipv4_addr, self.self_udp_port);
                        buf.trim(self.configured_state_size);

                        // let mut state = TcpState::deserialize(buf);
                        let mut state = TcpState::deserialize_from(&mut buf);

                        // Overwrite local address.
                        state.set_local(SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port));

                        // eprintln!("*** Received state ***");

                        // ACK CONNECTION_STATE.
                        let hdr = next_header(hdr, MigrationStage::ConnectionStateAck);
                        self.last_sent_stage = MigrationStage::ConnectionStateAck;
                        capy_log_mig!(
                            "[TX] CONN_STATE_ACK ({}, {}) to {}:{}",
                            self.origin,
                            self.client,
                            self.remote_ipv4_addr,
                            self.dest_udp_port
                        );
                        // capy_time_log!("SEND_STATE_ACK,({})", self.client);
                        self.send(hdr, empty_buffer());

                        // Take the buffered packets.
                        // let pkts = std::mem::take(&mut self.recv_queue);

                        return Ok(TcpmigReceiveStatus::StateReceived(state, Some(buf)));
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE")),
                }
            },

            // Expect ACK and send FIN.
            MigrationStage::ConnectionState => {
                match hdr.stage {
                    MigrationStage::ConnectionStateAck => {
                        // capy_time_log!("RECV_STATE_ACK,({})", self.client);
                        capy_log_mig!("CONN_STATE_ACK for ({}, {})", self.origin, self.client);
                        // TODO: Start closing the active migration.
                        return Ok(TcpmigReceiveStatus::MigrationCompleted);
                    },
                    _ => return Err(Fail::new(libc::EBADMSG, "expected CONNECTION_STATE_ACK")),
                }
            },

            // Expect FIN and close active migration.
            MigrationStage::ConnectionStateAck => {
                // TODO: Close active migration.
                capy_log_mig!("Received someting after sending CONN_STATE_ACK");
            },

            MigrationStage::Rejected => panic!("Target should not receive a packet after rejecting origin."),
            MigrationStage::HeartbeatUpdate | MigrationStage::HeartbeatResponse => {
                panic!("Heartbeat is not an active migration")
            },
        };
        Ok(TcpmigReceiveStatus::Ok)
    }

    pub fn initiate_migration(&mut self) {
        assert_eq!(self.last_sent_stage, MigrationStage::None);

        let tcpmig_hdr = TcpMigHeader::new(
            self.origin,
            self.client,
            0,
            MigrationStage::PrepareMigration,
            self.self_udp_port,
            self.dest_udp_port,
            // if self.self_udp_port == 10001 { 10000 } else { 10001 }, 
            // 10000, 
        );
        self.last_sent_stage = MigrationStage::PrepareMigration;
        capy_log_mig!(
            "\n\n******* START MIGRATION *******\n[TX] PREPARE_MIG ({}, {})",
            self.origin,
            self.client
        );
        capy_time_log!("SEND_PREPARE_MIG,({}),[{}->{}:{}](via switch)", self.client, self.origin, self.remote_ipv4_addr, self.dest_udp_port);
        self.send_via_switch(tcpmig_hdr, Buffer::Heap(DataBuffer::empty()));
    }

    pub fn send_connection_state(&mut self, state: TcpState, user_data: MigrateOut) {
        // capy_time_log!("SERIALIZE_STATE,({})", self.client);
        assert_eq!(self.last_sent_stage, MigrationStage::PrepareMigration);

        capy_log_mig!(
            "[TX] CONNECTION_STATE: ({}, {}) to {}:{}",
            self.origin,
            self.client,
            self.remote_ipv4_addr,
            self.dest_udp_port
        );
        // print the length of recv_queue here
        capy_log_mig!("Length of recv_queue: {}", state.recv_queue_len());

        // let buf = state.serialize();
        let buf_size = state.serialized_size() + user_data.serialized_size() + self.configured_state_size;
        let mut buf = Buffer::Heap(DataBuffer::new(buf_size).expect("can allocate buffer"));
        let remaining_buf = state.serialize_into(&mut buf);
        user_data.serialize(remaining_buf);

        let tcpmig_hdr = TcpMigHeader::new(
            self.origin,
            self.client,
            0,
            MigrationStage::ConnectionState,
            self.self_udp_port,
            self.dest_udp_port,
        ); // PORT should be the sender of PREPARE_MIGRATION_ACK

        self.last_sent_stage = MigrationStage::ConnectionState;
        let log_dest_ip = if self.remote_ipv4_addr == SWITCH_IP { self.local_ipv4_addr } else { self.remote_ipv4_addr };
        capy_time_log!("SEND_STATE,({}),[{}->{}:{}]", self.client, self.origin, log_dest_ip, self.dest_udp_port);
        self.send(tcpmig_hdr, buf);
    }

    pub fn buffer_packet(&mut self, tcp_hdr: TcpHeader, data: Buffer) {
        self.recv_queue.push((tcp_hdr, data));
    }

    pub fn take_buffered_packets(&mut self) -> Vec<(TcpHeader, Buffer)> {
        std::mem::take(&mut self.recv_queue)
    }

    /// Sends a TCPMig segment from local to remote.
    /// If `via_switch` is true, always send to switch (for PREPARE_MIG).
    /// Otherwise, send directly to target, but route via switch if target IP == local IP.
    fn send_with_routing(&self, tcpmig_hdr: TcpMigHeader, buf: Buffer, via_switch: bool) {
        debug!("TCPMig send {:?}", tcpmig_hdr);

        let (dst_ip, dst_mac) = if via_switch {
            // PREPARE_MIG: Always send to switch
            capy_log_mig!("[SEND] PREPARE_MIG via switch: {} -> {}", self.remote_ipv4_addr, SWITCH_IP);
            (SWITCH_IP, SWITCH_MAC)
        } else if self.remote_ipv4_addr == self.local_ipv4_addr {
            // Same IP: route via switch
            capy_log_mig!("[SEND] Same IP, routing via switch: {} -> {}", self.remote_ipv4_addr, SWITCH_IP);
            (SWITCH_IP, SWITCH_MAC)
        } else {
            // Different IP: send directly
            (self.remote_ipv4_addr, self.remote_link_addr)
        };

        capy_time_log!("MIG_SEND: stage={:?}, client={}, dst_ip={}", tcpmig_hdr.stage, tcpmig_hdr.client, dst_ip);

        let ip_hdr = Ipv4Header::new(self.local_ipv4_addr, dst_ip, IpProtocol::UDP);

        if buf.len() / MAX_FRAGMENT_SIZE > u16::MAX as usize {
            panic!("TcpState too large")
        }
        let segment = TcpMigSegment::new(
            Ethernet2Header::new(dst_mac, self.local_link_addr, EtherType2::Ipv4),
            ip_hdr,
            tcpmig_hdr,
            buf,
            self.mtu
        );
        for fragment in segment.fragments() {
            self.rt.transmit(Box::new(fragment));
        }
    }

    /// Send via switch (for PREPARE_MIG)
    fn send_via_switch(&self, tcpmig_hdr: TcpMigHeader, buf: Buffer) {
        self.send_with_routing(tcpmig_hdr, buf, true);
    }

    /// Send directly (or via switch if same IP)
    fn send(&self, tcpmig_hdr: TcpMigHeader, buf: Buffer) {
        self.send_with_routing(tcpmig_hdr, buf, false);
    }
}

//======================================================================================================================
// Functions
//======================================================================================================================
