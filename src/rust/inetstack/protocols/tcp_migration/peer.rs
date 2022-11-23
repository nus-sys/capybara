// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{segment::{
        TcpMigHeader,
        TcpMigSegment,
    }, active::ActiveMigration};
use crate::{
    inetstack::protocols::{
            arp::ArpPeer,
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ip::IpProtocol,
            ipv4::Ipv4Header, tcp::{segment::TcpHeader, migration::TcpState, TcpPeer}, tcp_migration::{MigrationStage, active::MigrationRequestStatus},
        },
    runtime::{
        fail::Fail,
        memory::{Buffer, DataBuffer},
        network::{
            types::MacAddress,
            NetworkRuntime,
        },
        QDesc,
    },
    scheduler::{
        Scheduler,
        SchedulerHandle,
    },
};
use ::libc::{
    EAGAIN,
    EBADF,
    EEXIST,
};
use std::{cell::RefCell};
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

        if hdr.stage == MigrationStage::PrepareMigration {
            let active = ActiveMigration::new(
                inner.rt.clone(),
                inner.local_ipv4_addr,
                inner.local_link_addr,
                hdr.origin.ip().clone(),
                MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]), // TEMP
                hdr.origin,
                hdr.target,
                hdr.remote,
            );
            if let Some(..) = inner.active_migrations.insert(key, active) {
                todo!("duplicate active migration");
            }
            inner.origins.insert((hdr.target, hdr.remote), hdr.origin);
        }

        let active = match inner.active_migrations.get_mut(&key) {
            Some(active) => active,
            None => return Err(Fail::new(libc::EINVAL, "no such active migration")),
        };

        if active.process_packet(tcp_peer, hdr, buf)? == MigrationRequestStatus::Rejected {
            todo!("handle migration rejection");
        }

        Ok(())
    }

    pub fn initiate_migration(&mut self, origin_port: u16, remote: SocketAddrV4) {
        let mut inner = self.inner.borrow_mut();

        let origin = SocketAddrV4::new(inner.local_ipv4_addr, origin_port);
        let target = SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), origin_port); // TEMP
        let key = (origin, remote);

        let active = ActiveMigration::new(
            inner.rt.clone(),
            inner.local_ipv4_addr,
            inner.local_link_addr,
            target.ip().clone(),
            MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xc5, 0xad]), // TEMP
            origin,
            target,
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

    pub fn try_buffer_packet(&self, target: SocketAddrV4, remote: SocketAddrV4, ip_hdr: Ipv4Header, tcp_hdr: TcpHeader, buf: Buffer) -> Result<(), ()> {
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



    // TEMP
    pub fn should_migrate(&self) -> bool {
        static mut FLAG: i32 = 0;
        unsafe {
            FLAG += 1;
            FLAG == 12
        }
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
            //background: handle,
        }
    }
}