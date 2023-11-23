// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use super::{
    active_open::ActiveOpenSocket,
    established::{
        EstablishedSocket,
        State, UnackedSegment,
    },
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
            established::{
                ControlBlock,
                Sender,
                Receiver,
                congestion_control::{
                    self,
                    CongestionControl,
                },
            },
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
        VecDeque,
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
    collections::HashSet,
};

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcp_migration::{TcpMigPeer, MigrationHandle};

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
    // local port -> FD
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
}

pub struct TcpPeer {
    pub(super) inner: Rc<RefCell<Inner>>,
}

#[cfg(feature = "tcp-migration")]
/// State needed for TCP Migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpState {
    pub local: SocketAddrV4, // 0..6
    pub remote: SocketAddrV4, // 6..12

    pub reader_next: SeqNumber, // 12..16
    pub receive_next: SeqNumber, // 16..20

    pub unsent_seq_no: SeqNumber, // 20..24
    pub send_unacked: SeqNumber, // 24..28
    pub send_next: SeqNumber, // 28..32
    pub send_window: u32, // 32..36
    pub send_window_last_update_seq: SeqNumber, // 36..40
    pub send_window_last_update_ack: SeqNumber, // 40..44
    pub mss: usize, // 44..48 (Assume < 4GB)

    pub receiver_window_size: u32, // 48..52
    pub receiver_window_scale: u32, // 52..56

    pub window_scale: u8, // 56..57
    out_of_order_fin: Option<SeqNumber>, // 57..58 - Option, 58..62 - number if Some

    // Queue: First 4 bytes - len (Assume < 4B elements), then elements.
    // Buffer: First 4 bytes - len (Assume < 4GB), then data.
    out_of_order_queue: VecDeque<(SeqNumber, Buffer)>,
    pub recv_queue: VecDeque<Buffer>,
    pub unacked_queue: VecDeque<UnackedSegment>,
    pub unsent_queue: VecDeque<Buffer>,

    user_data: Option<Buffer>, // First byte - Option, next data if present.
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
        tcpmig: TcpMigPeer,
    ) -> Result<Self, Fail> {
        let (tx, rx) = mpsc::unbounded();
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
            nonce,
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
        
        #[cfg(feature = "tcp-migration")]
        if inner.tcpmig.take_connection(local, remote) {
            capy_profile!("migrated_accept");
            
            #[cfg(not(feature = "mig-per-n-req"))]
            let recv_queue_len = cb.receiver.recv_queue_len();

            match inner.migrate_in_tcp_connection(new_qd, cb) {
                Ok(()) => {
                    #[cfg(not(feature = "mig-per-n-req"))]
                    inner.tcpmig.start_tracking_connection_stats(local, remote, recv_queue_len);

                    capy_log_mig!("MIG-CONNECTION ESTABLISHED (REMOTE: {:?})", remote);
            
                    /* activate this for recv_queue_len vs mig_lat eval */
                    /* let mig_key = (
                        SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), inner.tcpmig.get_port()),
                        SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 7), 201));
                    let qd = inner.qds.get(&mig_key).ok_or_else(|| Fail::new(EBADF, "socket not exist"))?;
                    if let Some(mig_socket) = inner.established.get(&mig_key) {
                        if mig_socket.cb.receiver.recv_queue_len() == 100 {
                            // The key exists in the hashmap, and mig_socket now contains the value.
                            // eprintln!("recv_queue_len to be mig: {}", mig_socket.cb.receiver.recv_queue_len());
                            inner.tcpmig.stop_tracking_connection_stats(mig_key.0, mig_key.1, mig_socket.cb.receiver.recv_queue_len());
                            inner.tcpmig.initiate_migration(mig_key, *qd);
                        }
                    } else {
                        // The key does not exist in the esablished hashmap, panic.
                        panic!("Key not found in established HashMap: {:?}", mig_key);
                    } */
                    /* activate this for recv_queue_len vs mig_lat eval */

                    return Poll::Ready(Ok(new_qd));
                },
                Err(e) => {
                    warn!("Dropped migrated-in connection");
                    return Poll::Ready(Err(e));
                }
            };
        };

        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let key: (SocketAddrV4, SocketAddrV4) = (local, remote);

        let socket: Socket = Socket::Established { local, remote };

        eprintln!("CONNECTION ESTABLISHED (REMOTE: {:?})", remote);

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.sockets.insert(new_qd, socket).is_some() {
            panic!("duplicate queue descriptor in sockets table");
        }
        inner.qds.insert((local, remote), new_qd);
        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.established.insert(key, established).is_some() {
            panic!("duplicate queue descriptor in established sockets table");
        }

        #[cfg(feature = "tcp-migration")]
        #[cfg(not(feature = "mig-per-n-req"))]
        inner.tcpmig.start_tracking_connection_stats(local, remote, 0);
        
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

        #[cfg(feature = "tcp-migration")]
        if inner.tcpmig.is_ready_to_migrate_out(fd) {
            return Poll::Ready(Err(Fail::new(crate::ETCPMIG, "socket ready to migrate out")));
        }

        capy_log!("\n\npolling POP on {:?}", key);
        match inner.established.get(&key) {
            Some(ref s) 
            => s.poll_recv(
                ctx,
                #[cfg(feature = "tcp-migration")]
                &inner.tcpmig,
            ),
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
        /* #[cfg(feature = "tcp-migration")]
        if self.inner.borrow().tcpmig.is_ready_to_migrate_out(fd) {
            return Err(Fail::new(super::super::tcp_migration::ETCPMIG, "connection ready to be migrated"))
        } */

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
            Some(ref s) => s.send(buf, #[cfg(feature = "tcp-migration")] &inner.tcpmig),
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
        }
    }

    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);

        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(EINVAL, "invalid address type"));
        }
        let key = (local, remote);

        capy_log!("\n\n[RX] {:?} => {:?}", remote, local);
        if let Some(s) = self.established.get(&key) {
            let is_data_empty = data.is_empty();

            debug!("Routing to established connection: {:?}", key);
            s.receive(
                &mut tcp_hdr,
                data,
                #[cfg(feature = "tcp-migration")]
                &self.tcpmig,
            );

            #[cfg(all(feature = "tcp-migration", not(feature = "mig-per-n-req")))]
            // Remove
            if tcp_hdr.fin || tcp_hdr.rst {
                capy_log!("RX FIN or RST => tcpmig stops tracking this conn");
                self.tcpmig.stop_tracking_connection_stats(local, remote, s.cb.receiver.recv_queue_len());
            }
            else if !is_data_empty {
                // println!("receive");
                // self.tcpmig.update_incoming_stats(local, remote, s.cb.receiver.recv_queue_len());
                // self.tcpmig.queue_length_heartbeat();
                
                /* activate this for recv_queue_len vs mig_lat eval */
                /* let mig_key = (
                    SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10000),
                    SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 7), 201));
                let qd = self.qds.get(&mig_key).ok_or_else(|| Fail::new(EBADF, "socket not exist"))?;
                if let Some(mig_socket) = self.established.get(&mig_key) {
                    if mig_socket.cb.receiver.recv_queue_len() == 100 {
                        // The key exists in the hashmap, and mig_socket now contains the value.
                        // eprintln!("recv_queue_len to be mig: {}", mig_socket.cb.receiver.recv_queue_len());
                        self.tcpmig.stop_tracking_connection_stats(mig_key.0, mig_key.1, mig_socket.cb.receiver.recv_queue_len());
                        self.tcpmig.initiate_migration(mig_key, *qd);
                    }
                } else {
                    // The key does not exist in the esablished hashmap, panic.
                    panic!("Key not found in established HashMap: {:?}", mig_key);
                } */
                /* activate this for recv_queue_len vs mig_lat eval */
                
                // Possible decision-making point.
                /* comment out this for recv_queue_len vs mig_lat eval */
                if let Some(conn) = self.tcpmig.should_migrate() {
                    // eprintln!("{:?}", conn);
                    capy_time_log!("INIT_MIG,({}-{})", conn.0, conn.1);
                    capy_log_mig!("should migrate");
                    {
                        capy_profile!("prepare");
                        // self.tcpmig.print_stats();
                        let qd = self.qds.get(&conn).ok_or_else(|| Fail::new(EBADF, "socket not exist"))?;
                        capy_log_mig!("qd: {:?}, conn: {:?}", qd, conn);
                        let mig_key = (conn.0, conn.1);
                        // Attempt to retrieve the value from the hashmap
                        if let Some(mig_socket) = self.established.get(&mig_key) {
                            // The key exists in the hashmap, and mig_socket now contains the value.
                            // eprintln!("recv_queue_len to be mig: {}", mig_socket.cb.receiver.recv_queue_len());
                            self.tcpmig.stop_tracking_connection_stats(conn.0, conn.1, mig_socket.cb.receiver.recv_queue_len());
                            self.tcpmig.initiate_migration(conn, *qd);
                        } else {
                            // The key does not exist in the esablished hashmap, panic.
                            panic!("Key not found in established HashMap: {:?}", mig_key);
                        }
                    }
                }
                /* comment out this for recv_queue_len vs mig_lat eval */
            }
            return Ok(());
        }
        if let Some(s) = self.connecting.get_mut(&key) {
            debug!("Routing to connecting connection: {:?}", key);
            s.receive(&tcp_hdr);
            return Ok(());
        }
        
        #[cfg(feature = "tcp-migration")]
        // Check if migrating queue exists. If yes, push buffer to queue and return, else continue normally.
        let (tcp_hdr, data) = match self.tcpmig.try_buffer_packet(local, remote, tcp_hdr, data) {
            Ok(()) => return Ok(()),
            Err(val) => val,
        };

        #[cfg(feature = "tcp-migration")]
        if self.tcpmig.is_migrated_out(remote) {
            capy_log!("Dropped packet received on migrated out connection ({local}, {remote})");
            return Ok(());
        }

        let (local, _) = key;
        if let Some(s) = self.passive.get_mut(&local) {
            debug!("Routing to passive connection: {:?}", local);
            return s.receive(ip_hdr, &tcp_hdr);
        }

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
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

//==============================================================================
// TCP Migration
//==============================================================================

#[cfg(feature = "tcp-migration")]
impl TcpPeer {
    pub fn try_get_migration_handle(&mut self, qd: QDesc) -> Result<Option<MigrationHandle>, Fail> {
        let inner = self.inner.borrow();
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

        Ok(inner.tcpmig.get_migration_handle(conn, qd))
    }

    pub fn do_migrate_out(&mut self, handle: MigrationHandle, data: Option<&[u8]>) -> Result<(), Fail> {
        let mut inner = self.inner.borrow_mut();
        let (conn, qd) = handle.inner();
        capy_log_mig!("\n\nMigrate Out ({}, {})", conn.0, conn.1);
        {
            capy_profile!("migrate");
            let mut state = inner.migrate_out_tcp_connection(qd)?;
            if let Some(data) = data {
                state.set_user_data(data);
            }

            inner.tcpmig.migrate_out(handle, state);
            Ok(())
        }
    }

    pub fn take_migrated_data(&mut self, qd: QDesc) -> Result<Option<Buffer>, Fail> {
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

        Ok(inner.tcpmig.take_incoming_user_data(&conn))
    }   
    
    #[cfg(feature = "mig-per-n-req")]
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

        capy_time_log!("INIT_MIG,({}-{})", conn.0, conn.1);
        inner.tcpmig.initiate_migration(conn, qd);
        Ok(())

        /* NON-CONCURRENT MIGRATION */
        /* if(conn.1.port() == 303){
            capy_time_log!("INIT_MIG,({}-{})", conn.0, conn.1); 
            inner.tcpmig.initiate_migration(conn, qd);
            Ok(())
        }else{
            return Err(Fail::new(EBADF, "this connection is not for migration"));
        } */
        /* NON-CONCURRENT MIGRATION */
    }

    pub fn get_migration_prepared_qds(&mut self) -> Result<HashSet<QDesc>, Fail> {
        self.inner.borrow_mut().tcpmig.get_migration_prepared_qds()
    }

    pub fn notify_passive(&mut self, state: TcpState) -> Result<(), Fail> {
        self.inner.borrow_mut().notify_passive(state)
    }

    pub fn migrate_out_tcp_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        self.inner.borrow_mut().migrate_out_tcp_connection(qd)
    }

    pub fn global_recv_queue_length(&mut self) -> usize {
        self.inner.borrow_mut().tcpmig.global_recv_queue_length()
    }
    pub fn print_queue_length(&mut self) {
        self.inner.borrow_mut().tcpmig.print_queue_length()
    }
    pub fn pushed_response(&mut self) {
        self.inner.borrow_mut().tcpmig.pushed_response()
    }
}

//==============================================================================
// TCP Migration
//==============================================================================

#[cfg(feature = "tcp-migration")]
impl Inner {
    fn take_tcp_state(&mut self, fd: QDesc) -> Result<TcpState, Fail> {
        info!("Retrieving TCP State for {:?}!", fd);

        match self.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => {
                let key = (*local, *remote);
                match self.established.get(&key) {
                    Some(connection) => {
                        let cb = connection.cb.clone();
                        let mss = cb.get_mss();
                        let (send_window, _) = cb.get_send_window();
                        let (send_unacked, _) = cb.get_send_unacked();
                        let (unsent_seq_no, _) = cb.get_unsent_seq_no();
                        let (send_next, _) = cb.get_send_next();
                        let receive_next = cb.receiver.receive_next.get();
                        let sender_window_scale = cb.get_sender_window_scale();
                        let reader_next = cb.receiver.reader_next.get();

                        Ok(TcpState::new(
                            *local,
                            *remote,

                            reader_next,
                            receive_next,
                            cb.take_receive_queue(),

                            unsent_seq_no,
                            send_unacked,
                            send_next,
                            send_window,
                            cb.sender.get_send_window_last_update_seq(),
                            cb.sender.get_send_window_last_update_ack(),
                            sender_window_scale,
                            mss,
                            cb.take_unacked_queue(),
                            cb.take_unsent_queue(),

                            cb.get_receiver_max_window_size(),
                            cb.get_receiver_window_scale(),

                            cb.take_out_of_order_queue(),
                            cb.out_of_order_fin.get(),
                        ))
                    },
                    None => {
                        Err(Fail::new(EINVAL, "We can only migrate out established connections."))
                    }
                }
            },
            _ => {
                Err(Fail::new(EINVAL, "We can only migrate out established connections."))
            }
        }
    }

    /// 1) remove this socket.
    /// 2) check if this connection is established one.
    /// 3) Remove socket from Established hashmap.
    fn migrate_out_tcp_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        let state = self.take_tcp_state(qd)?;
        
        let socket = self.sockets.get_mut(&qd);
        let (local, remote) = match socket {
            None => {
                debug!("No entry in `sockets` for fd: {:?}", qd);
                return Err(Fail::new(EBADF, "socket does not exist"));
            },
            Some(Socket::Established { local, remote }) => {
                (*local, *remote)
            },
            Some(s) => {
                panic!("Unsupported Socket variant: {:?} for migrating out.", s)
            },
        };

        // 1) remove this socket
        match self.sockets.remove(&qd) {
            Some(s) => {},
            None => return Err(Fail::new(EBADF, "sockeet not exist")),
        };

        // 2) check if this connection is established one
        let key = (local, remote);
        let mut entry = match self.established.entry(key) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(_) => {
                return Err(Fail::new(EINVAL, "socket not established"));
            }
        };

        let established = entry.get_mut();
        match established.cb.get_state() {
            State::Established => (),
            s => panic!("We only migrate out established connections. Found: {:?}", s),
        }

        // 3) Remove socket from Established hashmap.
        if let Some(socket) = self.established.remove(&key) {
            // 4) Remove the background for this connection
            // socket.abort();
        } else {
            // This panic is okay. This should never happen and represents an internal error
            // in our implementation.
            panic!("Established socket somehow missing.");
        }

        Ok(state)
    }

    fn notify_passive(&mut self, state: TcpState) -> Result<(), Fail> {
        let receiver = Receiver::migrated_in(
            state.reader_next,
            state.receive_next,
            state.recv_queue,
        );

        let sender = Sender::migrated_in(
            state.unsent_seq_no,
            state.send_unacked,
            state.send_next,
            state.send_window,
            state.send_window_last_update_seq,
            state.send_window_last_update_ack,
            state.window_scale,
            state.mss,
            state.unacked_queue,
            state.unsent_queue,
        );

        let cb = ControlBlock::migrated_in(
            state.local,
            state.remote,
            self.rt.clone(),
            self.scheduler.clone(),
            self.clock.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            self.tcp_config.get_ack_delay_timeout(),
            state.receiver_window_size,
            state.receiver_window_scale,
            state.out_of_order_queue,
            state.out_of_order_fin,
            sender,
            receiver,
            congestion_control::None::new,
            None,
        );

        match self.passive.get_mut(&state.local) {
            Some(passive) => passive.push_migrated_in(cb),
            None => return Err(Fail::new(libc::EBADF, "socket not listening")),
        };

        Ok(())
    }

    pub fn migrate_in_tcp_connection(&mut self, qd: QDesc, cb: ControlBlock) -> Result<(), Fail> {
        let local = cb.get_local();
        let remote = cb.get_remote();
        // Check if keys already exist first. This way we don't have to undo changes we make to
        // the state.
        if self.established.contains_key(&(local, remote)) {
            debug!("Key already exists in established hashmap.");
            // TODO: Not sure if there is a better error to use here.
            return Err(Fail::new(EBUSY, "This connection already exists."))
        }

        for (mut tcp_hdr, data) in self.tcpmig.take_buffer_queue(local, remote)? {
            /* let tcp_hdr_size = tcp_hdr.compute_size();
            let mut buf = vec![0u8; tcp_hdr_size + data.len()];
            tcp_hdr.serialize(&mut buf, &ip_hdr, &data, inner.tcp_config.get_rx_checksum_offload());

            // Find better way than cloning data.
            buf[tcp_hdr_size..].copy_from_slice(&data);

            let buf = Buffer::Heap(crate::runtime::memory::DataBuffer::from_slice(&buf)); */
            capy_log_mig!("take_buffer_queue");
            cb.receive(&mut tcp_hdr, data, &self.tcpmig);
        }

        // Connection should either not exist or have been migrated out (and now we are migrating
        // it back in).
        match self.sockets.entry(qd) {
            Entry::Occupied(mut e) => {
                // match e.get_mut() {
                //     e@Socket::MigratedOut { .. } => {
                //         *e = Socket::Established { local, remote };
                //     },
                //     _ => {
                panic!("Key already exists in sockets hashmap.");
                            // return Err(Fail::new(EBADF, "bad file descriptor"));
                //     }
                // }
                // inho: fn do_accept always allocate a new qd, so the qd of a connection migrated in 
                // cannot exist in sockets 
            },
            Entry::Vacant(v) => {
                let socket = Socket::Established { local, remote };
                v.insert(socket);
                self.qds.insert((local, remote), qd);
            }
        }

        let established = EstablishedSocket::new(cb, qd, self.dead_socket_tx.clone());

        if let Some(_) = self.established.insert((local, remote), established) {
            // This condition should have been checked for at the beginning of this function.
            unreachable!();
        }

        self.tcpmig.complete_migrating_in(local, remote);
        capy_log_mig!("\n\n!!! Accepted ({}, {}) !!!\n\n", local, remote);
        Ok(())
    }
}

#[cfg(feature = "tcp-migration")]
impl TcpState {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,

        reader_next: SeqNumber,
        receive_next: SeqNumber,
        recv_queue: VecDeque<Buffer>,

        unsent_seq_no: SeqNumber,
        send_unacked: SeqNumber,
        send_next: SeqNumber,
        send_window: u32,
        send_window_last_update_seq: SeqNumber,
        send_window_last_update_ack: SeqNumber,
        window_scale: u8,
        mss: usize,
        unacked_queue: VecDeque<UnackedSegment>,
        unsent_queue: VecDeque<Buffer>,

        receiver_window_size: u32,
        receiver_window_scale: u32,

        out_of_order_queue: VecDeque<(SeqNumber, Buffer)>,
        out_of_order_fin: Option<SeqNumber>,
    ) -> Self {
        Self {
            local,
            remote,

            reader_next,
            receive_next,
            recv_queue,

            unsent_seq_no,
            send_unacked,
            send_next,
            send_window,
            send_window_last_update_seq,
            send_window_last_update_ack,
            window_scale,
            mss,
            unacked_queue,
            unsent_queue,

            receiver_window_size,
            receiver_window_scale,

            out_of_order_queue,
            out_of_order_fin,

            user_data: None,
        }
    }

    fn set_user_data(&mut self, data: &[u8]) {
        self.user_data = Some(Buffer::Heap(data.into()));
    }

    pub fn take_user_data(&mut self) -> Option<Buffer> {
        std::mem::take(&mut self.user_data)
    }

    pub fn serialize(&self) -> Buffer {
        let mut buf = Buffer::Heap(crate::runtime::memory::DataBuffer::new(self.serialized_size()).unwrap());
        assert!(self.serialize_into(&mut buf).is_empty());
        buf
    }

    fn serialized_size(&self) -> usize {
        57 // fixed
        + 1 + if self.out_of_order_fin.is_some() { 4 } else { 0 } // out_of_order_fin
        + 4 + self.out_of_order_queue.iter().fold(0, |acc, (_seq, buf)| acc + 4 + 4 + buf.len()) // out_of_order_queue
        + 4 + self.recv_queue.iter().fold(0, |acc, e| acc + 4 + e.len()) // recv_queue
        + 4 + self.unacked_queue.iter().fold(0, |acc, e| acc + 4 + e.bytes.len()) // unacked_queue
        + 4 + self.unsent_queue.iter().fold(0, |acc, e| acc + 4 + e.len()) // unsent_queue
        + 1 + if let Some(ref data) = self.user_data { 4 + data.len() } else { 0 }
    }

    pub fn deserialize(mut buf: Buffer) -> Self {
        use byteorder::{ByteOrder, BigEndian};

        let buf = &mut buf;

        let local = SocketAddrV4::new(Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]), BigEndian::read_u16(&buf[4..6]));
        let remote = SocketAddrV4::new(Ipv4Addr::new(buf[6], buf[7], buf[8], buf[9]), BigEndian::read_u16(&buf[10..12]));

        let reader_next = SeqNumber::from(BigEndian::read_u32(&buf[12..16]));
        let receive_next = SeqNumber::from(BigEndian::read_u32(&buf[16..20]));
        
        let unsent_seq_no = SeqNumber::from(BigEndian::read_u32(&buf[20..24]));
        let send_unacked = SeqNumber::from(BigEndian::read_u32(&buf[24..28]));
        let send_next = SeqNumber::from(BigEndian::read_u32(&buf[28..32]));
        let send_window = BigEndian::read_u32(&buf[32..36]);
        let send_window_last_update_seq = SeqNumber::from(BigEndian::read_u32(&buf[36..40]));
        let send_window_last_update_ack = SeqNumber::from(BigEndian::read_u32(&buf[40..44]));
        let mss = usize::try_from(BigEndian::read_u32(&buf[44..48])).unwrap();
        
        let receiver_window_size = BigEndian::read_u32(&buf[48..52]);
        let receiver_window_scale = BigEndian::read_u32(&buf[52..56]);
        let window_scale = buf[56];

        let out_of_order_fin = match buf[57] {
            0 => None,
            1 => {
                let val = Some(SeqNumber::from(BigEndian::read_u32(&buf[58..62])));
                buf.adjust(4); // Adjust for the 4 additional bytes of out_of_order_fin.
                val
            },
            _ => panic!("invalid Option discriminant"),
        };
        buf.adjust(58); // Adjust for the fixed size.

        let out_of_order_queue = VecDeque::<(SeqNumber, Buffer)>::deserialize_from(buf);
        let recv_queue = VecDeque::<Buffer>::deserialize_from(buf);
        let unacked_queue = VecDeque::<UnackedSegment>::deserialize_from(buf);
        let unsent_queue = VecDeque::<Buffer>::deserialize_from(buf);

        let user_data = match buf[0] {
            0 => None,
            1 => {
                buf.adjust(1);
                Some(Buffer::deserialize_from(buf))
            },
            _ => panic!("invalid Option discriminant"),
        };

        TcpState {
            local,
            remote,
            reader_next,
            receive_next,
            unsent_seq_no,
            send_unacked,
            send_next,
            send_window,
            send_window_last_update_seq,
            send_window_last_update_ack,
            mss,
            receiver_window_size,
            receiver_window_scale,
            window_scale,
            out_of_order_fin,
            out_of_order_queue,
            recv_queue,
            unacked_queue,
            unsent_queue,
            user_data
        }
    }
}

//==============================================================================
// TCP State Serialization
//==============================================================================

#[cfg(feature = "tcp-migration")]
trait Serialize {
    /// Serializes into the buffer and returns its unused part.
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8];
}

#[cfg(feature = "tcp-migration")]
trait Deserialize: Sized {
    /// Deserializes and removes the deserialised part from the buffer.
    fn deserialize_from(buf: &mut Buffer) -> Self;
}

#[cfg(feature = "tcp-migration")]
impl Serialize for TcpState {
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        buf[0..4].copy_from_slice(&self.local.ip().octets());
        buf[4..6].copy_from_slice(&self.local.port().to_be_bytes());
        buf[6..10].copy_from_slice(&self.remote.ip().octets());
        buf[10..12].copy_from_slice(&self.remote.port().to_be_bytes());

        buf[12..16].copy_from_slice(&u32::from(self.reader_next).to_be_bytes());
        buf[16..20].copy_from_slice(&u32::from(self.receive_next).to_be_bytes());
        
        buf[20..24].copy_from_slice(&u32::from(self.unsent_seq_no).to_be_bytes());
        buf[24..28].copy_from_slice(&u32::from(self.send_unacked).to_be_bytes());
        buf[28..32].copy_from_slice(&u32::from(self.send_next).to_be_bytes());
        buf[32..36].copy_from_slice(&self.send_window.to_be_bytes());
        buf[36..40].copy_from_slice(&u32::from(self.send_window_last_update_seq).to_be_bytes());
        buf[40..44].copy_from_slice(&u32::from(self.send_window_last_update_ack).to_be_bytes());
        buf[44..48].copy_from_slice(&u32::try_from(self.mss).expect("mss too big").to_be_bytes());
        
        buf[48..52].copy_from_slice(&self.receiver_window_size.to_be_bytes());
        buf[52..56].copy_from_slice(&self.receiver_window_scale.to_be_bytes());

        buf[56] = self.window_scale;

        // out_of_order_fin
        let buf = if let Some(seq) = self.out_of_order_fin {
            buf[57] = 1;
            buf[58..62].copy_from_slice(&u32::from(seq).to_be_bytes());
            &mut buf[62..]
        } else {
            buf[57] = 0;
            &mut buf[58..]
        };

        let buf = self.out_of_order_queue.serialize_into(buf);
        let buf = self.recv_queue.serialize_into(buf);
        let buf = self.unacked_queue.serialize_into(buf);
        let buf = self.unsent_queue.serialize_into(buf);

        let buf = if let Some(ref data) = self.user_data {
            buf[0] = 1;
            data.serialize_into(&mut buf[1..])
        } else {
            buf[0] = 0;
            &mut buf[1..]
        };

        buf
    }
}

#[cfg(feature = "tcp-migration")]
impl<T: Serialize> Serialize for VecDeque<T> {
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        buf[0..4].copy_from_slice(&u32::try_from(self.len()).expect("deque len too big").to_be_bytes());
        let mut buf = &mut buf[4..];
        for e in self {
            buf = e.serialize_into(buf);
        }
        buf
    }
}

#[cfg(feature = "tcp-migration")]
impl Serialize for Buffer {
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        buf[0..4].copy_from_slice(&u32::try_from(self.len()).expect("buffer len too big").to_be_bytes());
        buf[4..4 + self.len()].copy_from_slice(&self);
        &mut buf[4 + self.len()..]
    }
}

#[cfg(feature = "tcp-migration")]
impl Serialize for UnackedSegment {
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        self.bytes.serialize_into(buf)
    }
}

#[cfg(feature = "tcp-migration")]
impl Serialize for (SeqNumber, Buffer) {
    fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        buf[0..4].copy_from_slice(&u32::from(self.0).to_be_bytes());
        self.1.serialize_into(&mut buf[4..])
    }
}

#[cfg(feature = "tcp-migration")]
impl<T: Deserialize> Deserialize for VecDeque<T> {
    fn deserialize_from(buf: &mut Buffer) -> Self {
        use byteorder::{ByteOrder, BigEndian};

        let size = usize::try_from(BigEndian::read_u32(&buf[0..4])).unwrap();
        let mut deque = VecDeque::<T>::with_capacity(size);
        buf.adjust(4);

        for _ in 0..size {
            let deserialized = T::deserialize_from(buf);
            deque.push_back(deserialized);
        }
        deque
    }
}

#[cfg(feature = "tcp-migration")]
impl Deserialize for Buffer {
    fn deserialize_from(buf: &mut Buffer) -> Self {
        use byteorder::{ByteOrder, BigEndian};

        let len = usize::try_from(BigEndian::read_u32(&buf[0..4])).unwrap();
        buf.adjust(4);

        let mut buffer = buf.clone();
        buffer.trim(buf.len() - len);
        buf.adjust(len);

        buffer
    }
}

#[cfg(feature = "tcp-migration")]
impl Deserialize for UnackedSegment {
    fn deserialize_from(buf: &mut Buffer) -> Self {
        let bytes = Buffer::deserialize_from(buf);
        Self { bytes, initial_tx: None }
    }
}

#[cfg(feature = "tcp-migration")]
impl Deserialize for (SeqNumber, Buffer) {
    fn deserialize_from(buf: &mut Buffer) -> Self {
        use byteorder::{ByteOrder, BigEndian};

        let seq = SeqNumber::from(BigEndian::read_u32(&buf[0..4]));
        buf.adjust(4);
        let buffer = Buffer::deserialize_from(buf);
        (seq, buffer)
    }
}

#[cfg(feature = "tcp-migration")]
#[cfg(test)]
mod test {
    use std::{net::{SocketAddrV4, Ipv4Addr}, collections::VecDeque};

    use crate::{
        runtime::memory::{Buffer, DataBuffer},
        inetstack::protocols::{
            tcp::established::UnackedSegment,
            tcp_migration::segment::{TcpMigSegment, TcpMigHeader, TcpMigDefragmenter},
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
        TcpState::new(
            get_socket_addr(),            
            get_socket_addr(),
            10.into(),
            10.into(),
            VecDeque::from(vec![get_buf(); 100]),
            10.into(),
            10.into(),
            10.into(),
            10,
            10.into(),
            10.into(),
            10,
            10,
            VecDeque::from(vec![UnackedSegment { bytes: get_buf(), initial_tx: None }; 100]),
            VecDeque::from(vec![get_buf(); 100]),
            10,
            10,
            VecDeque::from(vec![(7.into(), get_buf()); 10]),
            None,
        )
    }

    fn get_header() -> TcpMigHeader {
        TcpMigHeader::new(
            get_socket_addr(),
            get_socket_addr(),
            100,
            crate::inetstack::protocols::tcp_migration::MigrationStage::ConnectionState,
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
            capy_profile!("serialise");
            state.serialize()
        };

        let segment = {
            capy_profile!("segment creation");
            create_segment(state)
        };

        let seg_clone = segment.clone();
        let count = seg_clone.fragments().count();

        let mut fragments = Vec::with_capacity(count);
        {
            capy_profile!("total fragment");
            for e in segment.fragments() {
                fragments.push((e.tcpmig_hdr, e.data));
            }
        }
        
        let mut defragmenter = TcpMigDefragmenter::new();

        let mut i = 0;
        let mut segment = None;
        {
            capy_profile!("total defragment");
            for (hdr, buf) in fragments {
                i += 1;
                if let Some(seg) = {
                    capy_profile!("defragment");
                    defragmenter.defragment(hdr, buf)
                } {
                    segment = Some(seg);
                }
            }
        };
        assert_eq!(count, i);
        
        let (hdr, buf) = segment.unwrap();
        let state = {
            capy_profile!("deserialise");
            TcpState::deserialize(buf)
        };
        assert_eq!(state, get_state());

        capy_profile_dump!(&mut std::io::stderr().lock());
        eprintln!();
    }
}

/*
 * Optimisations:
 * - Implement deserialisation on fragments to prevent defragmentation.
 * - Use hashmap crate for faster lookups.
 */
