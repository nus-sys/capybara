// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================
use bit_vec::BitVec;
use super::constants::*;
use super::{segment::TcpMigHeader, active::ActiveMigration, stats::TcpMigStats};
use crate::{
    inetstack::protocols::{
            ipv4::Ipv4Header, 
            tcp::{
                segment::TcpHeader, peer::TcpState, TcpPeer,
            },
            tcp_migration::{
                MigrationStage,
                active::MigrationRequestStatus
            },
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
use std::{cell::RefCell, collections::HashSet, time::Instant};
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

use crate::{capy_log, capy_log_mig};

// TODO: Get rid of this.
pub static mut LAST_MIGRATED_OUT_QD: Option<QDesc> = None;


//======================================================================================================================
// Structures
//======================================================================================================================

struct HeartbeatData {
    last_heartbeat_instant: Instant,
}

/// TCPMig Peer
struct Inner {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime>,
    /* /// Underlying ARP peer.
    arp: ArpPeer, */
    /// Local link address.
    local_link_addr: MacAddress,
    /// Local IPv4 address.
    local_ipv4_addr: Ipv4Addr,

    // Only if this is on a backend server.
    heartbeat: Option<HeartbeatData>,

    /// Connections being actively migrated in/out.
    /// 
    /// key = (origin, client).
    active_migrations: HashMap<(SocketAddrV4, SocketAddrV4), ActiveMigration>,

    /// QDescs of connections prepared to be migrated.
    /// 
    /// key = QDesc.
    // migrations_prepared_qds: HashSet<QDesc>,


    /// Origins. Only used on the target side to get the origin of redirected packets.
    /// 
    /// key = (target, client).
    origins: HashMap<(SocketAddrV4, SocketAddrV4), SocketAddrV4>,

    /// Connections ready to be migrated-in.
    /// 
    /// key = (local, client).
    incoming_connections: HashSet<(SocketAddrV4, SocketAddrV4)>,

    incoming_user_data: HashMap<(SocketAddrV4, SocketAddrV4), Buffer>,

    stats: TcpMigStats,

    /// Marks which QD is ready to migrate out.
    ready_to_migrate_out: BitVec,

    //is_currently_migrating: bool,

    //rx_tx_threshold_ratio: f64,
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

    /* /// The background co-routine retransmits TCPMig packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle, */
}

#[derive(Clone)]
pub struct TcpMigPeer {
    inner: Rc<RefCell<Inner>>,
}

pub struct MigrationHandle((SocketAddrV4, SocketAddrV4), QDesc);

impl MigrationHandle {
    pub fn inner(&self) -> ((SocketAddrV4, SocketAddrV4), QDesc) {
        let Self(conn, qd) = self;
        (*conn, *qd)
    }
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [TcpMigPeer].
impl TcpMigPeer {
    /// Creates a TCPMig peer.
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        //scheduler: Scheduler,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        //arp: ArpPeer,
    ) -> Result<Self, Fail> {
        /* let future = Self::background_sender(
            rt.clone(),
            local_ipv4_addr,
            local_link_addr,
            offload_checksum,
            arp.clone(),
            send_queue.clone(),
        );
        let handle: SchedulerHandle = match scheduler.insert(FutureOperation::Background(future.boxed_local())) {
            Some(handle) => handle,
            None => {
                return Err(Fail::new(
                    EAGAIN,
                    "failed to schedule background co-routine for TCPMig module",
                ))
            },
        }; */

        let inner = Inner::new(
            rt.clone(),
            local_link_addr,
            local_ipv4_addr,
        );
        println!("RECV_QUEUE_LENGTH_THRESHOLD: {}", inner.recv_queue_length_threshold);

        Ok(Self{inner: Rc::new(RefCell::new(inner))})
    }

    pub fn set_port(&mut self, port: u16) {
        let mut inner = self.inner.borrow_mut();
        inner.self_udp_port = port;
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, tcp_peer: &mut TcpPeer, ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        // Parse header.
        let (hdr, buf) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        capy_log_mig!("\n\n[RX] TCPMig");

        let key = (hdr.origin, hdr.client);

        let mut inner = self.inner.borrow_mut();

        // First packet that target receives.
        if hdr.stage == MigrationStage::PrepareMigration {
            capy_time_log!("RECV_PREPARE_MIG,({}-{})", key.0, key.1);
            capy_profile!("prepare_ack");

            capy_log_mig!("******* MIGRATION REQUESTED *******");
            capy_log_mig!("PREPARE_MIG {:?}", key);

            let active = ActiveMigration::new(
                inner.rt.clone(),
                inner.local_ipv4_addr,
                inner.local_link_addr,
                FRONTEND_IP,
                FRONTEND_MAC, // Need to go through the switch 
                inner.self_udp_port,
                hdr.origin.port(), 
                hdr.origin,
                hdr.client,
                None,
            );
            if let Some(..) = inner.active_migrations.insert(key, active) {
                // todo!("duplicate active migration");
                // It happens when a backend send PREPARE_MIGRATION to the switch
                // but it receives back the message again (i.e., this is the current minimum workload backend)
                // In this case, remove the active migration.
                capy_log_mig!("It returned back to itself, maybe it's the current-min-workload server");
                inner.active_migrations.remove(&key); 
                //inner.is_currently_migrating = false;
                return Ok(())
            }

            let target = SocketAddrV4::new(inner.local_ipv4_addr, inner.self_udp_port);
            capy_log_mig!("I'm target {}", target);
            inner.origins.insert((target, hdr.client), hdr.origin);
        }

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => return Err(Fail::new(libc::EINVAL, "no such active migration")),
        };
        

        capy_log_mig!("Active migration {:?}", key);
        match active.process_packet(ipv4_hdr, hdr, buf)? {
            MigrationRequestStatus::Rejected => unimplemented!("migration rejection"),
            MigrationRequestStatus::PrepareMigrationAcked(qd) => {
                /* #[cfg(not(feature = "mig-per-n-req"))]
                inner.set_as_ready_to_migrate_out(qd);
                
                #[cfg(feature = "mig-per-n-req")]{
                    inner.set_as_ready_to_migrate_out(qd);
                    // capy_profile!("migrate");

                    // capy_log_mig!("\n\nMigrate Out ({}, {})", hdr.origin, hdr.client);
                    // let state = tcp_peer.migrate_out_tcp_connection(active.qd().unwrap())?;
                    // let remote = state.remote;
                    // active.send_connection_state(state);
                    // assert!(inner.migrated_out_connections.insert(remote), "Duplicate migrated_out_connections set insertion");
                } */

                let conn = key;
                let state = tcp_peer.migrate_out_tcp_connection(qd).unwrap();
                // TODO: User data support.

                active.send_connection_state(state);

                // Remove migrated user data if present.
                inner.incoming_user_data.remove(&conn);
                assert!(inner.migrated_out_connections.insert(conn.1));

                unsafe { LAST_MIGRATED_OUT_QD = Some(qd); }
            },
            MigrationRequestStatus::StateReceived(mut state) => {
                capy_profile_merge_previous!("migrate_ack");

                let conn = (state.local, state.remote);
                // capy_log_mig!("migrating in connection local: {}, client: {}", local, client);

                let user_data = state.take_user_data();
                
                tcp_peer.notify_passive(state)?;

                // Push user data into queue.
                if let Some(data) = user_data {
                    assert!(inner.incoming_user_data.insert(conn, data).is_none());
                }

                inner.incoming_connections.insert(conn);
                capy_log_mig!("======= MIGRATING IN STATE ({}, {}) =======", conn.0, conn.1);
                // inner.active_migrations.remove(&(hdr.origin, hdr.client)).expect("active migration should exist"); 
                // Shouldn't be removed yet. Should be after processing migration queue.
            },
            MigrationRequestStatus::MigrationCompleted => {
                capy_log_mig!("1");
                let (local, client) = (hdr.origin, hdr.client);

                capy_log_mig!("2, active_migrations: {:?}, removing {:?}", 
                    inner.active_migrations.keys().collect::<Vec<_>>(), (local, client));

                inner.active_migrations.remove(&(local, client)).expect("active migration should exist"); 
                
                //inner.is_currently_migrating = false;
                capy_log_mig!("CONN_STATE_ACK ({}, {})\n=======  MIGRATION COMPLETE! =======\n\n", local, client);
            },
            MigrationRequestStatus::Ok => (),
        };

        Ok(())
    }

    pub fn initiate_migration(&mut self, conn: (SocketAddrV4, SocketAddrV4), qd: QDesc) {
        let mut inner = self.inner.borrow_mut();
        
        {
            capy_profile!("additional_delay");
            for _ in 0..inner.additional_mig_delay {
                thread::yield_now();
            }
        }

        let (origin, client) = conn;
        
        // eprintln!("initiate migration for connection {} <-> {}", origin, client);

        //let origin = SocketAddrV4::new(inner.local_ipv4_addr, origin_port);
        // let target = SocketAddrV4::new(FRONTEND_IP, FRONTEND_PORT); // TEMP
        

        let active = ActiveMigration::new(
            inner.rt.clone(),
            inner.local_ipv4_addr,
            inner.local_link_addr,
            FRONTEND_IP,
            FRONTEND_MAC, 
            inner.self_udp_port,
            if inner.self_udp_port == 10001 { 10000 } else { 10001 }, // dest_udp_port is unknown until it receives PREPARE_MIGRATION_ACK, so it's 0 initially.
            origin,
            client,
            Some(qd),
        ); // Inho: Q. Why link_addr (MAC addr) is needed when the libOS has arp_table already? Is it possible to use the arp_table instead?
        
        if let Some(..) = inner.active_migrations.insert(conn, active) {
            // Control should never enter here.
            panic!("duplicate active migration");
        };

        let active = match inner.active_migrations.get_mut(&conn) {
            Some(active) => active,
            None => unreachable!(),
        };

        active.initiate_migration();
        //inner.is_currently_migrating = true;
    }

    pub fn is_migrated_out(&self, client: SocketAddrV4) -> bool {
        self.inner.borrow().migrated_out_connections.contains(&client)
    }

    pub fn is_ready_to_migrate_out(&self, qd: QDesc) -> bool {
        self.inner.borrow().ready_to_migrate_out.get(qd.into()).unwrap_or(false)
    }

    pub fn get_migration_handle(&self, conn: (SocketAddrV4, SocketAddrV4), qd: QDesc) -> Option<MigrationHandle> {
        let inner = self.inner.borrow();
        if let Some(true) = inner.ready_to_migrate_out.get(qd.into()) {
            let active = inner.active_migrations.get(&conn).expect("active migration should exist");
            assert!(active.is_prepared());
            return Some(MigrationHandle(conn, qd))
        }
        None
    }

    pub fn migrate_out(&mut self, handle: MigrationHandle, state: TcpState) {
        let MigrationHandle(conn, qd) = handle;
        let mut inner = self.inner.borrow_mut();

        inner.ready_to_migrate_out.set(qd.into(), false);

        // Remove migrated user data if present.
        inner.incoming_user_data.remove(&conn);

        let active = inner.active_migrations.get_mut(&conn).unwrap();
        active.send_connection_state(state);
        #[cfg(not(feature = "mig-per-n-req"))] {
            let qd = active.qd().unwrap();
            // if !inner.migrations_prepared_qds.remove(&qd){
            //     panic!("this qd is not migration-prepared");
            // }
        }
        if !inner.migrated_out_connections.insert(conn.1) {
            panic!("Duplicate migrated_out_connections set insertion");
        }
    }

    /// Returns the moved buffers for further use by the caller if packet was not buffered.
    pub fn try_buffer_packet(&mut self, target: SocketAddrV4, client: SocketAddrV4, tcp_hdr: TcpHeader, data: Buffer) -> Result<(), (TcpHeader, Buffer)> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, client)) {
            Some(origin) => *origin,
            None => return Err((tcp_hdr, data)),
        };

        match inner.active_migrations.get_mut(&(origin, client)) {
            Some(active) => {
                #[cfg(feature = "capy-log")]
                capy_log_mig!("mig_prepared ==> buffer");
                active.buffer_packet(tcp_hdr, data);
                Ok(())
            },
            None => Err((tcp_hdr, data)),
        }
    }

    pub fn take_connection(&mut self, local: SocketAddrV4, client: SocketAddrV4) -> bool {
        self.inner.borrow_mut()
        .incoming_connections.remove(&(local, client))
    }

    pub fn take_buffer_queue(&mut self, target: SocketAddrV4, client: SocketAddrV4) -> Result<Vec<(TcpHeader, Buffer)>, Fail> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, client)) {
            Some(origin) => *origin,
            None => return Err(Fail::new(libc::EINVAL, "no origin found")),
        };

        match inner.active_migrations.get_mut(&(origin, client)) {
            Some(active) => Ok(std::mem::take(&mut active.recv_queue)),
            None => Err(Fail::new(libc::EINVAL, "no active migration found")),
        }
    }

    pub fn complete_migrating_in(&mut self, target: SocketAddrV4, client: SocketAddrV4) {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, client)) {
            Some(origin) => *origin,
            None => panic!("no origin found for connection: ({:?}, {:?})", target, client),
        };
        inner.active_migrations.remove(&(origin, client)).expect("active migration should exist");
        inner.migrated_out_connections.remove(&client);
        // eprintln!("migrated_out_connections: {:?}", inner.migrated_out_connections);
    }

    pub fn take_incoming_user_data(&mut self, conn: &(SocketAddrV4, SocketAddrV4)) -> Option<Buffer> {
        self.inner.borrow_mut().incoming_user_data.remove(conn)
    }

    /* pub fn update_incoming_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        self.inner.borrow_mut().stats.update_incoming(local, client, recv_queue_len);

        // unsafe {
        //     static mut TMP: i32 = 0;
        //     TMP += 1;
        //     if TMP == 1000 {
        //         TMP = 0;
        //         println!("global_recv_queue_length: {}", self.inner.borrow().stats.global_recv_queue_length());
        //     }
        // }
    } */

    #[inline]
    /// Returns the updated granularity counter.
    pub fn stats_recv_queue_push(&self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize, granularity_counter: &Cell<i32>) {
        let mut inner = self.inner.borrow_mut();
        match inner.active_migrations.get_mut(&connection) {
            Some(active) => {},
            None => { 
                inner.stats.recv_queue_update(connection, true, new_queue_len, granularity_counter); 
            },
        }
    }

    #[inline]
    /// Returns the updated granularity counter.
    pub fn stats_recv_queue_pop(&self, connection: (SocketAddrV4, SocketAddrV4), new_queue_len: usize, granularity_counter: &Cell<i32>) {
        let mut inner = self.inner.borrow_mut();
        match inner.active_migrations.get_mut(&connection) {
            Some(active) => {},
            None => { 
                inner.stats.recv_queue_update(connection, false, new_queue_len, granularity_counter); 
            },
        }
    }

    /* pub fn update_outgoing_stats(&mut self) {
        self.inner.borrow_mut().stats.update_outgoing();
    } */

    pub fn should_migrate(&mut self) -> Option<(SocketAddrV4, SocketAddrV4)> {
        static mut NUM_MIG: u32 = 0;
        static mut FLAG: u32 = 0;

        let mut inner = self.inner.borrow_mut();
        
        if std::env::var("CORE_ID") != Ok("1".to_string()) {
            return None;
        }
        unsafe{
            
            if inner.stats.num_of_connections() <= 1 
                // || inner.active_migrations.len() >= 3 
                || NUM_MIG >= inner.migration_variable {
                return None;
            }

            // FLAG += 1;
            
            // if FLAG % 1024 == 0{
            //     FLAG = 0;
            // }
            // else{
            //     return None;
            // }
            let recv_queue_len = inner.stats.avg_global_recv_queue_length();

            // log_len(recv_queue_len);
            // return None;
            // println!("check recv_queue_len {}, inner.recv_queue_length_threshold {}", recv_queue_len, inner.recv_queue_length_threshold);
            if recv_queue_len > inner.recv_queue_length_threshold {
                match inner.stats.get_connection_to_migrate_out() {
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
    }

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

        let mut inner = self.inner.borrow_mut();
        // println!("NUM_MIG: {} , num_conn: {}", unsafe{NUM_MIG}, inner.stats.num_of_connections());
        unsafe{
            if inner.stats.num_of_connections() <= 0 
                || NUM_MIG >= inner.migration_variable {
                return None;
            }
            
            FLAG += 1;
            // eprintln!("FLAG: {}", FLAG);
            if FLAG == 3 {
                FLAG = 0;
                NUM_MIG+=1;
                capy_log_mig!("START MIG");
                return inner.stats.get_connection_to_migrate_out();
            }
            return None;
        }
    } */

    pub fn queue_length_heartbeat(&mut self) {

        let mut inner = self.inner.borrow_mut();
        let queue_len: usize = inner.stats.global_recv_queue_length() as usize;
        if queue_len > inner.recv_queue_length_threshold{
            return;
        }
        if let Some(heartbeat) = inner.heartbeat.as_mut() { 
            let now = Instant::now();
            if now - heartbeat.last_heartbeat_instant > HEARTBEAT_INTERVAL {
                heartbeat.last_heartbeat_instant = now;

                let mut data = HEARTBEAT_MAGIC.to_be_bytes().to_vec();
                data.extend_from_slice(&queue_len.to_be_bytes());

                let segment = UdpDatagram::new(
                    Ethernet2Header::new(FRONTEND_MAC, inner.local_link_addr, EtherType2::Ipv4),
                    Ipv4Header::new(inner.local_ipv4_addr, FRONTEND_IP, IpProtocol::UDP),
                    UdpHeader::new(inner.self_udp_port, FRONTEND_PORT),
                    Buffer::Heap(DataBuffer::from_slice(&data)),
                    false,
                );

                inner.rt.transmit(Box::new(segment));
            }
        }
    }

    pub fn stop_tracking_connection_stats(
        &mut self, 
        local: SocketAddrV4, 
        client: SocketAddrV4,
        recv_queue_len: usize,
    ) {
        self.inner.borrow_mut().stats.stop_tracking_connection(
                                                                local, 
                                                                client, 
                                                                recv_queue_len,
                                                            )
    }

    /* pub fn stats_decrease_global_queue_length(&mut self, by: usize) {
        self.inner.borrow_mut().stats.decrease_global_queue_length(by)
    } */

    pub fn print_stats(&self) {
        let inner = self.inner.borrow();
        println!("global_recv_queue_length: {}", self.inner.borrow().stats.global_recv_queue_length());
        // println!("ratio: {}", self.inner.borrow().stats.get_rx_tx_ratio());
        // println!("TCPMig stats: {:?}", inner.stats);
    }

    pub fn get_migration_prepared_qds(&mut self) -> Result<HashSet<QDesc>, Fail> {
        // let prepared_qds = self.inner.borrow().migrations_prepared_qds.clone();
        // return Ok(prepared_qds)
        Err(Fail::new(libc::EINVAL, "Currently we don't use this optimization"))
    }
    
    pub fn start_tracking_connection_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        self.inner.borrow_mut().stats.start_tracking_connection(local, client, recv_queue_len)
    }
    
    pub fn global_recv_queue_length(&mut self) -> usize {
        self.inner.borrow_mut().stats.global_recv_queue_length()
    }

    pub fn print_queue_length(&mut self) {
        self.inner.borrow_mut().stats.print_queue_length();
    }

    pub fn pushed_response(&mut self) {
        // self.inner.borrow_mut().stats.pushed_response();
    }
}

impl Inner {
    fn new(
        rt: Rc<dyn NetworkRuntime>,
        //scheduler: Scheduler,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        //arp: ArpPeer,
    ) -> Self {
        let recv_queue_length_threshold: usize = match std::env::var("RECV_QUEUE_LEN") {
            Ok(val) => val.parse().expect("RECV_QUEUE_LEN should be a number"),
            Err(..) => BASE_RECV_QUEUE_LENGTH_THRESHOLD,
        };

        log_init();

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
            // migrations_prepared_qds: HashSet::new(),
            origins: HashMap::new(),
            incoming_connections: HashSet::new(),
            incoming_user_data: HashMap::new(),
            stats: TcpMigStats::new(recv_queue_length_threshold),
            ready_to_migrate_out: BitVec::from_elem(64, false),
            //is_currently_migrating: false,
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
            //background: handle,
        }
    }

    fn set_as_ready_to_migrate_out(&mut self, qd: QDesc) {
        let idx = qd.into();
        if self.ready_to_migrate_out.len() <= idx {
            self.ready_to_migrate_out.grow(self.ready_to_migrate_out.len(), false);
        }
        self.ready_to_migrate_out.set(idx, true);
    }

    // fn is_currently_migrating(&self) -> bool {
    //     #[cfg(feature = "concurrent-tcp-migrations")]
    //     return false;
    //     #[cfg(not(feature = "concurrent-tcp-migrations"))]
    //     return true;
    // }
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