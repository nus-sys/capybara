// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================
use super::constants::*;
use super::{segment::TcpMigHeader, active::ActiveMigration, stats::TcpMigStats};
use crate::inetstack::protocols::ethernet2::{EtherType2, Ethernet2Header};
use crate::inetstack::protocols::ip::IpProtocol;
use crate::inetstack::protocols::udp::{UdpDatagram, UdpHeader};
use crate::{
    inetstack::protocols::{
            ipv4::Ipv4Header, 
            tcp::{
                segment::TcpHeader, peer::TcpState, TcpPeer,
            },
            tcpmig::{
                MigrationStage,
                active::MigrationRequestStatus
            },
        },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    }, 
};

#[cfg(feature = "tcp-migration-profiler")]
use crate::{tcpmig_profile, tcpmig_profile_merge_previous};

use std::{cell::RefCell, collections::{VecDeque, HashSet}, time::Instant};
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

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcpmig_log;


//======================================================================================================================
// Structures
//======================================================================================================================

struct HeartbeatData {
    last_heartbeat_instant: Instant,
}

/// TCPMig Peer
struct Inner<const N: usize> {
    /// Underlying runtime.
    rt: Rc<dyn NetworkRuntime<N>>,
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
    active_migrations: HashMap<(SocketAddrV4, SocketAddrV4), ActiveMigration<N>>,

    /// Origins. Only used on the target side to get the origin of redirected packets.
    /// 
    /// key = (target, client).
    origins: HashMap<(SocketAddrV4, SocketAddrV4), SocketAddrV4>,

    /// Connections ready to be migrated-in.
    /// 
    /// key = (local, client).
    incoming_connections: HashSet<(SocketAddrV4, SocketAddrV4)>,

    stats: TcpMigStats,

    is_currently_migrating: bool,

    //rx_tx_threshold_ratio: f64,
    recv_queue_length_threshold: f64,

    self_udp_port: u16,

    migrated_out_connections: HashSet<SocketAddrV4>,
    additional_mig_delay: u32,
    /* /// The background co-routine retransmits TCPMig packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle, */
}

#[derive(Clone)]
pub struct TcpMigPeer<const N: usize> {
    inner: Rc<RefCell<Inner<N>>>,
}

pub struct MigrationHandle {
    origin: SocketAddrV4,
    client: SocketAddrV4,
}

//======================================================================================================================
// Associate Functions
//======================================================================================================================

/// Associate functions for [TcpMigPeer].
impl<const N: usize> TcpMigPeer<N> {
    /// Creates a TCPMig peer.
    pub fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
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
    pub fn receive(&mut self, tcp_peer: &mut TcpPeer<N>, ipv4_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcpmig::receive");

        // Parse header.
        let (hdr, buf) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        debug!("TCPMig received {:?}", hdr);
        // eprintln!("TCPMig received {:#?}", hdr);
        #[cfg(feature = "capybara-log")]
        {
            tcpmig_log(format!("\n\n[RX] TCPMig"));
        }

        let key = (hdr.origin, hdr.client);

        let mut inner = self.inner.borrow_mut();

        // First packet that target receives.
        if hdr.stage == MigrationStage::PrepareMigration {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("prepare_ack");

            #[cfg(feature = "capybara-log")]
            {
                tcpmig_log(format!("******* MIGRATION REQUESTED *******"));
                tcpmig_log(format!("PREPARE_MIG {:?}", key));
            }
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
            );
            if let Some(..) = inner.active_migrations.insert(key, active) {
                // todo!("duplicate active migration");
                // It happens when a backend send PREPARE_MIGRATION to the switch
                // but it receives back the message again (i.e., this is the current minimum workload backend)
                // In this case, remove the active migration.
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("It returned back to itself, maybe it's the current-min-workload server"));
                }
                inner.active_migrations.remove(&key); 
                inner.is_currently_migrating = false;
                return Ok(())
            }

            let target = SocketAddrV4::new(inner.local_ipv4_addr, inner.self_udp_port);
            #[cfg(feature = "capybara-log")]
            {
                tcpmig_log(format!("I'm target {}", target));
            }
            inner.origins.insert((target, hdr.client), hdr.origin);
        }

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => return Err(Fail::new(libc::EINVAL, "no such active migration")),
        };

        #[cfg(feature = "capybara-log")]
        {
            tcpmig_log(format!("Active migration {:?}", key));
        }
        match active.process_packet(ipv4_hdr, hdr, buf)? {
            MigrationRequestStatus::Rejected => todo!("handle migration rejection"),
            MigrationRequestStatus::StateReceived(state) => {
                #[cfg(feature = "tcp-migration-profiler")]
                tcpmig_profile_merge_previous!("migrate_ack");

                let (local, client) = (state.local, state.remote);
                // println!("migrating in connection local: {}, client: {}", local, client);
                
                tcp_peer.notify_passive(state)?;
                inner.incoming_connections.insert((local, client));
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("======= MIGRATING IN STATE ({}, {}) =======", local, client));
                }
                // inner.active_migrations.remove(&(hdr.origin, hdr.client)).expect("active migration should exist"); 
                // Shouldn't be removed yet. Should be after processing migration queue.
            },
            MigrationRequestStatus::MigrationCompleted => {
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("1"));
                }
                let (local, client) = (hdr.origin, hdr.client);
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("2, active_migrations: {:?}, removing {:?}", 
                        inner.active_migrations.keys().collect::<Vec<_>>(), (local, client)));
                }
                inner.active_migrations.remove(&(local, client)).expect("active migration should exist"); //HERE!
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("3"));
                }
                inner.stats.stop_tracking_connection(local, client);
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("4"));
                }
                inner.is_currently_migrating = false;
                #[cfg(feature = "capybara-log")]
                {
                    tcpmig_log(format!("CONN_STATE_ACK ({}, {})\n=======  MIGRATION COMPLETE! =======\n\n", local, client));
                }
            },
            MigrationRequestStatus::Ok => (),
        };

        Ok(())
    }

    pub fn initiate_migration(&mut self) {
        // thread::sleep(Duration::from_nanos(1));
        let mut inner = self.inner.borrow_mut();
        
        {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("additional_delay");
            for _ in 0..inner.additional_mig_delay {
                thread::yield_now();
            }
        }

        let (origin, client) = match inner.stats.get_connection_to_migrate_out() {
            Some(conn) => conn,
            None => return,
        };
        let key = (origin, client);
        // if inner.active_migrations.contains_key(&key) {
        //     return;
        // }
        // this one maybe no need, because we are checking if the migration is
        // ongoing with is_currently_migrating

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
        ); // Inho: Q. Why link_addr (MAC addr) is needed when the libOS has arp_table already? Is it possible to use the arp_table instead?
        
        if let Some(..) = inner.active_migrations.insert(key, active) {
            todo!("duplicate active migration");
            // inho: how it can happen when we check is_currently_migrating?
            // isn't that there is one one active migration always?
            // A. looks like it cannot happen (leave it just for debugging purpose)
        };

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => unreachable!(),
        };

        active.initiate_migration();
        inner.is_currently_migrating = true;
    }

    pub fn is_migrated_out(&self, client: SocketAddrV4) -> bool {
        self.inner.borrow().migrated_out_connections.contains(&client)
    }

    pub fn can_migrate_out(&self, origin: SocketAddrV4, client: SocketAddrV4) -> Option<MigrationHandle> {
        let key = (origin, client);
        let inner = self.inner.borrow();
        if let Some(active) = inner.active_migrations.get(&key) {
            if active.is_prepared() {
                return Some(MigrationHandle { origin, client })
            }
        }
        None
    }

    pub fn migrate_out(&mut self, handle: MigrationHandle, state: TcpState) {
        let key = (handle.origin, handle.client);
        let mut inner = self.inner.borrow_mut();
        if let Some(active) = inner.active_migrations.get_mut(&key) {
            active.send_connection_state(state);
        }
        if !inner.migrated_out_connections.insert(handle.client) {
            panic!("Duplicate migrated_out_connections set insertion");
        }
    }

    pub fn try_buffer_packet(&mut self, target: SocketAddrV4, client: SocketAddrV4, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: DemiBuffer) -> Result<(), ()> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, client)) {
            Some(origin) => *origin,
            None => return Err(()),
        };

        match inner.active_migrations.get_mut(&(origin, client)) {
            Some(active) => {
                active.buffer_packet(ip_hdr, tcp_hdr, buf);
                Ok(())
            },
            None => Err(()),
        }
    }

    pub fn take_connection(&mut self, local: SocketAddrV4, client: SocketAddrV4) -> bool {
        self.inner.borrow_mut()
        .incoming_connections.remove(&(local, client))
    }

    pub fn take_buffer_queue(&mut self, target: SocketAddrV4, client: SocketAddrV4) -> Result<VecDeque<(Ipv4Header, TcpHeader, DemiBuffer)>, Fail> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, client)) {
            Some(origin) => *origin,
            None => return Err(Fail::new(libc::EINVAL, "no origin found")),
        };

        match inner.active_migrations.get_mut(&(origin, client)) {
            Some(active) => Ok(active.recv_queue.drain(..).collect()),
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

    pub fn update_incoming_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4, recv_queue_len: usize) {
        self.inner.borrow_mut().stats.update_incoming(local, client, recv_queue_len);

        // unsafe {
        //     static mut TMP: i32 = 0;
        //     TMP += 1;
        //     if TMP == 1000 {
        //         TMP = 0;
        //         println!("global_recv_queue_length: {}", self.inner.borrow().stats.global_recv_queue_length());
        //     }
        // }
    }

    pub fn update_outgoing_stats(&mut self) {
        self.inner.borrow_mut().stats.update_outgoing();
    }

    // pub fn should_migrate(&self) -> bool {
    //     let inner = self.inner.borrow();
    //     if inner.is_currently_migrating || inner.stats.num_of_connections() <= 100 { return false; }

    //     let recv_queue_len = inner.stats.global_recv_queue_length();
    //     // println!("check recv_queue_len {}, inner.recv_queue_length_threshold {}", recv_queue_len, inner.recv_queue_length_threshold);
    //     recv_queue_len.is_finite() && recv_queue_len > inner.recv_queue_length_threshold
    // }

    // TEMP (for migration test)
    pub fn should_migrate(&self) -> bool {
        // return false;
        static mut FLAG: i32 = 0;
        static mut NUM_MIG: i32 = 0;
        let inner = self.inner.borrow();
        // println!("currently_migration: {} , num_conn: {}", inner.is_currently_migrating, inner.stats.num_of_connections());
        
        unsafe {
            if inner.is_currently_migrating || inner.stats.num_of_connections() <= 40 || NUM_MIG >= 200 { return false; }
            if FLAG == 1000 {
                FLAG = 0;
                NUM_MIG += 1;
            }
            FLAG += 1;
            // println!("FLAG: {}", FLAG);
            FLAG == 1000
        }
    }

    pub fn queue_length_heartbeat(&mut self) {

        let mut inner = self.inner.borrow_mut();
        let queue_len = inner.stats.global_recv_queue_length() as u32;
        if queue_len as f64 > inner.recv_queue_length_threshold{
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
                    DemiBuffer::from_slice(&data).expect("Heartbeat exceeded DemiBuffer size limit"),
                    false,
                );

                inner.rt.transmit(Box::new(segment));
            }
        }
    }

    pub fn stop_tracking_connection_stats(&mut self, local: SocketAddrV4, client: SocketAddrV4) {
        self.inner.borrow_mut().stats.stop_tracking_connection(local, client)
    }

    pub fn print_stats(&self) {
        println!("global_recv_queue_length: {}", self.inner.borrow().stats.global_recv_queue_length());
        // println!("ratio: {}", self.inner.borrow().stats.get_rx_tx_ratio());
        // println!("TCPMig stats: {:?}", inner.stats);
    }
}

impl<const N: usize> Inner<N> {
    fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
        //scheduler: Scheduler,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        //arp: ArpPeer,
    ) -> Self {
        let recv_queue_length_threshold = match std::env::var("RECV_QUEUE_LEN") {
            Ok(val) => val.parse().expect("RECV_QUEUE_LEN should be a number"),
            Err(..) => BASE_RECV_QUEUE_LENGTH_THRESHOLD,
        };

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
            origins: HashMap::new(),
            incoming_connections: HashSet::new(),
            stats: TcpMigStats::new(recv_queue_length_threshold),
            is_currently_migrating: false,
            recv_queue_length_threshold,
            self_udp_port: SELF_UDP_PORT, // TEMP
            migrated_out_connections: HashSet::new(),
            additional_mig_delay: env::var("MIG_DELAY")
            .unwrap_or_else(|_| String::from("0")) // Default value if DELAY is not set
            .parse::<u32>()
            .expect("Invalid DELAY value"),
            //background: handle,
        }
    }
}

impl HeartbeatData {
    fn new() -> Self {
        Self { last_heartbeat_instant: Instant::now() }
    }
}