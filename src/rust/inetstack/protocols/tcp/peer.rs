// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    active_open::ActiveOpenSocket,
    established::EstablishedSocket,
    isn_generator::IsnGenerator,
    passive_open::PassiveSocket,
};
use crate::{
    inetstack::protocols::{
        arp::ArpPeer,
        ethernet2::{
            EtherType2,
            Ethernet2Header,
        },
        ip::{
            EphemeralPorts,
            IpProtocol,
        },
        ipv4::Ipv4Header,
        tcp::{
            established::ControlBlock,
            operations::{
                AcceptFuture,
                ConnectFuture,
                PopFuture,
                PushFuture,
            },
            segment::{
                TcpHeader,
                TcpSegment,
            },
            SeqNumber,
        },
    },
    runtime::{
        fail::Fail,
        memory::Buffer,
        network::{
            config::TcpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        timer::TimerRc,
        QDesc,
    },
    scheduler::scheduler::Scheduler,
    capy_time_log,
};

use ::futures::channel::mpsc;
use ::libc::{
    EAGAIN,
    EBADF,
    EBUSY,
    EINPROGRESS,
    EINVAL,
    ENOTCONN,
    ENOTSUP,
    EOPNOTSUPP,
};
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};

use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    collections::{
        HashMap,
        hash_map::Entry,
    },
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

#[cfg(feature = "tcp-migration")]
use state::TcpState;
#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::{
    tcp::stats::Stats,
    tcpmig::{TcpMigPeer, TcpmigPollState}
};

#[cfg(feature = "profiler")]
use crate::timer;

use crate::{capy_profile, capy_log, capy_log_mig};

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Debug)]
pub enum Socket {
    Inactive { local: Option<SocketAddrV4> },
    Listening { local: SocketAddrV4 },
    Connecting { local: SocketAddrV4, remote: SocketAddrV4 },
    Established { local: SocketAddrV4, remote: SocketAddrV4 },

    #[cfg(feature = "tcp-migration")]
    MigratedOut { local: SocketAddrV4, remote: SocketAddrV4 },
}

//==============================================================================
// Structures
//==============================================================================

pub struct Inner {
    isn_generator: IsnGenerator,

    ephemeral_ports: EphemeralPorts,

    // FD -> local port
    pub sockets: HashMap<QDesc, Socket>,
    // (Local, Remote) -> FD
    pub qds: HashMap<(SocketAddrV4, SocketAddrV4), QDesc>,

    passive: HashMap<SocketAddrV4, PassiveSocket>,
    connecting: HashMap<(SocketAddrV4, SocketAddrV4), ActiveOpenSocket>,
    established: HashMap<(SocketAddrV4, SocketAddrV4), EstablishedSocket>,

    rt: Rc<dyn NetworkRuntime>,
    scheduler: Scheduler,
    clock: TimerRc,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    tcp_config: TcpConfig,
    arp: ArpPeer,
    rng: Rc<RefCell<SmallRng>>,

    dead_socket_tx: mpsc::UnboundedSender<QDesc>,

    #[cfg(feature = "tcp-migration")]
    tcpmig: TcpMigPeer,
    #[cfg(feature = "tcp-migration")]
    stats: Stats,
    #[cfg(feature = "tcp-migration")]
    tcpmig_poll_state: Rc<TcpmigPollState>,
}

pub struct TcpPeer {
    pub(super) inner: Rc<RefCell<Inner>>,
}

//==============================================================================
// Associated FUnctions
//==============================================================================

impl TcpPeer {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        rng_seed: [u8; 32],

        #[cfg(feature = "tcp-migration")]
        tcpmig_poll_state: Rc<TcpmigPollState>,
    ) -> Result<Self, Fail> {
        let (tx, rx) = mpsc::unbounded();
        
        #[cfg(feature = "tcp-migration")]
        let tcpmig = TcpMigPeer::new(rt.clone(), local_link_addr, local_ipv4_addr);

        let inner = Rc::new(RefCell::new(Inner::new(
            rt.clone(),
            scheduler,
            clock,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng_seed,

            #[cfg(feature = "tcp-migration")]
            tcpmig,
            #[cfg(feature = "tcp-migration")]
            tcpmig_poll_state,

            tx,
            rx,
        )));
        Ok(Self { inner })
    }

    /// Opens a TCP socket.
    pub fn do_socket(&self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcp::socket");
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();
        match inner.sockets.contains_key(&qd) {
            false => {
                let socket: Socket = Socket::Inactive { local: None };
                inner.sockets.insert(qd, socket);
                Ok(())
            },
            true => return Err(Fail::new(EBUSY, "queue descriptor in use")),
        }
    }

    pub fn bind(&self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Check if address is already bound.
        for (_, socket) in &inner.sockets {
            match socket {
                Socket::Inactive { local: Some(local) }
                | Socket::Listening { local }
                | Socket::Connecting { local, remote: _ }
                | Socket::Established { local, remote: _ }
                    if *local == addr =>
                {
                    return Err(Fail::new(libc::EADDRINUSE, "address already in use"))
                },
                _ => (),
            }
        }

        // Check if this is an ephemeral port.
        if EphemeralPorts::is_private(addr.port()) {
            // Allocate ephemeral port from the pool, to leave  ephemeral port allocator in a consistent state.
            inner.ephemeral_ports.alloc_port(addr.port())?
        }

        // Check if we have to handle wildcard port binding.
        if addr.port() == 0 {
            // Allocate ephemeral port.
            // TODO: we should free this when closing.
            let new_port: u16 = inner.ephemeral_ports.alloc_any()?;
            addr.set_port(new_port);
        }
        
        #[cfg(feature = "tcp-migration")]
        {
            inner.tcpmig.set_port(addr.port());
        }
        
        // Issue operation.
        let ret: Result<(), Fail> = match inner.sockets.get_mut(&qd) {
            Some(Socket::Inactive { ref mut local }) => match *local {
                Some(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
                None => {
                    *local = Some(addr);
                    Ok(())
                },
            },
            _ => Err(Fail::new(EBADF, "invalid queue descriptor")),
        };

        // Handle return value.
        match ret {
            Ok(x) => Ok(x),
            Err(e) => {
                // Rollback ephemeral port allocation.
                if EphemeralPorts::is_private(addr.port()) {
                    inner.ephemeral_ports.free(addr.port());
                }
                Err(e)
            },
        }
    }

    pub fn receive(&self, ip_header: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        self.inner.borrow_mut().receive(ip_header, buf)
    }

    // Marks the target socket as passive.
    pub fn listen(&self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Get bound address while checking for several issues.
        let local: SocketAddrV4 = match inner.sockets.get_mut(&qd) {
            Some(Socket::Inactive { local: Some(local) }) => *local,
            Some(Socket::Listening { local: _ }) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
            Some(Socket::Inactive { local: None }) => {
                return Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address"))
            },
            Some(Socket::Connecting { local: _, remote: _ }) => {
                return Err(Fail::new(libc::EINVAL, "socket is connecting"))
            },
            Some(Socket::Established { local: _, remote: _ }) => {
                return Err(Fail::new(libc::EINVAL, "socket is connected"))
            },
            _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Check if there isn't a socket listening on this address/port pair.
        if inner.passive.contains_key(&local) {
            return Err(Fail::new(
                libc::EADDRINUSE,
                "another socket is already listening on the same address/port pair",
            ));
        }

        let nonce: u32 = inner.rng.borrow_mut().gen();
        let socket = PassiveSocket::new(
            local,
            backlog,
            inner.rt.clone(),
            inner.scheduler.clone(),
            inner.clock.clone(),
            inner.tcp_config.clone(),
            inner.local_link_addr,
            inner.arp.clone(),
            nonce
        );
        assert!(inner.passive.insert(local, socket).is_none());
        inner.sockets.insert(qd, Socket::Listening { local });
        Ok(())
    }

    /// Accepts an incoming connection.
    pub fn do_accept(&self, qd: QDesc, new_qd: QDesc) -> AcceptFuture {
        AcceptFuture::new(qd, new_qd, self.inner.clone())
    }

    /// Handles an incoming connection.
    pub fn poll_accept(&self, qd: QDesc, new_qd: QDesc, ctx: &mut Context) -> Poll<Result<QDesc, Fail>> {
        let mut inner_: RefMut<Inner> = self.inner.borrow_mut();
        let inner: &mut Inner = &mut *inner_;

        let local: SocketAddrV4 = match inner.sockets.get(&qd) {
            Some(Socket::Listening { local }) => *local,
            Some(..) => return Poll::Ready(Err(Fail::new(EOPNOTSUPP, "socket not listening"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad file descriptor"))),
        };

        let passive: &mut PassiveSocket = inner.passive.get_mut(&local).expect("sockets/local inconsistency");
        let cb: ControlBlock = match passive.poll_accept(ctx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(e)) => e,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
        };
        let remote = cb.get_remote();

        // Enable stats tracking.
        #[cfg(feature = "tcp-migration")]
        cb.enable_stats(&mut inner.stats);

        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let key: (SocketAddrV4, SocketAddrV4) = (local, remote);

        let socket: Socket = Socket::Established { local, remote };

        // eprintln!("CONNECTION ESTABLISHED (REMOTE: {:?}, new_qd: {:?})", remote, new_qd);

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        match inner.sockets.insert(new_qd, socket) {
            None => (),
            #[cfg(feature = "tcp-migration")]
            Some(Socket::MigratedOut { .. }) => { capy_log_mig!("migrated socket QD overwritten"); },
            _ => panic!("duplicate queue descriptor in sockets table"),
        }

        assert!(inner.qds.insert((local, remote), new_qd).is_none(), "duplicate entry in qds table");

        /* activate this for recv_queue_len vs mig_lat eval */
        /* static mut NUM_MIG: u32 = 0;
        if established.cb.receiver.recv_queue_len() == 1000 {
            // The key exists in the hashmap, and mig_socket now contains the value.
            // eprintln!("recv_queue_len to be mig: {}", mig_socket.cb.receiver.recv_queue_len());
            capy_time_log!("INIT_MIG,({})", key.1);
            established.cb.disable_stats();
            unsafe{ NUM_MIG += 1; }
            if unsafe{ NUM_MIG } < 10000 {
                inner.tcpmig.initiate_migration(key, new_qd);
            }
        } */
        /* activate this for recv_queue_len vs mig_lat eval */

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.established.insert(key, established).is_some() {
            panic!("duplicate queue descriptor in established sockets table");
        }
        capy_time_log!("CONN_ESTABLISHED,({})", remote);
        Poll::Ready(Ok(new_qd))
    }

    pub fn connect(&self, qd: QDesc, remote: SocketAddrV4) -> Result<ConnectFuture, Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        // Get local address bound to socket.
        let local: SocketAddrV4 = match inner.sockets.get_mut(&qd) {
            // Handle unbound socket.
            Some(Socket::Inactive { local: None }) => {
                // TODO: we should free this when closing.
                let local_port: u16 = inner.ephemeral_ports.alloc_any()?;
                SocketAddrV4::new(inner.local_ipv4_addr, local_port)
            },
            // Handle bound socket.
            Some(Socket::Inactive { local: Some(local) }) => *local,
            Some(Socket::Connecting { local: _, remote: _ }) => Err(Fail::new(libc::EALREADY, "socket is connecting"))?,
            Some(Socket::Established { local: _, remote: _ }) => Err(Fail::new(libc::EISCONN, "socket is connected"))?,
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor"))?,
        };

        // Update socket state.
        match inner.sockets.get_mut(&qd) {
            Some(socket) => {
                *socket = Socket::Connecting { local, remote };
            },
            None => {
                // This should not happen, because we've queried the sockets table above.
                error!("socket is no longer in the sockets table?");
                return Err(Fail::new(libc::EAGAIN, "failed to retrieve socket from sockets table"));
            },
        };

        // Create active socket.
        let local_isn: SeqNumber = inner.isn_generator.generate(&local, &remote);
        let socket: ActiveOpenSocket = ActiveOpenSocket::new(
            inner.scheduler.clone(),
            local_isn,
            local,
            remote,
            inner.rt.clone(),
            inner.tcp_config.clone(),
            inner.local_link_addr,
            inner.clock.clone(),
            inner.arp.clone(),
        );

        // Insert socket in connecting table.
        if inner.connecting.insert((local, remote), socket).is_some() {
            // This should not happen, unless we are leaking entries when transitioning to established state.
            error!("socket is already connecting?");
            Err(Fail::new(libc::EALREADY, "socket is connecting"))?;
        }

        Ok(ConnectFuture {
            fd: qd,
            inner: self.inner.clone(),
        })
    }

    pub fn poll_recv(&self, fd: QDesc, ctx: &mut Context) -> Poll<Result<Buffer, Fail>> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(Socket::Connecting { .. }) => return Poll::Ready(Err(Fail::new(EINPROGRESS, "socket connecting"))),
            Some(Socket::Inactive { .. }) => return Poll::Ready(Err(Fail::new(EBADF, "socket inactive"))),
            Some(Socket::Listening { .. }) => return Poll::Ready(Err(Fail::new(ENOTCONN, "socket listening"))),
            #[cfg(feature = "tcp-migration")]
            Some(Socket::MigratedOut { .. }) => return Poll::Ready(Err(Fail::new(crate::ETCPMIG, "socket migrated out"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };

        capy_log!("\n\npolling POP on {:?}", key);
        match inner.established.get(&key) {
            Some(ref s) => s.poll_recv(ctx),
            None => Poll::Ready(Err(Fail::new(ENOTCONN, "connection not established"))),
        }
    }

    pub fn push(&self, fd: QDesc, buf: Buffer) -> PushFuture {
        let err = match self.send(fd, buf) {
            Ok(()) => None,
            Err(e) => Some(e),
        };
        PushFuture { fd, err }
    }

    pub fn pop(&self, fd: QDesc) -> Result<PopFuture, Fail> {
        Ok(PopFuture {
            fd,
            inner: self.inner.clone(),
        })
    }

    fn send(&self, fd: QDesc, buf: Buffer) -> Result<(), Fail> {
        let inner = self.inner.borrow_mut();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                eprintln!("connection not established");
                return Err(Fail::new(ENOTCONN, "connection not established"))
            },
            None => {
                eprintln!("bad queue descriptor");
                return Err(Fail::new(EBADF, "bad queue descriptor"))
            },
        };
        let send_result = match inner.established.get(&key) {
            Some(ref s) => s.send(buf),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        };

        // #[cfg(feature = "tcp-migration")]
        // {
        //     inner.tcpmig.update_outgoing_stats();
        // }

        send_result
    }

    /// Closes a TCP socket.
    pub fn do_close(&self, qd: QDesc) -> Result<(), Fail> {
        let mut inner: RefMut<Inner> = self.inner.borrow_mut();

        match inner.sockets.remove(&qd) {
            Some(Socket::Established { local, remote }) => {
                let key: (SocketAddrV4, SocketAddrV4) = (local, remote);
                match inner.established.get(&key) {
                    Some(ref s) => s.close()?,
                    None => return Err(Fail::new(ENOTCONN, "connection not established")),
                }
            },

            Some(..) => return Err(Fail::new(ENOTSUP, "close not implemented for listening sockets")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        }

        Ok(())
    }

    pub fn remote_mss(&self, fd: QDesc) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.remote_mss()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }

    pub fn current_rto(&self, fd: QDesc) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.current_rto()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }

    pub fn endpoints(&self, fd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        let inner = self.inner.borrow();
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => return Err(Fail::new(ENOTCONN, "connection not established")),
            None => return Err(Fail::new(EBADF, "bad queue descriptor")),
        };
        match inner.established.get(&key) {
            Some(ref s) => Ok(s.endpoints()),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        }
    }
}

impl Inner {
    fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer,
        rng_seed: [u8; 32],

        #[cfg(feature = "tcp-migration")]
        tcpmig: TcpMigPeer,
        #[cfg(feature = "tcp-migration")]
        tcpmig_poll_state: Rc<TcpmigPollState>,

        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        _dead_socket_rx: mpsc::UnboundedReceiver<QDesc>,
    ) -> Self {
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);
        let nonce: u32 = rng.gen();

        /* capy_log!("Creating new TcpPeer::Inner"); */
        Self {
            isn_generator: IsnGenerator::new(nonce),
            ephemeral_ports,
            sockets: HashMap::new(),
            qds: HashMap::new(),
            passive: HashMap::new(),
            connecting: HashMap::new(),
            established: HashMap::new(),
            rt,
            scheduler,
            clock,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng: Rc::new(RefCell::new(rng)),
            dead_socket_tx,

            #[cfg(feature = "tcp-migration")]
            tcpmig,
            #[cfg(feature = "tcp-migration")]
            stats: Stats::new(),
            #[cfg(feature = "tcp-migration")]
            tcpmig_poll_state,
        }
    }

    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        capy_log!("\n\n[RX]");
        
        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);
        capy_log!("{:?} => {:?}", remote, local);
        capy_log!("SERVER RECEIVE TCP seq_num: {}", tcp_hdr.seq_num);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(EINVAL, "invalid address type"));
        }
        let key = (local, remote);

        if let Some(s) = self.established.get(&key) {
            /* activate this for recv_queue_len vs mig_lat eval */
            // let is_data_empty = data.is_empty();
            /* activate this for recv_queue_len vs mig_lat eval */

            debug!("Routing to established connection: {:?}", key);
            s.receive(&mut tcp_hdr,data);

            /* activate this for recv_queue_len vs mig_lat eval */
            /* if !is_data_empty {
                let mig_key = (
                    SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10000),
                    SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 7), 201));
                let qd = self.qds.get(&mig_key).ok_or_else(|| Fail::new(EBADF, "socket not exist"))?;
                if let Some(mig_socket) = self.established.get(&mig_key) {
                    if mig_socket.cb.receiver.recv_queue_len() == 1000 {
                        // The key exists in the hashmap, and mig_socket now contains the value.
                        // eprintln!("recv_queue_len to be mig: {}", mig_socket.cb.receiver.recv_queue_len());
                        capy_time_log!("INIT_MIG,({})", mig_key.1);
                        s.cb.disable_stats();
                        self.tcpmig.initiate_migration(mig_key, *qd);
                    }
                } else {
                    // The key does not exist in the esablished hashmap, panic.
                    panic!("Key not found in established HashMap: {:?}", mig_key);
                }
            } */
            /* activate this for recv_queue_len vs mig_lat eval */

            return Ok(());
        }
        if let Some(s) = self.connecting.get_mut(&key) {
            debug!("Routing to connecting connection: {:?}", key);
            capy_log!("Routing to connecting connection: {:?}", key);
        
            s.receive(&tcp_hdr);
            return Ok(());
        }
        
        #[cfg(feature = "tcp-migration")]
        // Check if migrating queue exists. If yes, push buffer to queue and return, else continue normally.
        let (tcp_hdr, data) = match self.tcpmig.try_buffer_packet(remote, tcp_hdr, data) {
            Ok(()) => return Ok(()),
            Err(val) => val,
        };

        /* #[cfg(feature = "tcp-migration")]
        if self.tcpmig.is_migrated_out(remote) {
            capy_log!("Dropped packet received on migrated out connection ({local}, {remote})");
            return Ok(());
        } */

        let (local, _) = key;
        if let Some(s) = self.passive.get_mut(&local) {
            debug!("Routing to passive connection: {:?}", local);
            capy_log!("Routing to passive connection: {:?}", local);
            return s.receive(ip_hdr, &tcp_hdr);
        }

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
        capy_log!("Sending RST for {:?}, {:?}", local, remote);
        self.send_rst(&local, &remote)?;
        Ok(())
    }

    fn send_rst(&mut self, local: &SocketAddrV4, remote: &SocketAddrV4) -> Result<(), Fail> {
        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self
            .arp
            .try_query(remote.ip().clone())
            .ok_or(Fail::new(EINVAL, "detination not in ARP cache"))?;

        let mut tcp_hdr = TcpHeader::new(local.port(), remote.port());
        tcp_hdr.rst = true;

        let segment = TcpSegment {
            ethernet2_hdr: Ethernet2Header::new(remote_link_addr, self.local_link_addr, EtherType2::Ipv4),
            ipv4_hdr: Ipv4Header::new(local.ip().clone(), remote.ip().clone(), IpProtocol::TCP),
            tcp_hdr,
            data: None,
            tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
        };
        self.rt.transmit(Box::new(segment));

        Ok(())
    }

    pub(super) fn poll_connect_finished(&mut self, fd: QDesc, context: &mut Context) -> Poll<Result<(), Fail>> {
        let key = match self.sockets.get(&fd) {
            Some(Socket::Connecting { local, remote }) => (*local, *remote),
            Some(..) => return Poll::Ready(Err(Fail::new(EAGAIN, "socket not connecting"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };

        let result = {
            let socket = match self.connecting.get_mut(&key) {
                Some(s) => s,
                None => return Poll::Ready(Err(Fail::new(EAGAIN, "socket not connecting"))),
            };
            match socket.poll_result(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => r,
            }
        };
        self.connecting.remove(&key);

        let cb = result?;
        let socket = EstablishedSocket::new(cb, fd, self.dead_socket_tx.clone());
        assert!(self.established.insert(key, socket).is_none());
        let (local, remote) = key;
        self.sockets.insert(fd, Socket::Established { local, remote });

        Poll::Ready(Ok(()))
    }

    pub fn sockets(&self) -> &HashMap<QDesc, Socket> {
        &self.sockets
    }
}

//==========================================================================================================================
// TCP Migration
//==========================================================================================================================

//==============================================================================
//  Implementations
//==============================================================================

#[cfg(feature = "tcp-migration")]
impl TcpPeer {
    pub fn receive_tcpmig(&self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        self.inner.borrow_mut().receive_tcpmig(ip_hdr, buf)
    }

    pub fn migrate_out_and_send(&mut self, qd: QDesc) {
        let mut inner = self.inner.borrow_mut();
        let state = inner.migrate_out_connection(qd).unwrap();
        inner.tcpmig.send_tcp_state(state);
    }

    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn initiate_migration(&mut self, conn: (SocketAddrV4, SocketAddrV4)) {
        capy_profile!("prepare");
        capy_log_mig!("INIT MIG");

        let mut inner = self.inner.borrow_mut();
        let qd = *inner.qds.get(&conn).expect("no QD found for connection");

        // Disable stats for this connection.
        inner.established.get(&conn).expect("connection not in established table")
            .cb.disable_stats();

        // TODO: Turn off stats for this connection.
        inner.tcpmig.initiate_migration(conn, qd);
    }
    
    #[cfg(feature = "manual-tcp-migration")]
    pub fn initiate_migration(&mut self, qd: QDesc) -> Result<(), Fail> {
        capy_profile!("prepare");
        capy_log_mig!("INIT MIG");
        let mut inner = self.inner.borrow_mut();
        let conn = match inner.sockets.get(&qd) {
            None => {
                debug!("No entry in `sockets` for fd: {:?}", qd);
                return Err(Fail::new(EBADF, "socket does not exist"));
            },
            Some(Socket::Established { local, remote }) => {
                (*local, *remote)
            },
            Some(..) => {
                return Err(Fail::new(EBADF, "unsupported socket variant for migrating out"));
            },
        };

        // Disable stats for this connection.
        inner.established.get(&conn).expect("connection not in established table")
            .cb.disable_stats();

        capy_time_log!("INIT_MIG,({})", conn.1);
        inner.tcpmig.initiate_migration(conn, qd);
        Ok(())

        /* NON-CONCURRENT MIGRATION */
        /* if(conn.1.port() == 301){
            capy_time_log!("INIT_MIG,({})", conn.1); 
            inner.tcpmig.initiate_migration(conn, qd);
            Ok(())
        }else{
            return Err(Fail::new(EBADF, "this connection is not for migration"));
        } */
        /* NON-CONCURRENT MIGRATION */
    }

    pub fn take_migrated_data(&mut self, qd: QDesc) -> Result<Option<Buffer>, Fail> {
        let mut inner = self.inner.borrow_mut();
        let remote = match inner.sockets.get(&qd) {
            None => {
                debug!("No entry in `sockets` for fd: {:?}", qd);
                return Err(Fail::new(EBADF, "socket does not exist"));
            },
            Some(Socket::Established { remote, .. }) => {
                *remote
            },
            Some(..) => {
                return Err(Fail::new(EBADF, "unsupported socket variant for migrating out"));
            },
        };

        Ok(inner.tcpmig.take_incoming_user_data(remote))
    }

    pub fn poll_stats(&mut self) {
        self.inner.borrow_mut().stats.poll();
    }

    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_migrate(&mut self) -> Option<arrayvec::ArrayVec<(SocketAddrV4, SocketAddrV4), { super::stats::MAX_EXTRACTED_CONNECTIONS }>> {
        self.inner.borrow_mut().stats.connections_to_migrate()
    }

    pub fn global_recv_queue_length(&self) -> usize {
        self.inner.borrow().stats.global_recv_queue_length()
    }
}

#[cfg(feature = "tcp-migration")]
impl Inner {
    fn receive_tcpmig(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        use super::super::tcpmig::TcpmigReceiveStatus;

        match self.tcpmig.receive(ip_hdr, buf)? {
            TcpmigReceiveStatus::Ok | TcpmigReceiveStatus::MigrationCompleted => {},
            TcpmigReceiveStatus::PrepareMigrationAcked(qd) => {
                // Set the qd to be freed.
                self.tcpmig_poll_state.set_qd(qd);

                // If fast migration is allowed, migrate out the connection immediately.
                if self.tcpmig_poll_state.is_fast_migrate_enabled() {
                    let state = self.migrate_out_connection(qd)?;
                    self.tcpmig.send_tcp_state(state);
                }
            },
            TcpmigReceiveStatus::StateReceived(state, buffered) => {
                self.migrate_in_connection(state, buffered)?;
            },
        };
        Ok(())
    }

    /// 1) Mark this socket as migrated out.
    /// 2) Check if this connection is established one.
    /// 3) Remove socket from Established hashmap.
    fn migrate_out_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        // Mark socket as migrated out.
        let conn = match self.sockets.get_mut(&qd) {
            None => panic!("invalid QD for migrating out"),
            Some(s) => {
                let (local, remote) = match s {
                    Socket::Established { local, remote } => (*local, *remote),
                    s => panic!("invalid socket type for migrating out: {:?}", s),
                };
                *s = Socket::MigratedOut { local, remote };
                (local, remote)
            }
        };
        
        // Remove from `qds`.
        self.qds.remove(&conn).unwrap();

        // 2) Check if this connection is established one
        let entry = match self.established.entry(conn) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(_) => panic!("inconsistency between `sockets` and `established`"),
        };

        // 3) Remove connection from Established hashmap.
        let cb = entry.remove().cb;
        Ok(TcpState::new(cb.as_ref().into()))
    }

    fn migrate_in_connection(&mut self, state: TcpState, buffered: Vec<(TcpHeader, Buffer)>) -> Result<(), Fail> {
        // TODO: Handle user data from the state.

        // Convert state to control block.
        let cb = ControlBlock::from_state(
            self.rt.clone(),
            self.scheduler.clone(),
            self.clock.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            self.tcp_config.get_ack_delay_timeout(),
            state.cb
        );

        // Receive all target-buffered packets into the CB.
        for (mut hdr, data) in buffered {
            capy_log_mig!("start receiving target-buffered packets into the CB");
            cb.receive(&mut hdr, data);
        }

        match self.passive.get_mut(&cb.get_local()) {
            Some(passive) => passive.push_migrated_in(cb),
            None => return Err(Fail::new(libc::EBADF, "socket not listening")),
        };

        Ok(())
    }
}

//==============================================================================
// TCP State
//==============================================================================

#[cfg(feature = "tcp-migration")]
pub mod state {
    use std::net::SocketAddrV4;

    use crate::{inetstack::protocols::tcp::established::ControlBlockState, runtime::memory::{Buffer, DataBuffer}};

    pub trait Serialize {
        /// Serializes into the buffer and returns its unused part.
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8];
    }
    
    #[cfg(feature = "tcp-migration")]
    pub trait Deserialize: Sized {
        /// Deserializes and removes the deserialised part from the buffer.
        fn deserialize_from(buf: &mut Buffer) -> Self;
    }

    #[derive(Debug, PartialEq, Eq)]
    pub struct TcpState {
        pub cb: ControlBlockState
    }

    impl Serialize for TcpState {
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
            self.cb.serialize_into(buf)
        }
    }

    impl TcpState {
        pub fn new(cb: ControlBlockState) -> Self {
            Self { cb }
        }

        pub fn remote(&self) -> SocketAddrV4 {
            self.cb.remote()
        }

        pub fn connection(&self) -> (SocketAddrV4, SocketAddrV4) {
            self.cb.endpoints()
        }

        pub fn set_local(&mut self, local: SocketAddrV4) {
            self.cb.set_local(local)
        }

        pub fn recv_queue_len(&self) -> usize {
            self.cb.recv_queue_len()
        }

        pub fn serialize(&self) -> Buffer {
            let mut buf = Buffer::Heap(DataBuffer::new(self.serialized_size()).unwrap());
            let remaining = self.cb.serialize_into(&mut buf);
            assert!(remaining.is_empty());
            buf
        }

        pub fn deserialize(mut buf: Buffer) -> Self {
            let cb = ControlBlockState::deserialize_from(&mut buf);
            Self { cb }
        }

        fn serialized_size(&self) -> usize {
            self.cb.serialized_size()
        }
    }

    #[cfg(test)]
    mod test {
        use std::net::{SocketAddrV4, Ipv4Addr};

        use crate::{
            runtime::memory::{Buffer, DataBuffer},
            inetstack::protocols::{
                tcpmig::segment::{TcpMigSegment, TcpMigHeader, TcpMigDefragmenter},
                ethernet2::Ethernet2Header, ipv4::Ipv4Header
            },
            capy_profile, capy_profile_dump, MacAddress
        };

        use super::TcpState;

        fn get_socket_addr() -> SocketAddrV4 {
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 10000)
        }

        fn get_buf() -> Buffer {
            Buffer::Heap(DataBuffer::from_slice(&vec![1; 64]))
        }

        fn get_state() -> TcpState {
            TcpState { cb: super::super::super::established::test_get_control_block_state() }
        }

        fn get_header() -> TcpMigHeader {
            TcpMigHeader::new(
                get_socket_addr(),
                get_socket_addr(),
                100,
                crate::inetstack::protocols::tcpmig::MigrationStage::ConnectionState,
                10000,
                10000,
            )
        }

        #[inline(always)]
        fn create_segment(payload: Buffer) -> TcpMigSegment {
            let len = payload.len();
            TcpMigSegment::new(
                Ethernet2Header::new(MacAddress::broadcast(), MacAddress::broadcast(), crate::inetstack::protocols::ethernet2::EtherType2::Ipv4),
                Ipv4Header::new(Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, crate::inetstack::protocols::ip::IpProtocol::UDP),
                get_header(),
                payload,
            )
        }

        #[test]
        fn measure_state_time() {
            std::env::set_var("CAPY_LOG", "all");
            crate::capylog::init();
            eprintln!();
            let state = get_state();

            let state = {
                // capy_profile!("serialise");
                state.serialize()
            };

            let segment = {
                // capy_profile!("segment creation");
                create_segment(state)
            };

            let seg_clone = segment.clone();
            let count = seg_clone.fragments().count();

            let mut fragments = Vec::with_capacity(count);
            {
                // capy_profile!("total fragment");
                for e in segment.fragments() {
                    fragments.push((e.tcpmig_hdr, e.data));
                }
            }
            
            let mut defragmenter = TcpMigDefragmenter::new();

            let mut i = 0;
            let mut segment = None;
            {
                // capy_profile!("total defragment");
                for (hdr, buf) in fragments {
                    i += 1;
                    if let Some(seg) = {
                        // capy_profile!("defragment");
                        defragmenter.defragment(hdr, buf)
                    } {
                        segment = Some(seg);
                    }
                }
            };
            assert_eq!(count, i);
            
            let (hdr, buf) = segment.unwrap();
            let state = {
                // capy_profile!("deserialise");
                TcpState::deserialize(buf)
            };
            assert_eq!(state, get_state());

            capy_profile_dump!(&mut std::io::stderr().lock());
            eprintln!();
        }
    }
}