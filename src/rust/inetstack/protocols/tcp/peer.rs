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
    queue::TcpQueue,
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
        queue::InetQueue,
        tcp::{
            established::ControlBlock,
            operations::{
                AcceptFuture,
                CloseFuture,
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
        memory::DemiBuffer,
        network::{
            config::TcpConfig,
            types::MacAddress,
            NetworkRuntime,
        },
        queue::IoQueueTable,
        timer::TimerRc,
        QDesc,
    },
    scheduler::scheduler::Scheduler,
};
use ::futures::channel::mpsc;
use ::rand::{
    prelude::SmallRng,
    Rng,
    SeedableRng,
};

use ::std::{
    cell::{
        Ref,
        RefCell,
        RefMut,
    },
    collections::HashMap,
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

#[cfg(feature = "profiler")]
use crate::timer;

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcpmig::TcpMigPeer;
#[cfg(feature = "tcp-migration")]
use super::established::ControlBlockState;
#[cfg(feature = "tcp-migration-profiler")]
use crate::{
    tcpmig_profiler::{tcpmig_log, tcp_log},
    tcpmig_profile
};

//==============================================================================
// Enumerations
//==============================================================================

pub enum Socket<const N: usize> {
    Inactive(Option<SocketAddrV4>),
    Listening(PassiveSocket<N>),
    Connecting(ActiveOpenSocket<N>),
    Established(EstablishedSocket<N>),
    Closing(EstablishedSocket<N>),
    
    #[cfg(feature = "tcp-migration")]
    MigratedOut,
}

#[derive(PartialEq, Eq, Hash)]
enum SocketId {
    Active(SocketAddrV4, SocketAddrV4),
    Passive(SocketAddrV4),
}

//==============================================================================
// Structures
//==============================================================================

pub struct Inner<const N: usize> {
    isn_generator: IsnGenerator,
    ephemeral_ports: EphemeralPorts,
    // queue descriptor -> per queue metadata
    qtable: Rc<RefCell<IoQueueTable<InetQueue<N>>>>,
    // Connection or socket identifier for mapping incoming packets to the Demikernel queue
    addresses: HashMap<SocketId, QDesc>,
    rt: Rc<dyn NetworkRuntime<N>>,
    scheduler: Scheduler,
    clock: TimerRc,
    local_link_addr: MacAddress,
    local_ipv4_addr: Ipv4Addr,
    tcp_config: TcpConfig,
    arp: ArpPeer<N>,
    rng: Rc<RefCell<SmallRng>>,
    dead_socket_tx: mpsc::UnboundedSender<QDesc>,

    #[cfg(feature = "tcp-migration")]
    tcpmig: TcpMigPeer<N>,
}

pub struct TcpPeer<const N: usize> {
    pub(super) inner: Rc<RefCell<Inner<N>>>,
}

#[cfg(feature = "tcp-migration")]
/// State needed for TCP Migration.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TcpState {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    cb_state: ControlBlockState,
}

//==============================================================================
// Associated Functions
//==============================================================================

impl<const N: usize> TcpPeer<N> {
    pub fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
        scheduler: Scheduler,
        qtable: Rc<RefCell<IoQueueTable<InetQueue<N>>>>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer<N>,
        rng_seed: [u8; 32],

        #[cfg(feature = "tcp-migration")]
        tcpmig: TcpMigPeer<N>,
    ) -> Result<Self, Fail> {
        let (tx, rx) = mpsc::unbounded();
        let inner = Rc::new(RefCell::new(Inner::new(
            rt.clone(),
            scheduler,
            qtable.clone(),
            clock,
            local_link_addr,
            local_ipv4_addr,
            tcp_config,
            arp,
            rng_seed,
            tx,
            rx,

            #[cfg(feature = "tcp-migration")]
            tcpmig,
        )));
        Ok(Self { inner })
    }

    /// Opens a TCP socket.
    pub fn do_socket(&self) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("tcp::socket");
        let inner: Ref<Inner<N>> = self.inner.borrow();
        let mut qtable: RefMut<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow_mut();
        let new_qd: QDesc = qtable.alloc(InetQueue::Tcp(TcpQueue::new()));
        Ok(new_qd)
    }

    pub fn bind(&self, qd: QDesc, mut addr: SocketAddrV4) -> Result<(), Fail> {
        let mut inner: RefMut<Inner<N>> = self.inner.borrow_mut();

        // Check if address is already bound.
        for (socket_id, _) in &inner.addresses {
            match socket_id {
                SocketId::Passive(local) | SocketId::Active(local, _) if *local == addr => {
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
        let ret: Result<(), Fail> = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Inactive(None) => {
                    queue.set_socket(Socket::Inactive(Some(addr)));
                    Ok(())
                },
                Socket::Inactive(_) => Err(Fail::new(libc::EINVAL, "socket is already bound to an address")),
                Socket::Listening(_) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EINVAL, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EINVAL, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
                #[cfg(feature = "tcp-migration")]
                Socket::MigratedOut => return Err(Fail::new(libc::EINVAL, "socket is migrated out")),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        };

        // Handle return value.
        match ret {
            Ok(x) => {
                inner.addresses.insert(SocketId::Passive(addr), qd);
                Ok(x)
            },
            Err(e) => {
                // Rollback ephemeral port allocation.
                if EphemeralPorts::is_private(addr.port()) {
                    if inner.ephemeral_ports.free(addr.port()).is_err() {
                        warn!("bind(): leaking ephemeral port (port={})", addr.port());
                    }
                }
                Err(e)
            },
        }
    }

    pub fn receive(&mut self, ip_header: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        self.inner.borrow_mut().receive(ip_header, buf)
    }

    // Marks the target socket as passive.
    pub fn listen(&self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        // This code borrows a reference to inner, instead of the entire self structure,
        // so we can still borrow self later.
        let mut inner_: RefMut<Inner<N>> = self.inner.borrow_mut();
        let inner: &mut Inner<N> = &mut *inner_;
        let mut qtable: RefMut<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow_mut();
        // Get bound address while checking for several issues.
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Inactive(Some(local)) => {
                    // Check if there isn't a socket listening on this address/port pair.
                    if inner.addresses.contains_key(&SocketId::Passive(*local)) {
                        if *inner.addresses.get(&SocketId::Passive(*local)).unwrap() != qd {
                            return Err(Fail::new(
                                libc::EADDRINUSE,
                                "another socket is already listening on the same address/port pair",
                            ));
                        }
                    }

                    let nonce: u32 = inner.rng.borrow_mut().gen();
                    let socket = PassiveSocket::new(
                        *local,
                        backlog,
                        inner.rt.clone(),
                        inner.scheduler.clone(),
                        inner.clock.clone(),
                        inner.tcp_config.clone(),
                        inner.local_link_addr,
                        inner.arp.clone(),
                        nonce,
                    );
                    inner.addresses.insert(SocketId::Passive(local.clone()), qd);
                    queue.set_socket(Socket::Listening(socket));
                    Ok(())
                },
                Socket::Inactive(None) => {
                    return Err(Fail::new(libc::EDESTADDRREQ, "socket is not bound to a local address"))
                },
                Socket::Listening(_) => return Err(Fail::new(libc::EINVAL, "socket is already listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EINVAL, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EINVAL, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
                #[cfg(feature = "tcp-migration")]
                Socket::MigratedOut => return Err(Fail::new(libc::EINVAL, "socket is migrated out")),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts an incoming connection.
    pub fn do_accept(&self, qd: QDesc) -> (QDesc, AcceptFuture<N>) {
        let mut inner_: RefMut<Inner<N>> = self.inner.borrow_mut();
        let inner: &mut Inner<N> = &mut *inner_;

        let new_qd: QDesc = inner.qtable.borrow_mut().alloc(InetQueue::Tcp(TcpQueue::new()));
        (new_qd, AcceptFuture::new(qd, new_qd, self.inner.clone()))
    }

    /// Handles an incoming connection.
    pub fn poll_accept(
        &self,
        qd: QDesc,
        new_qd: QDesc,
        ctx: &mut Context,
    ) -> Poll<Result<(QDesc, SocketAddrV4), Fail>> {
        let mut inner: RefMut<Inner<N>> = self.inner.borrow_mut();

        let cb: ControlBlock<N> = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Listening(socket) => match socket.poll_accept(ctx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(result) => match result {
                        Ok(cb) => cb,
                        Err(err) => {
                            inner.qtable.borrow_mut().free(&new_qd);
                            return Poll::Ready(Err(err));
                        },
                    },
                },
                _ => return Poll::Ready(Err(Fail::new(libc::EOPNOTSUPP, "socket not listening"))),
            },
            _ => return Poll::Ready(Err(Fail::new(libc::EBADF, "invalid queue descriptor"))),
        };

        let local: SocketAddrV4 = cb.get_local();
        let remote: SocketAddrV4 = cb.get_remote();

        let established: EstablishedSocket<N> = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        match inner.qtable.borrow_mut().get_mut(&new_qd) {
            Some(InetQueue::Tcp(queue)) => queue.set_socket(Socket::Established(established)),
            _ => panic!("Should have been pre-allocated!"),
        };
        if inner
            .addresses
            .insert(SocketId::Active(local, remote), new_qd)
            .is_some()
        {
            panic!("duplicate queue descriptor in established sockets table");
        }

        #[cfg(feature = "tcp-migration")]
        if inner.tcpmig.take_connection(local, remote) {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("migrated_accept");
            
            if let Err(e) = inner.migrate_in_tcp_connection(local, remote) {
                warn!("Dropped migrated-in connection");
                inner.qtable.borrow_mut().free(&new_qd);
                inner.addresses.remove(&SocketId::Active(local, remote));
                return Poll::Ready(Err(e));
            };
        };

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        Poll::Ready(Ok((new_qd, remote)))
    }

    pub fn connect(&self, qd: QDesc, remote: SocketAddrV4) -> Result<ConnectFuture<N>, Fail> {
        let mut inner_: RefMut<Inner<N>> = self.inner.borrow_mut();
        let inner: &mut Inner<N> = &mut *inner_;
        let mut qtable: RefMut<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow_mut();

        // Get local address bound to socket.
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Inactive(local_socket) => {
                    let local: SocketAddrV4 = match local_socket {
                        Some(local) => local.clone(),
                        None => {
                            // TODO: we should free this when closing.
                            let local_port: u16 = inner.ephemeral_ports.alloc_any()?;
                            SocketAddrV4::new(inner.local_ipv4_addr, local_port)
                        },
                    };

                    // Create active socket.
                    let local_isn: SeqNumber = inner.isn_generator.generate(&local, &remote);
                    let socket: ActiveOpenSocket<N> = ActiveOpenSocket::new(
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

                    // Update socket state.
                    queue.set_socket(Socket::Connecting(socket));
                    inner.addresses.insert(SocketId::Active(local, remote.clone()), qd)
                },
                Socket::Listening(_) => return Err(Fail::new(libc::EOPNOTSUPP, "socket is listening")),
                Socket::Connecting(_) => return Err(Fail::new(libc::EALREADY, "socket is connecting")),
                Socket::Established(_) => return Err(Fail::new(libc::EISCONN, "socket is connected")),
                Socket::Closing(_) => return Err(Fail::new(libc::EINVAL, "socket is closed")),
                #[cfg(feature = "tcp-migration")]
                Socket::MigratedOut => return Err(Fail::new(libc::EINVAL, "socket is migrated out")),
            },
            _ => return Err(Fail::new(libc::EBADF, "invalid queue descriptor"))?,
        };
        Ok(ConnectFuture {
            qd: qd,
            inner: self.inner.clone(),
        })
    }

    pub fn poll_recv(&self, qd: QDesc, ctx: &mut Context, size: Option<usize>) -> Poll<Result<DemiBuffer, Fail>> {
        let inner: Ref<Inner<N>> = self.inner.borrow();
        let mut qtable: RefMut<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(ref mut queue)) => match queue.get_mut_socket() {
                Socket::Established(ref mut socket) => socket.poll_recv(ctx, size),
                Socket::Closing(ref mut socket) => socket.poll_recv(ctx, size),
                Socket::Connecting(_) => Poll::Ready(Err(Fail::new(libc::EINPROGRESS, "socket connecting"))),
                Socket::Inactive(_) => Poll::Ready(Err(Fail::new(libc::EBADF, "socket inactive"))),
                Socket::Listening(_) => Poll::Ready(Err(Fail::new(libc::ENOTCONN, "socket listening"))),
                #[cfg(feature = "tcp-migration")]
                Socket::MigratedOut => Poll::Ready(Err(Fail::new(libc::EBADF, "socket migrated out"))),
            },
            _ => Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        }
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn push(&self, qd: QDesc, buf: DemiBuffer) -> PushFuture {
        let err: Option<Fail> = match self.send(qd, buf) {
            Ok(()) => None,
            Err(e) => Some(e),
        };
        PushFuture { qd, err }
    }

    /// TODO: Should probably check for valid queue descriptor before we schedule the future
    pub fn pop(&self, qd: QDesc, size: Option<usize>) -> PopFuture<N> {
        PopFuture {
            qd,
            size,
            inner: self.inner.clone(),
        }
    }

    fn send(&self, qd: QDesc, buf: DemiBuffer) -> Result<(), Fail> {
        let inner = self.inner.borrow();
        let qtable = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(ref queue)) => match queue.get_socket() {
                Socket::Established(ref socket) => socket.send(buf),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    /// Closes a TCP socket.
    pub fn do_close(&self, qd: QDesc) -> Result<(), Fail> {
        let mut inner: RefMut<Inner<N>> = self.inner.borrow_mut();
        // TODO: Currently we do not handle close correctly because we continue to receive packets at this point to finish the TCP close protocol.
        // 1. We do not remove the endpoint from the addresses table
        // 2. We do not remove the queue from the queue table.
        // As a result, we have stale closed queues that are labelled as closing. We should clean these up.
        // look up socket
        let (addr, result): (SocketAddrV4, Result<(), Fail>) = match inner.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Established(socket) => {
                        socket.close()?;
                        queue.set_socket(Socket::Closing(socket.clone()));
                        return Ok(());
                    },
                    // Closing an unbound socket.
                    Socket::Inactive(None) => {
                        return Ok(());
                    },
                    // Closing a bound socket.
                    Socket::Inactive(Some(addr)) => (addr.clone(), Ok(())),
                    // Closing a listening socket.
                    Socket::Listening(socket) => {
                        let cause: String = format!("cannot close a listening socket (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        (socket.endpoint(), Err(Fail::new(libc::ENOTSUP, &cause)))
                    },
                    // Closing a connecting socket.
                    Socket::Connecting(_) => {
                        let cause: String = format!("cannot close a connecting socket (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },
                    // Closing a closing socket.
                    Socket::Closing(_) => {
                        let cause: String = format!("cannot close a socket that is closing (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },

                    #[cfg(feature = "tcp-migration")]
                    Socket::MigratedOut => {
                        let cause: String = format!("cannot close a socket that is migrated out (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        // TODO: remove active sockets from the addresses table.
        inner.addresses.remove(&SocketId::Passive(addr));
        result
    }

    /// Closes a TCP socket.
    pub fn do_async_close(&self, qd: QDesc) -> Result<CloseFuture<N>, Fail> {
        match self.inner.borrow().qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Established(socket) => {
                        // Send FIN
                        socket.close()?;
                        // Move socket to closing state
                        queue.set_socket(Socket::Closing(socket.clone()));
                    },
                    // Closing an unbound socket.
                    Socket::Inactive(_) => (),
                    // Closing a listening socket.
                    Socket::Listening(_) => {
                        // TODO: Remove this address from the addresses table
                        let cause: String = format!("cannot close a listening socket (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },
                    // Closing a connecting socket.
                    Socket::Connecting(_) => {
                        let cause: String = format!("cannot close a connecting socket (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },
                    // Closing a closing socket.
                    Socket::Closing(_) => {
                        let cause: String = format!("cannot close a socket that is closing (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },

                    #[cfg(feature = "tcp-migration")]
                    Socket::MigratedOut => {
                        let cause: String = format!("cannot close a socket that is migrated out (qd={:?})", qd);
                        error!("do_close(): {}", &cause);
                        return Err(Fail::new(libc::ENOTSUP, &cause));
                    },
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        // Schedule a co-routine to all of the cleanup
        Ok(CloseFuture {
            qd,
            inner: self.inner.clone(),
        })
    }

    pub fn remote_mss(&self, qd: QDesc) -> Result<usize, Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.remote_mss()),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    pub fn current_rto(&self, qd: QDesc) -> Result<Duration, Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.current_rto()),
                _ => return Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    pub fn endpoints(&self, qd: QDesc) -> Result<(SocketAddrV4, SocketAddrV4), Fail> {
        let inner = self.inner.borrow();
        let qtable: Ref<IoQueueTable<InetQueue<N>>> = inner.qtable.borrow();
        match qtable.get(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_socket() {
                Socket::Established(socket) => Ok(socket.endpoints()),
                _ => Err(Fail::new(libc::ENOTCONN, "connection not established")),
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }
}

impl<const N: usize> Inner<N> {
    fn new(
        rt: Rc<dyn NetworkRuntime<N>>,
        scheduler: Scheduler,
        qtable: Rc<RefCell<IoQueueTable<InetQueue<N>>>>,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        tcp_config: TcpConfig,
        arp: ArpPeer<N>,
        rng_seed: [u8; 32],
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        _dead_socket_rx: mpsc::UnboundedReceiver<QDesc>,

        #[cfg(feature = "tcp-migration")]
        tcpmig: TcpMigPeer<N>,
    ) -> Self {
        let mut rng: SmallRng = SmallRng::from_seed(rng_seed);
        let ephemeral_ports: EphemeralPorts = EphemeralPorts::new(&mut rng);
        let nonce: u32 = rng.gen();
        Self {
            isn_generator: IsnGenerator::new(nonce),
            ephemeral_ports,
            rt,
            scheduler,
            qtable: qtable.clone(),
            addresses: HashMap::<SocketId, QDesc>::new(),
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

    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: DemiBuffer) -> Result<(), Fail> {
        #[cfg(feature = "tcp-migration")]
        let cloned_buf = buf.clone();

        let (mut tcp_hdr, data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(libc::EINVAL, "invalid address type"));
        }

        #[cfg(feature = "tcp-migration")]
        // Check if migrating queue exists. If yes, push buffer to queue.
        if let Ok(()) = self.tcpmig.try_buffer_packet(local, remote, ip_hdr.clone(), tcp_hdr.clone(), cloned_buf) {
            return Ok(());
        }

        // grab the queue descriptor based on the incoming.
        let &qd: &QDesc = match self.addresses.get(&SocketId::Active(local, remote)) {
            Some(qdesc) => qdesc,
            None => match self.addresses.get(&SocketId::Passive(local)) {
                Some(qdesc) => qdesc,
                None => return Err(Fail::new(libc::EBADF, "Socket not bound")),
            },
        };
        // look up the queue metadata based on queue descriptor.
        let mut qtable = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Established(socket) => {
                    debug!("Routing to established connection: {:?}", socket.endpoints());

                    #[cfg(feature = "tcp-migration")]
                    {
                        // Stop tracking connection if FIN or RST received.
                        if tcp_hdr.fin || tcp_hdr.rst {
                            #[cfg(feature = "capybara-log")]
                            {
                                tcp_log(format!("RX FIN or RST => tcpmig stops tracking this conn"));
                            }
                            self.tcpmig.stop_tracking_connection_stats(local, remote);
                        }
                        else {
                            // println!("receive");
                            self.tcpmig.update_incoming_stats(local, remote, socket.cb.recv_queue_len());
                            // self.tcpmig.queue_length_heartbeat();

                            // Possible decision-making point.
                            if self.tcpmig.should_migrate() {
                                #[cfg(feature = "tcp-migration-profiler")]
                                tcpmig_profile!("prepare");
                                // eprintln!("*** Should Migrate ***");
                                // self.tcpmig.print_stats();
                                self.tcpmig.initiate_migration();
                            }
                        }
                    }

                    socket.receive(&mut tcp_hdr, data);
                    return Ok(());
                },
                Socket::Connecting(socket) => {
                    debug!("Routing to connecting connection: {:?}", socket.endpoints());
                    socket.receive(&tcp_hdr);
                    return Ok(());
                },
                Socket::Listening(socket) => {
                    debug!("Routing to passive connection: {:?}", local);
                    return socket.receive(ip_hdr, &tcp_hdr);
                },
                Socket::Inactive(_) => (),
                Socket::Closing(socket) => {
                    debug!("Routing to closing connection: {:?}", socket.endpoints());
                    socket.receive(&mut tcp_hdr, data);
                    return Ok(());
                },
                #[cfg(feature = "tcp-migration")]
                Socket::MigratedOut => {
                    warn!("Dropped packet, received on migrated out connection ({local}, {remote})");
                    return Ok(());
                },
            },
            _ => panic!("No queue descriptor"),
        };

        // The packet isn't for an open port; send a RST segment.
        debug!("Sending RST for {:?}, {:?}", local, remote);
        self.send_rst(&local, &remote)?;
        Ok(())
    }

    fn send_rst(&self, local: &SocketAddrV4, remote: &SocketAddrV4) -> Result<(), Fail> {
        // TODO: Make this work pending on ARP resolution if needed.
        let remote_link_addr = self
            .arp
            .try_query(remote.ip().clone())
            .ok_or(Fail::new(libc::EINVAL, "detination not in ARP cache"))?;

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

    pub(super) fn poll_connect_finished(&mut self, qd: QDesc, context: &mut Context) -> Poll<Result<(), Fail>> {
        let mut qtable: RefMut<IoQueueTable<InetQueue<N>>> = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => match queue.get_mut_socket() {
                Socket::Connecting(socket) => {
                    let result: Result<ControlBlock<N>, Fail> = match socket.poll_result(context) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(r) => r,
                    };
                    match result {
                        Ok(cb) => {
                            let new_socket =
                                Socket::Established(EstablishedSocket::new(cb, qd, self.dead_socket_tx.clone()));
                            queue.set_socket(new_socket);
                            Poll::Ready(Ok(()))
                        },
                        Err(fail) => Poll::Ready(Err(fail)),
                    }
                },
                _ => Poll::Ready(Err(Fail::new(libc::EAGAIN, "socket not connecting"))),
            },
            _ => Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        }
    }

    // TODO: Eventually use context to store the waker for this function in the established socket.
    pub(super) fn poll_close_finished(&mut self, qd: QDesc, _context: &mut Context) -> Poll<Result<(), Fail>> {
        let sockid: Option<SocketId> = match self.qtable.borrow_mut().get_mut(&qd) {
            Some(InetQueue::Tcp(queue)) => {
                match queue.get_socket() {
                    // Closing an active socket.
                    Socket::Closing(socket) => match socket.poll_close() {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(_) => Some(SocketId::Active(socket.endpoints().0, socket.endpoints().1)),
                    },
                    // Closing an unbound socket.
                    Socket::Inactive(None) => None,
                    // Closing a bound socket.
                    Socket::Inactive(Some(addr)) => Some(SocketId::Passive(addr.clone())),
                    // Closing a listening socket.
                    Socket::Listening(_) => unimplemented!("Do not support async close for listening sockets yet"),
                    // Closing a connecting socket.
                    Socket::Connecting(_) => unimplemented!("Do not support async close for listening sockets yet"),
                    // Closing a closing socket.
                    Socket::Established(_) => unreachable!("Should have moved this socket to closing already!"),
                    #[cfg(feature = "tcp-migration")]
                    // Closing a migrated out socket.
                    Socket::MigratedOut => unimplemented!("Do not support async close for migrated out sockets yet"),
                }
            },
            _ => return Poll::Ready(Err(Fail::new(libc::EBADF, "bad queue descriptor"))),
        };

        // Remove queue from qtable
        self.qtable.borrow_mut().free(&qd);
        // Remove address from addresses backmap
        if let Some(addr) = sockid {
            self.addresses.remove(&addr);
        }
        Poll::Ready(Ok(()))
    }
}

//==============================================================================
// TCP Migration
//==============================================================================

#[cfg(feature = "tcp-migration")]
impl<const N: usize> TcpPeer<N> {
    pub fn notify_migration_safety(&mut self, qd: QDesc) -> Result<bool, Fail> {
        let mut inner = self.inner.borrow_mut();
        let (local, remote) = match inner.qtable.borrow().get(&qd) {
            Some(InetQueue::Tcp(socket)) => {
                match socket.get_socket() {
                    Socket::Established(socket) => socket.endpoints(),
                    _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        
        if let Some(handle) = inner.tcpmig.can_migrate_out(local, remote) {
            #[cfg(feature = "tcp-migration-profiler")]
            tcpmig_profile!("migrate");

            // eprintln!("*** Can migrate out ***");
            #[cfg(feature = "capybara-log")]
            {
                tcpmig_log(format!("\n\nMigrate Out ({}, {})", local, remote));
            }
            let state = inner.migrate_out_tcp_connection(qd)?;
            inner.tcpmig.migrate_out(handle, state);
            return Ok(true)
        }

        Ok(false)
    }

    pub fn notify_passive(&mut self, state: TcpState) -> Result<(), Fail> {
        self.inner.borrow_mut().notify_passive(state)
    }
}

#[cfg(feature = "tcp-migration")]
impl<const N: usize> Inner<N> {
    fn take_tcp_state(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        info!("Retrieving TCP State for QDesc {:?}!", qd);

        match self.qtable.borrow().get(&qd) {
            Some(InetQueue::Tcp(socket)) => {
                match socket.get_socket() {
                    Socket::Established(socket) => {
                        let (local, remote) = socket.endpoints();
                        let cb_state = socket.cb.to_state();
                        Ok(TcpState::new(local, remote, cb_state))
                    },
                    _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
                }
            },
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    pub fn migrate_out_tcp_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        let state = self.take_tcp_state(qd)?;
        
        let mut qtable = self.qtable.borrow_mut();
        let socket = match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(socket)) => socket.get_mut_socket(),
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };
        
        let (local, remote) = match socket {
            Socket::Established(socket) => socket.endpoints(),
            _ => return Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        };

        // Change status of our socket to MigratedOut
        *socket = Socket::MigratedOut;

        // Remove connection from `addresses`.
        self.addresses.remove(&SocketId::Active(local, remote)).expect("Connection not in addresses.");

        Ok(state)
    }

    fn notify_passive(&mut self, state: TcpState) -> Result<(), Fail> {
        let cb = ControlBlock::from_state(
            state.cb_state,
            self.rt.clone(),
            self.scheduler.clone(),
            self.clock.clone(),
            self.local_link_addr,
            self.tcp_config.clone(),
            self.arp.clone(),
            self.tcp_config.get_ack_delay_timeout(),
        );

        let qd = match self.addresses.get(&SocketId::Passive(state.local)) {
            Some(qd) => *qd,
            _ => return Err(Fail::new(libc::EBADF, "not a listening socket")),
        };

        let mut qtable = self.qtable.borrow_mut();
        match qtable.get_mut(&qd) {
            Some(InetQueue::Tcp(socket)) => {
                match socket.get_mut_socket() {
                    Socket::Listening(socket) => socket.push_migrated_in(cb),
                    _ => return Err(Fail::new(libc::EBADF, "not a listening socket")),
                }
            },
            _ => return Err(Fail::new(libc::EBADF, "not a listening socket")),
        }

        Ok(())
    }

    pub fn migrate_in_tcp_connection(&mut self, local: SocketAddrV4, remote: SocketAddrV4) -> Result<(), Fail> {
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
    fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        cb_state: ControlBlockState,
    ) -> Self {
        Self {
            local,
            remote,
            cb_state,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    pub fn deserialize(serialized: &[u8]) -> Result<Self, postcard::Error> {
        // TODO: Check if having all `UnackedSegment` timestamps as `None` affects anything.
        postcard::from_bytes(serialized)
    }

    // For logging
    pub fn recv_queue_len(&self) -> usize {
        self.cb_state.recv_queue_len()
    }
}