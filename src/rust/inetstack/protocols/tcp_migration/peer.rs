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
            ipv4::Ipv4Header, tcp::segment::TcpHeader,
        },
    runtime::{
        fail::Fail,
        memory::Buffer,
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
use std::{str::FromStr, cell::RefCell};
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
    /// key = (origin, target, remote).
    active_migrations: HashMap<(SocketAddrV4, SocketAddrV4, SocketAddrV4), ActiveMigration>,

    /* /// The background co-routine retransmits TCPMig packets.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle, */
}

#[derive(Clone)]
pub struct TcpMigPeer {
    inner: Rc<RefCell<Inner>>,
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
                    "failed to schedule background co-routine for UDP module",
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
    pub fn receive(&mut self, ipv4_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcpmig::receive");

        // Parse datagram.
        let (hdr, data) = TcpMigHeader::parse(ipv4_hdr, buf)?;
        debug!("TCPMig received {:?}", hdr);

        // Pass this packet to the correct migration connection.

        Ok(())
    }

    /// Sends a TCPMig segment.
    fn send(
        rt: Rc<dyn NetworkRuntime>,
        local_ipv4_addr: Ipv4Addr,
        local_link_addr: MacAddress,
        remote_link_addr: MacAddress,
        buf: Buffer,
        local: &SocketAddrV4,
        remote: &SocketAddrV4,
    ) {
        /* let tcpmig_hdr = TcpMigHeader::new(local.port(), remote.port());
        debug!("TCPMig send {:?}", tcpmig_hdr);

        let segment = TcpMigSegment::new(
            Ethernet2Header::new(remote_link_addr, local_link_addr, EtherType2::Ipv4),
            Ipv4Header::new(local_ipv4_addr, remote.ip().clone(), IpProtocol::UDP),
            tcpmig_hdr,
            buf,
        );
        rt.transmit(Box::new(segment)); */
    }

    // For now, assume only called when receiving.
    pub fn init_migration_out(&mut self, ipv4_hdr: &Ipv4Header, tcp_hdr: &TcpHeader) {
        // TEMP
        let target = SocketAddrV4::from_str("10.0.1.9:22222").unwrap();
        let target_mac = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xc5, 0xad]);
        let origin = SocketAddrV4::new(ipv4_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ipv4_hdr.get_src_addr(), tcp_hdr.src_port);

        let mut inner = self.inner.borrow_mut();

        let active = ActiveMigration::new();
        if let Some(..) = inner.active_migrations.insert((origin, target, remote), active) {
            todo!("duplicate active migration");
        }

        eprintln!("NOW SENDING PREPARE MIGRATION");
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
            rt: rt.clone(),
            //arp,
            local_link_addr,
            local_ipv4_addr,
            active_migrations: HashMap::new(),
            //background: handle,
        }
    }
}