// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

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
        },
    runtime::{
        fail::Fail,
        memory::Buffer,
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
    },
};
use std::{cell::RefCell, collections::{VecDeque, HashSet}};
use ::std::{
    collections::HashMap,
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

const BASE_RX_TX_THRESHOLD_RATIO: f64 = 2.0;

//======================================================================================================================
// Structures
//======================================================================================================================

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

    /// Connections being actively migrated in/out.
    /// 
    /// key = (origin, remote).
    active_migrations: HashMap<(SocketAddrV4, SocketAddrV4), ActiveMigration>,

    /// Origins. Only used on the target side to get the origin of redirected packets.
    /// 
    /// key = (target, remote).
    origins: HashMap<(SocketAddrV4, SocketAddrV4), SocketAddrV4>,

    /// Connections ready to be migrated-in.
    /// 
    /// key = (local, remote).
    incoming_connections: HashSet<(SocketAddrV4, SocketAddrV4)>,

    stats: TcpMigStats,

    is_currently_migrating: bool,

    rx_tx_threshold_ratio: f64,

    /* /// The background co-routine retransmits TCPMig packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle, */
}

#[derive(Clone)]
pub struct TcpMigPeer {
    inner: Rc<RefCell<Inner>>,
}

pub struct MigrationHandle {
    origin: SocketAddrV4,
    remote: SocketAddrV4,
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
        println!("RX_TX_RATIO: {}", inner.rx_tx_threshold_ratio);

        Ok(Self{inner: Rc::new(RefCell::new(inner))})
    }

    /// Consumes the payload from a buffer.
    pub fn receive(&mut self, tcp_peer: &mut TcpPeer, ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcpmig::receive");

        // Parse header.
        let (hdr, buf) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        debug!("TCPMig received {:?}", hdr);
        eprintln!("TCPMig received {:#?}", hdr);

        let key = (hdr.origin, hdr.remote);

        let mut inner = self.inner.borrow_mut();

        // First packet that target receives.
        if hdr.stage == MigrationStage::PrepareMigration {
            let active = ActiveMigration::new(
                inner.rt.clone(),
                inner.local_ipv4_addr,
                inner.local_link_addr,
                hdr.origin.ip().clone(),
                MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]), // TEMP
                hdr.origin,
                hdr.remote,
            );
            if let Some(..) = inner.active_migrations.insert(key, active) {
                todo!("duplicate active migration");
            }

            let target = SocketAddrV4::new(inner.local_ipv4_addr, hdr.origin.port());
            inner.origins.insert((target, hdr.remote), hdr.origin);
        }

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => return Err(Fail::new(libc::EINVAL, "no such active migration")),
        };

        match active.process_packet(hdr, buf)? {
            MigrationRequestStatus::Rejected => todo!("handle migration rejection"),
            MigrationRequestStatus::StateReceived(state) => {
                let (local, remote) = (state.local, state.remote);
                tcp_peer.notify_passive(state)?;
                inner.incoming_connections.insert((local, remote));
                // inner.active_migrations.remove(&(hdr.origin, hdr.remote)).expect("active migration should exist"); 
                // Shouldn't be removed yet. Should be after processing migration queue.
            },
            MigrationRequestStatus::MigrationCompleted => {
                let (local, remote) = (hdr.origin, hdr.remote);
                inner.active_migrations.remove(&(local, remote)).expect("active migration should exist");
                inner.stats.stop_tracking_connection(local, remote);
                inner.is_currently_migrating = false;
            },
            MigrationRequestStatus::Ok => (),
        };

        Ok(())
    }

    pub fn initiate_migration(&mut self) {
        let mut inner = self.inner.borrow_mut();

        let (origin, remote) = match inner.stats.get_connection_to_migrate_out() {
            Some(conn) => conn,
            None => return,
        };

        eprintln!("initiate migration for connection {} <-> {}", origin, remote);

        //let origin = SocketAddrV4::new(inner.local_ipv4_addr, origin_port);
        let target = SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), origin.port()); // TEMP
        let key = (origin, remote);

        let active = ActiveMigration::new(
            inner.rt.clone(),
            inner.local_ipv4_addr,
            inner.local_link_addr,
            target.ip().clone(),
            MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xc5, 0xad]), // TEMP
            origin,
            remote,
        );
        
        if let Some(..) = inner.active_migrations.insert(key, active) {
            todo!("duplicate active migration");
        };

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => unreachable!(),
        };

        active.initiate_migration();
        inner.is_currently_migrating = true;
    }

    pub fn can_migrate_out(&self, origin: SocketAddrV4, remote: SocketAddrV4) -> Option<MigrationHandle> {
        let key = (origin, remote);
        let inner = self.inner.borrow();
        if let Some(active) = inner.active_migrations.get(&key) {
            if active.is_prepared() {
                return Some(MigrationHandle { origin, remote })
            }
        }
        None
    }

    pub fn migrate_out(&mut self, handle: MigrationHandle, state: TcpState) {
        let key = (handle.origin, handle.remote);
        let mut inner = self.inner.borrow_mut();
        if let Some(active) = inner.active_migrations.get_mut(&key) {
            active.send_connection_state(state);
        }
    }

    pub fn try_buffer_packet(&mut self, target: SocketAddrV4, remote: SocketAddrV4, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: Buffer) -> Result<(), ()> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, remote)) {
            Some(origin) => *origin,
            None => return Err(()),
        };

        match inner.active_migrations.get_mut(&(origin, remote)) {
            Some(active) => {
                active.buffer_packet(ip_hdr, tcp_hdr, buf);
                Ok(())
            },
            None => Err(()),
        }
    }

    pub fn take_connection(&mut self, local: SocketAddrV4, remote: SocketAddrV4) -> bool {
        self.inner.borrow_mut()
        .incoming_connections.remove(&(local, remote))
    }

    pub fn take_buffer_queue(&mut self, target: SocketAddrV4, remote: SocketAddrV4) -> Result<VecDeque<(Ipv4Header, TcpHeader, Buffer)>, Fail> {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, remote)) {
            Some(origin) => *origin,
            None => return Err(Fail::new(libc::EINVAL, "no origin found")),
        };

        match inner.active_migrations.get_mut(&(origin, remote)) {
            Some(active) => Ok(active.recv_queue.drain(..).collect()),
            None => Err(Fail::new(libc::EINVAL, "no active migration found")),
        }
    }

    pub fn complete_migrating_in(&mut self, target: SocketAddrV4, remote: SocketAddrV4) {
        let mut inner = self.inner.borrow_mut();

        let origin = match inner.origins.get(&(target, remote)) {
            Some(origin) => *origin,
            None => panic!("no origin found for connection: ({:?}, {:?})", target, remote),
        };
        inner.active_migrations.remove(&(origin, remote)).expect("active migration should exist");
    }

    pub fn update_incoming_stats(&mut self, local: SocketAddrV4, remote: SocketAddrV4, recv_queue_len: usize) {
        self.inner.borrow_mut().stats.update_incoming(local, remote, recv_queue_len);

        unsafe {
            static mut TMP: i32 = 0;
            TMP += 1;
            if TMP == 500 {
                TMP = 0;
                println!("ratio: {}", self.inner.borrow().stats.get_rx_tx_ratio());
            }
        }
    }

    pub fn update_outgoing_stats(&mut self) {
        self.inner.borrow_mut().stats.update_outgoing();
    }

    pub fn should_migrate(&self) -> bool {
        let inner = self.inner.borrow();
        if inner.is_currently_migrating { return false; }

        let ratio = inner.stats.get_rx_tx_ratio();
        ratio.is_finite() && ratio > inner.rx_tx_threshold_ratio
    }

    pub fn print_stats(&self) {
        let inner = self.inner.borrow();
        println!("ratio: {}", self.inner.borrow().stats.get_rx_tx_ratio());
        println!("TCPMig stats: {:?}", inner.stats);
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
        Self {
            rt,
            //arp,
            local_link_addr,
            local_ipv4_addr,
            active_migrations: HashMap::new(),
            origins: HashMap::new(),
            incoming_connections: HashSet::new(),
            stats: TcpMigStats::new(),
            is_currently_migrating: false,
            rx_tx_threshold_ratio: match std::env::var("RX_TX_RATIO") {
                Ok(val) => val.parse().expect("RX_TX_RATIO should be a number"),
                Err(..) => BASE_RX_TX_THRESHOLD_RATIO,
            },
            //background: handle,
        }
    }
}