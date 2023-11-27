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

use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::{collections::HashSet, time::Instant};
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
    StateReceived(TcpState, Vec<(TcpHeader, Buffer)>),
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

    // stats: TcpMigStats,

    //is_currently_migrating: bool,

    recv_queue_length_threshold: usize,

    self_udp_port: u16,

    migrated_out_connections: HashSet<SocketAddrV4>,
    /// for testing
    additional_mig_delay: u32,
    /// for testing, the number of migrations to perform
    migration_variable: u32,
    /// for testing, frequency of migration
    migration_per_n: i32,
    /// for testing, number of requests remaining before next migration
    requests_remaining: HashMap<(SocketAddrV4, SocketAddrV4), i32>,

    /// For testing
    is_core_id_1: bool,
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
        let recv_queue_length_threshold: usize = match std::env::var("RECV_QUEUE_LEN") {
            Ok(val) => val.parse().expect("RECV_QUEUE_LEN should be a number"),
            Err(..) => 30,
        };

        log_init();
        println!("RECV_QUEUE_LENGTH_THRESHOLD: {}", recv_queue_length_threshold);

        Self {
            rt: rt.clone(),
            //arp,
            local_link_addr,
            local_ipv4_addr,
            heartbeat: match std::env::var("IS_FRONTEND") {
                Ok(val) if val == "1" => None,
                _ => Some(HeartbeatData::new())
            },
            active_migrations: HashMap::new(),
            incoming_user_data: HashMap::new(),
            // stats: TcpMigStats::new(recv_queue_length_threshold),
            recv_queue_length_threshold,
            self_udp_port: SELF_UDP_PORT, // TEMP
            migrated_out_connections: HashSet::new(),

            // for testing
            additional_mig_delay: env::var("MIG_DELAY")
            .unwrap_or_else(|_| String::from("0")) // Default value is 0 if MIG_DELAY is not set
            .parse::<u32>()
            .expect("Invalid DELAY value"),
            migration_variable: env::var("MIG_VAR")
            .unwrap_or_else(|_| String::from("0")) // Default value is 0 if MIG_VAR is not set
            .parse::<u32>()
            .expect("Invalid DELAY value"),
            migration_per_n: env::var("MIG_PER_N")
            .unwrap_or(String::from("100")) // Default value is 100 if MIG_PER_N is not set
            .parse()
            .expect("MIG_PER_N must be a i32"),
            requests_remaining: HashMap::new(),
            is_core_id_1: std::env::var("CORE_ID") == Ok("1".to_string()),
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
            capy_profile!("prepare_ack");

            capy_log_mig!("******* MIGRATION REQUESTED *******");
            capy_log_mig!("PREPARE_MIG {}", remote);
            let target = SocketAddrV4::new(self.local_ipv4_addr, self.self_udp_port);
            capy_log_mig!("I'm target {}", target);

            capy_time_log!("RECV_PREPARE_MIG,({}-{})", hdr.origin, remote);
            

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
                //self.is_currently_migrating = false;
                return Ok(TcpmigReceiveStatus::Ok)
            }
            //self.origins.insert((target, hdr.client), hdr.origin);
        }

        let mut entry = match self.active_migrations.entry(remote) {
            Entry::Vacant(..) => panic!("no such active migration"),
            Entry::Occupied(entry) => entry,
        };
        let active = entry.get_mut();


        capy_log_mig!("Active migration {:?}", remote);
        let status = active.process_packet(ipv4_hdr, hdr, buf)?;
        match status {
            TcpmigReceiveStatus::PrepareMigrationAcked(qd) => {
                /* #[cfg(not(feature = "mig-per-n-req"))]
                self.set_as_ready_to_migrate_out(qd);
                
                #[cfg(feature = "mig-per-n-req")]{
                    self.set_as_ready_to_migrate_out(qd);
                    // capy_profile!("migrate");

                    // capy_log_mig!("\n\nMigrate Out ({}, {})", hdr.origin, hdr.client);
                    // let state = tcp_peer.migrate_out_tcp_connection(active.qd().unwrap())?;
                    // let remote = state.remote;
                    // active.send_connection_state(state);
                    // assert!(self.migrated_out_connections.insert(remote), "Duplicate migrated_out_connections set insertion");
                } */

                /* let state = tcp_peer.migrate_out_tcp_connection(qd).unwrap();
                // TODO: User data support.

                active.send_connection_state(state);

                // Remove migrated user data if present.
                self.incoming_user_data.remove(&remote);
                assert!(self.migrated_out_connections.insert(remote)); */

                // unsafe { LAST_MIGRATED_OUT_QD = Some(qd); }
            },
            TcpmigReceiveStatus::StateReceived(ref state, _) => {
                capy_profile_merge_previous!("migrate_ack");

                // capy_log_mig!("migrating in connection local: {}, client: {}", local, client);

                //let user_data = state.take_user_data();
                
                // tcp_peer.notify_passive(state)?;

                // Push user data into queue.
                /* if let Some(data) = user_data {
                    assert!(self.incoming_user_data.insert(remote, data).is_none());
                } */

                let conn = state.connection();
                capy_log_mig!("======= MIGRATING IN STATE ({}, {}) =======", conn.0, conn.1);

                // Remove active migration.
                entry.remove();
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
            capy_profile!("additional_delay");
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

        // unsafe { LAST_MIGRATED_OUT_QD = Some(qd); }
    }

    /// Returns the moved buffers for further use by the caller if packet was not buffered.
    pub fn try_buffer_packet(&mut self, remote: SocketAddrV4, tcp_hdr: TcpHeader, data: Buffer) -> Result<(), (TcpHeader, Buffer)> {
        match self.active_migrations.get_mut(&remote) {
            Some(active) => {
                capy_log_mig!("mig_prepared ==> buffer");
                active.buffer_packet(tcp_hdr, data);
                Ok(())
            },
            None => Err((tcp_hdr, data)),
        }
    }

    pub fn take_incoming_user_data(&mut self, remote: SocketAddrV4) -> Option<Buffer> {
        self.incoming_user_data.remove(&remote)
    }

    /* pub fn update_incoming_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        self.self.borrow_mut().stats.update_incoming(local, client, recv_queue_len);

        // unsafe {
        //     static mut TMP: i32 = 0;
        //     TMP += 1;
        //     if TMP == 1000 {
        //         TMP = 0;
        //         println!("global_recv_queue_length: {}", self.self.borrow().stats.global_recv_queue_length());
        //     }
        // }
    } */

    #[inline]
    /// Returns the updated granularity counter.
    pub fn stats_recv_queue_push(&self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize, granularity_counter: &Cell<i32>) {
        /* match self.active_migrations.get_mut(&connection.1) {
            Some(active) => {},
            None => { 
                self.stats.recv_queue_update(connection, true, new_queue_len, granularity_counter); 
            },
        } */
    }

    #[inline]
    /// Returns the updated granularity counter.
    pub fn stats_recv_queue_pop(&self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize, granularity_counter: &Cell<i32>) {
        /* match self.active_migrations.get_mut(&connection.1) {
            Some(active) => {},
            None => { 
                self.stats.recv_queue_update(connection, false, new_queue_len, granularity_counter); 
            },
        } */
    }

    /* pub fn update_outgoing_stats(&mut self) {
        self.self.borrow_mut().stats.update_outgoing();
    } */

    /* pub fn should_migrate(&mut self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        static mut NUM_MIG: u32 = 0;
        static mut FLAG: u32 = 0;
        
        if !self.is_core_id_1 {
            return None;
        }
        unsafe{
            
            if self.stats.num_of_connections() <= 1 
                // || self.active_migrations.len() >= 3 
                || NUM_MIG >= self.migration_variable {
                return None;
            }

            // FLAG += 1;
            
            // if FLAG % 1024 == 0{
            //     FLAG = 0;
            // }
            // else{
            //     return None;
            // }
            let recv_queue_len = self.stats.avg_global_recv_queue_length();

            // log_len(recv_queue_len);
            // return None;
            // println!("check recv_queue_len {}, self.recv_queue_length_threshold {}", recv_queue_len, self.recv_queue_length_threshold);
            if recv_queue_len > self.recv_queue_length_threshold {
                match self.stats.get_connection_to_migrate_out() {
                    Some(connection) => {
                        NUM_MIG += 1;
                        return Some(connection);
                    },
                    None => {
                        return None;
                    },
                }
            
            }
            return None;
        }
    } */

    // TEMP (for migration test)
    /* pub fn should_migrate(&self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        static mut FLAG: u32 = 0;
        static mut WARMUP: u32 = 0;
        static mut NUM_MIG: u32 = 0;

        if std::env::var("CORE_ID") == Ok("1".to_string()) {
            unsafe { WARMUP += 1; }
            if unsafe { WARMUP } < 200000 {
                return None;
            }
        }else{
            return None;
        }

        let mut self = self.self.borrow_mut();
        // println!("NUM_MIG: {} , num_conn: {}", unsafe{NUM_MIG}, self.stats.num_of_connections());
        unsafe{
            if self.stats.num_of_connections() <= 0 
                || NUM_MIG >= self.migration_variable {
                return None;
            }
            
            FLAG += 1;
            // eprintln!("FLAG: {}", FLAG);
            if FLAG == 3 {
                FLAG = 0;
                NUM_MIG+=1;
                capy_log_mig!("START MIG");
                return self.stats.get_connection_to_migrate_out();
            }
            return None;
        }
    } */

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

    /* pub fn stop_tracking_connection_stats(
        &mut self, 
        local: SocketAddrV4, 
        client: SocketAddrV4,
        recv_queue_len: usize,
    ) {
        self.stats.stop_tracking_connection(local, client, recv_queue_len)
    } */

    /* pub fn stats_decrease_global_queue_length(&mut self, by: usize) {
        self.self.borrow_mut().stats.decrease_global_queue_length(by)
    } */

    /* pub fn print_stats(&self) {
        println!("global_recv_queue_length: {}", self.stats.global_recv_queue_length());
        // println!("ratio: {}", self.self.borrow().stats.get_rx_tx_ratio());
        // println!("TCPMig stats: {:?}", self.stats);
    }
    
    pub fn start_tracking_connection_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        self.stats.start_tracking_connection(local, client, recv_queue_len)
    }
    
    pub fn global_recv_queue_length(&mut self) -> usize {
        self.stats.global_recv_queue_length()
    } */
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