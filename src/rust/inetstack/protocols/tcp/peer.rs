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
    
};

#[cfg(feature = "tcp-migration-profiler")]
use crate::tcpmig_profile;

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
};

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcp_migration::TcpMigPeer;

#[cfg(feature = "profiler")]
use crate::timer;


#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::{tcp_log, tcpmig_log};

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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TcpState {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,

    pub reader_next: SeqNumber,
    pub receive_next: SeqNumber,

    pub recv_queue: VecDeque<Buffer>,

    pub unsent_seq_no: SeqNumber,
    pub send_unacked: SeqNumber,
    pub send_next: SeqNumber,
    pub send_window: u32,
    pub send_window_last_update_seq: SeqNumber,
    pub send_window_last_update_ack: SeqNumber,
    pub window_scale: u8,
    pub mss: usize,
    pub unacked_queue: VecDeque<UnackedSegment>,
    pub unsent_queue: VecDeque<Buffer>,

    pub receiver_window_size: u32,
    pub receiver_window_scale: u32,

    out_of_order_queue: VecDeque<(SeqNumber, Buffer)>,
    out_of_order_fin: Option<SeqNumber>,
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

        #[cfg(feature = "tcp-migration")]
        if inner.tcpmig.take_connection(cb.get_local(), cb.get_remote()) {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("migrated_accept");
            
            match inner.migrate_in_tcp_connection(new_qd, cb) {
                Ok(()) => {
                    // eprintln!("*** Accepted migrated connection ***");
                    return Poll::Ready(Ok(new_qd));
                },
                Err(e) => {
                    warn!("Dropped migrated-in connection");
                    return Poll::Ready(Err(e));
                }
            };
        };

        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let key: (SocketAddrV4, SocketAddrV4) = (established.cb.get_local(), established.cb.get_remote());

        let socket: Socket = Socket::Established {
            local: established.cb.get_local(),
            remote: established.cb.get_remote(),
        };
        println!("CONNECTION ESTABLISHED (REMOTE: {:?})", established.cb.get_remote());
        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.sockets.insert(new_qd, socket).is_some() {
            panic!("duplicate queue descriptor in sockets table");
        }

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        if inner.established.insert(key, established).is_some() {
            panic!("duplicate queue descriptor in established sockets table");
        }

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
            Some(Socket::MigratedOut { .. }) => return Poll::Ready(Err(Fail::new(EBADF, "socket migrated out"))),
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("\n\npolling POP on {:?}", key));
        }
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

    pub fn pop(&self, fd: QDesc) -> PopFuture {
        PopFuture {
            fd,
            inner: self.inner.clone(),
        }
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

        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        _dead_socket_rx: mpsc::UnboundedReceiver<QDesc>,
    ) -> Self {
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);
        let nonce: u32 = rng.gen();

        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("Creating new TcpPeer::Inner"));
        }
        Self {
            isn_generator: IsnGenerator::new(nonce),
            ephemeral_ports,
            sockets: HashMap::new(),
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
        let cloned_buf = buf.clone();

        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(EINVAL, "invalid address type"));
        }
        let key = (local, remote);

        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("\n\n[RX] {:?} => {:?}", remote, local));
        }
        if let Some(s) = self.established.get(&key) {
            debug!("Routing to established connection: {:?}", key);
            #[cfg(feature = "tcp-migration")]
            {
                // Remove
                if tcp_hdr.fin || tcp_hdr.rst {
                    #[cfg(feature = "capybara-log")]
                    {
                        tcp_log(format!("RX FIN or RST => tcpmig stops tracking this conn"));
                    }
                    self.tcpmig.stop_tracking_connection_stats(local, remote);
                }
                else {
                    // println!("receive");
                    self.tcpmig.update_incoming_stats(local, remote, s.cb.receiver.recv_queue_len());
                    // self.tcpmig.queue_length_heartbeat();
                }
            }

            s.receive(&mut tcp_hdr, data);
            return Ok(());
        }
        if let Some(s) = self.connecting.get_mut(&key) {
            debug!("Routing to connecting connection: {:?}", key);
            s.receive(&tcp_hdr);
            return Ok(());
        }
        
        #[cfg(feature = "tcp-migration")]
        // Check if migrating queue exists. If yes, push buffer to queue.
        if let Ok(()) = self.tcpmig.try_buffer_packet(local, remote, ip_hdr.clone(), tcp_hdr.clone(), cloned_buf) {
            return Ok(());
        }

        #[cfg(feature = "tcp-migration")]
        if self.tcpmig.is_migrated_out(remote) {
            warn!("Dropped packet received on migrated out connection ({local}, {remote})");
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
    pub fn notify_migration_safety(&mut self, qd: QDesc) -> Result<bool, Fail> {
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

        if inner.tcpmig.should_migrate(conn) {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("prepare");

            inner.tcpmig.initiate_migration(conn, qd);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn notify_passive(&mut self, state: TcpState) -> Result<(), Fail> {
        self.inner.borrow_mut().notify_passive(state)
    }

    pub fn migrate_out_tcp_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        self.inner.borrow_mut().migrate_out_tcp_connection(qd)
    }
}

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

    /// 1) Change status of our socket to MigratedOut.
    /// 2) Change status of ControlBlock state to Migrated out.
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

        // 1) Change status of our socket to MigratedOut
        *socket.unwrap() = Socket::MigratedOut { local, remote };

        // 2) Change status of ControlBlock state to Migrated out.
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
            socket.abort();
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

        // Connection should either not exist or have been migrated out (and now we are migrating
        // it back in).
        match self.sockets.entry(qd) {
            Entry::Occupied(mut e) => {
                match e.get_mut() {
                    e@Socket::MigratedOut { .. } => {
                        *e = Socket::Established { local, remote };
                    },
                    _ => {
                        debug!("Key already exists in sockets hashmap.");
                        return Err(Fail::new(EBADF, "bad file descriptor"));
                    }
                }
            },
            Entry::Vacant(v) => {
                let socket = Socket::Established { local, remote };
                v.insert(socket);
            }
        }

        let established = EstablishedSocket::new(cb, qd, self.dead_socket_tx.clone());

        if let Some(_) = self.established.insert((local, remote), established) {
            // This condition should have been checked for at the beginning of this function.
            unreachable!();
        }

        for (ip_hdr, _, buf) in self.tcpmig.take_buffer_queue(local, remote)? {
            /* let tcp_hdr_size = tcp_hdr.compute_size();
            let mut buf = vec![0u8; tcp_hdr_size + data.len()];
            tcp_hdr.serialize(&mut buf, &ip_hdr, &data, inner.tcp_config.get_rx_checksum_offload());

            // Find better way than cloning data.
            buf[tcp_hdr_size..].copy_from_slice(&data);

            let buf = Buffer::Heap(crate::runtime::memory::DataBuffer::from_slice(&buf)); */
            #[cfg(feature = "capybara-log")]
            {
                tcpmig_log(format!("take_buffer_queue"));
            }
            self.receive(&ip_hdr, buf)?;
        }
        self.tcpmig.complete_migrating_in(local, remote);
        #[cfg(feature = "capybara-log")]
        {
            tcpmig_log(format!("\n\n!!! Accepted ({}, {}) !!!\n\n", local, remote));
        }
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
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    pub fn deserialize(serialized: &[u8]) -> Result<Self, postcard::Error> {
        // TODO: Check if having all `UnackedSegment` timestamps as `None` affects anything.
        postcard::from_bytes(serialized)
    }
}