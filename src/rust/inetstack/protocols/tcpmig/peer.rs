// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::constants::*;
use super::{segment::TcpMigHeader, active::ActiveMigration};
use crate::{
    inetstack::protocols::{
            ipv4::Ipv4Header, 
            tcp::{
                segment::TcpHeader, peer::state::TcpState,
            },
            tcpmig::MigrationStage,
            ethernet2::{EtherType2, Ethernet2Header},
            ip::IpProtocol,
            udp::{UdpDatagram, UdpHeader},
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
    capy_profile, capy_profile_merge_previous, capy_time_log,
};

use std::collections::hash_map::Entry;
use std::time::Instant;
use ::std::{
    collections::HashMap,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    thread,
    rc::Rc,
    env,
};

#[cfg(feature = "profiler")]
use crate::timer;

use crate::capy_log_mig;

//======================================================================================================================
// Structures
//======================================================================================================================

pub enum TcpmigReceiveStatus {
    Ok,
    PrepareMigrationAcked(QDesc),
    StateReceived(TcpState),
    MigrationCompleted,
}

struct HeartbeatData {
    last_heartbeat_instant: Instant,
}

/// TCPMig Peer
pub struct TcpMigPeer {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime>,
    
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,

    // Only if this is on a backend server.
    heartbeat: Option<HeartbeatData>,

    /// Connections being actively migrated in/out.
    /// 
    /// key = remote.
    active_migrations: HashMap<SocketAddrV4, ActiveMigration>,

    incoming_user_data: HashMap<SocketAddrV4, Buffer>,

    self_udp_port: u16,

    /// for testing
    additional_mig_delay: u32,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [TcpMigPeer].
impl TcpMigPeer {
    /// Creates a TCPMig peer.
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
    ) -> Self {
        log_init();

        Self {
            rt: rt.clone(),
            local_link_addr,
            local_ipv4_addr,
            heartbeat: match std::env::var("IS_FRONTEND") {
                Ok(val) if val == "1" => None,
                _ => Some(HeartbeatData::new())
            },
            active_migrations: HashMap::new(),
            incoming_user_data: HashMap::new(),
            self_udp_port: SELF_UDP_PORT, // TEMP

            // for testing
            additional_mig_delay: env::var("MIG_DELAY")
            .unwrap_or_else(|_| String::from("0")) // Default value is 0 if MIG_DELAY is not set
            .parse::<u32>()
            .expect("Invalid DELAY value"),
        }
    }

    pub fn set_port(&mut self, port: u16) {
        self.self_udp_port = port;
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<TcpmigReceiveStatus, Fail> {
        // Parse header.
        let (hdr, buf) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        capy_log_mig!("\n\n[RX] TCPMig");

        let remote = hdr.client;

        // First packet that target receives.
        if hdr.stage == MigrationStage::PrepareMigration {
            // capy_profile!("prepare_ack");

            capy_log_mig!("******* MIGRATION REQUESTED *******");
            capy_log_mig!("PREPARE_MIG {}", remote);
            let target = SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port);
            capy_log_mig!("I'm target {}", target);

            capy_time_log!("RECV_PREPARE_MIG,({})", remote);
            

            let active = ActiveMigration::new(
                self.rt.clone(),
                self.local_ipv4_addr,
                self.local_link_addr,
                FRONTEND_IP,
                FRONTEND_MAC, // Need to go through the switch 
                self.self_udp_port,
                hdr.origin.port(), 
                hdr.origin,
                hdr.client,
                None,
            );
            if let Some(..) = self.active_migrations.insert(remote, active) {
                // todo!("duplicate active migration");
                // It happens when a backend send PREPARE_MIGRATION to the switch
                // but it receives back the message again (i.e., this is the current minimum workload backend)
                // In this case, remove the active migration.
                capy_log_mig!("It returned back to itself, maybe it's the current-min-workload server");
                self.active_migrations.remove(&remote); 
                return Ok(TcpmigReceiveStatus::Ok)
            }
        }

        let mut entry = match self.active_migrations.entry(remote) {
            Entry::Vacant(..) => panic!("no such active migration"),
            Entry::Occupied(entry) => entry,
        };
        let active = entry.get_mut();


        capy_log_mig!("Active migration {:?}", remote);
        let status = active.process_packet(ipv4_hdr, hdr, buf)?;
        match status {
            TcpmigReceiveStatus::PrepareMigrationAcked(..) => (),
            TcpmigReceiveStatus::StateReceived(ref state) => {
                // capy_profile_merge_previous!("migrate_ack");

                // Push user data into queue.
                /* if let Some(data) = user_data {
                    assert!(self.incoming_user_data.insert(remote, data).is_none());
                } */

                let conn = state.connection();
                capy_log_mig!("======= MIGRATING IN STATE ({}, {}) =======", conn.0, conn.1);

                // Remove active migration.
                // entry.remove();
            },
            TcpmigReceiveStatus::MigrationCompleted => {
                // Remove active migration.
                entry.remove();

                capy_log_mig!("1");
                capy_log_mig!("2, active_migrations: {:?}, removing {}", 
                    self.active_migrations.keys().collect::<Vec<_>>(), remote);

                
                //self.is_currently_migrating = false;
                capy_log_mig!("CONN_STATE_ACK ({})\n=======  MIGRATION COMPLETE! =======\n\n", remote);
            },
            TcpmigReceiveStatus::Ok => (),
        };

        Ok(status)
    }

    pub fn initiate_migration(&mut self, conn: (SocketAddrV4, SocketAddrV4), qd: QDesc) {
        {
            // capy_profile!("additional_delay");
            for _ in 0..self.additional_mig_delay {
                thread::yield_now();
            }
        }

        let (local, remote) = conn;
        
        // eprintln!("initiate migration for connection {} <-> {}", origin, client);

        //let origin = SocketAddrV4::new(self.local_ipv4_addr, origin_port);
        // let target = SocketAddrV4::new(FRONTEND_IP, FRONTEND_PORT); // TEMP
        

        let active = ActiveMigration::new(
            self.rt.clone(),
            self.local_ipv4_addr,
            self.local_link_addr,
            FRONTEND_IP,
            FRONTEND_MAC, 
            self.self_udp_port,
            if self.self_udp_port == 10001 { 10000 } else { 10001 }, // dest_udp_port is unknown until it receives PREPARE_MIGRATION_ACK, so it's 0 initially.
            local,
            remote,
            Some(qd),
        ); // Inho: Q. Why link_addr (MAC addr) is needed when the libOS has arp_table already? Is it possible to use the arp_table instead?
        
        let active = match self.active_migrations.entry(remote) {
            Entry::Occupied(..) => panic!("duplicate initiate migration"),
            Entry::Vacant(entry) => entry.insert(active),
        };

        active.initiate_migration();
    }

    pub fn send_tcp_state(&mut self, state: TcpState) {
        let remote = state.remote();
        let active = self.active_migrations.get_mut(&remote).unwrap();
        active.send_connection_state(state);

        // Remove migrated user data if present.
        self.incoming_user_data.remove(&remote);
    }

    /// Returns the moved buffers for further use by the caller if packet was not buffered.
    pub fn try_buffer_packet(&mut self, remote: SocketAddrV4, tcp_hdr: TcpHeader, data: Buffer) -> Result<(), (TcpHeader, Buffer)> {
        match self.active_migrations.get_mut(&remote) {
            Some(active) => {
                capy_log_mig!("mig_prepared ==> buffer");
                active.buffer_packet(tcp_hdr, data);
                Ok(())
            },
            None => {
                capy_log_mig!("trying to buffer, but there is no corresponding active migration");
                Err((tcp_hdr, data))
            },
        }
    }

    /// Returns the buffered packets for the migrated connection.
    pub fn close_active_migration(&mut self, remote: SocketAddrV4) -> Option<Vec<(TcpHeader, Buffer)>> {
        self.active_migrations.remove(&remote).map(|mut active| active.take_buffered_packets())
    }

    pub fn take_incoming_user_data(&mut self, remote: SocketAddrV4) -> Option<Buffer> {
        self.incoming_user_data.remove(&remote)
    }

    pub fn queue_length_heartbeat(&mut self) {
        todo!("heartbeat")
        /* let queue_len: usize = self.stats.global_recv_queue_length() as usize;
        if queue_len > self.recv_queue_length_threshold{
            return;
        }
        if let Some(heartbeat) = self.heartbeat.as_mut() { 
            let now = Instant::now();
            if now - heartbeat.last_heartbeat_instant > HEARTBEAT_INTERVAL {
                heartbeat.last_heartbeat_instant = now;

                // BAD: Cache DataBuffer.
                let mut data = HEARTBEAT_MAGIC.to_be_bytes().to_vec();
                data.extend_from_slice(&queue_len.to_be_bytes());

                let segment = UdpDatagram::new(
                    Ethernet2Header::new(FRONTEND_MAC, self.local_link_addr, EtherType2::Ipv4),
                    Ipv4Header::new(self.local_ipv4_addr, FRONTEND_IP, IpProtocol::UDP),
                    UdpHeader::new(self.self_udp_port, FRONTEND_PORT),
                    Buffer::Heap(DataBuffer::from_slice(&data)),
                    false,
                );

                self.rt.transmit(Box::new(segment));
            }
        } */
    }
}

impl HeartbeatData {
    fn new() -> Self {
        Self { last_heartbeat_instant: Instant::now() }
    }
}

/*************************************************************/
/* LOGGING QUEUE LENGTH */
/*************************************************************/

static mut LOG: Option<Vec<usize>> = None;
const GRANULARITY: i32 = 1; // Logs length after every GRANULARITY packets.

fn log_init() {
    unsafe { LOG = Some(Vec::with_capacity(1024*1024)); }
}

fn log_len(len: usize) {
    static mut GRANULARITY_FLAG: i32 = GRANULARITY;

    unsafe {
        GRANULARITY_FLAG -= 1;
        if GRANULARITY_FLAG > 0 {
            return;
        }
        GRANULARITY_FLAG = GRANULARITY;
    }
    
    unsafe { LOG.as_mut().unwrap_unchecked() }.push(len);
}

pub fn log_print() {
    unsafe { LOG.as_ref().unwrap_unchecked() }.iter().for_each(|len| println!("{}", len));
}