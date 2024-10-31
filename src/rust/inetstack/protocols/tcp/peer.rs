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
    capy_time_log,
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
use num_traits::ToBytes;

use ::std::{
    cell::{
        RefCell,
        RefMut,
    },
    collections::{
        hash_map::Entry,
        HashMap,
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

use ::byteorder::{
    ByteOrder,
    NetworkEndian,
};

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::{
    tcp::stats::Stats,
    tcpmig::{
        ApplicationState,
        TcpMigPeer,
        TcpmigPollState,
    },
};
#[cfg(feature = "tcp-migration")]
use state::TcpState;

#[cfg(feature = "profiler")]
use crate::timer;

use crate::{
    capy_log,
    capy_log_mig,
    capy_profile,
    capy_profile_total,
};

//==============================================================================
// Enumerations
//==============================================================================

#[derive(Debug)]
pub enum Socket {
    Inactive {
        local: Option<SocketAddrV4>,
    },
    Listening {
        local: SocketAddrV4,
    },
    Connecting {
        local: SocketAddrV4,
        remote: SocketAddrV4,
    },
    Established {
        local: SocketAddrV4,
        remote: SocketAddrV4,
    },

    #[cfg(feature = "tcp-migration")]
    MigratedOut {
        local: SocketAddrV4,
        remote: SocketAddrV4,
    },
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
    recv_queue_stats: Stats,
    #[cfg(feature = "tcp-migration")]
    rps_stats: Stats,
    #[cfg(feature = "tcp-migration")]
    tcpmig_poll_state: Rc<TcpmigPollState>,
    #[cfg(feature = "tcp-migration")]
    min_threshold: usize,
    #[cfg(feature = "tcp-migration")]
    rps_threshold: f64,
    #[cfg(feature = "tcp-migration")]
    threshold_epsilon: f64,
    #[cfg(feature = "tcp-migration")]
    reactive_migration_enabled: bool,
    #[cfg(feature = "tcp-migration")]
    last_rps_signal: Option<(usize, usize)>,

    #[cfg(feature = "server-reply-analysis")]
    listening_port: u16,

    #[cfg(feature = "capybara-switch")]
    backend_servers: arrayvec::ArrayVec<SocketAddrV4, 4>,

    #[cfg(feature = "capybara-switch")]
    migration_directory: HashMap<SocketAddrV4, SocketAddrV4>,
    #[cfg(feature = "capybara-switch")]
    num_backends: usize,

    #[cfg(feature = "tcp-migration")]
    user_connection: Option<UserConnection>,
}

pub struct TcpPeer {
    pub(super) inner: Rc<RefCell<Inner>>,
}

pub trait UserConnectionContext {
    // the pair of _state update_ methods
    // the trait is designed to work with `Rc<dyn _>` so the methods take `&self`, but they are
    // expected to generate side effect and not reentrant

    // be called inside `set_user_connection_context(..)` call, indicating the accepting connection
    // is an incoming migration
    // to ensure this method is called before any IO on the connection,
    // `set_user_connection_context(..)` should be called immediately after accepting
    fn migrate_in(&self, data: Buffer);
    // be called when the connection has been migrated out, and no more data will be received from
    // the connection (no more should be written to the connection as well)
    // it is *not* called inside `initiate_migration_*(..)` call
    fn migrate_out(&self);

    // the following methods are expected to be pure (although they are probably only called once
    // per migration anyway)
    fn buf_size(&self) -> usize;
    fn write(&self, buf: &mut [u8]);
}

#[cfg(feature = "tcp-migration")]
struct UserConnection {
    // interfaces for capybara to user defined connections
    // inserted when calling `set_user_connection_context(..)` on accepted QDesc
    // removed when calling `initiate_migration_*(..)` interfaces or when the connection is closed
    // it is application's responsibility to call `set_user_connection_context(..)` on every
    // accepted connections (as long as `enable_user_connection()` is called), or the migration
    // behavior will be undefined
    contexts: HashMap<QDesc, Rc<dyn UserConnectionContext>>,
    // incoming migration user data, temporarily hold before application calls
    // `set_user_connection_context(..)`
    pending_migrate_in: HashMap<SocketAddrV4, Buffer>,
}

pub fn enable_user_connection(libos: &crate::LibOS) {
    match libos {
        crate::LibOS::NetworkLibOS(libos) => match libos {
            crate::demikernel::libos::network::NetworkLibOS::Catnip(libos) => {
                libos.ipv4.tcp.inner.borrow_mut().enable_user_connection()
            },
            _ => unimplemented!(),
        },
    }
}

pub fn set_user_connection_context(libos: &crate::LibOS, qd: QDesc, context: Rc<dyn UserConnectionContext>) {
    match libos {
        crate::LibOS::NetworkLibOS(libos) => match libos {
            crate::demikernel::libos::network::NetworkLibOS::Catnip(libos) => libos
                .ipv4
                .tcp
                .inner
                .borrow_mut()
                .set_user_connection_context(qd, context),
            _ => unimplemented!(),
        },
    }
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

        #[cfg(feature = "tcp-migration")] tcpmig_poll_state: Rc<TcpmigPollState>,
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

        #[cfg(feature = "server-reply-analysis")]
        {
            inner.listening_port = addr.port();
        }

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

    pub fn receive(
        &self,
        ip_header: &mut Ipv4Header,
        buf: Buffer,
        #[cfg(feature = "capybara-switch")] eth_hdr: Ethernet2Header,
    ) -> Result<(), Fail> {
        self.inner.borrow_mut().receive(
            ip_header,
            buf,
            #[cfg(feature = "capybara-switch")]
            eth_hdr,
        )
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

        // Enable stats tracking.
        #[cfg(feature = "tcp-migration")]
        {
            // Process buffered packets.
            if let Some(buffered) = inner.tcpmig.close_active_migration(remote) {
                capy_time_log!("CONN_ACCEPTED,({})", remote);
                for (mut hdr, data) in buffered {
                    capy_log_mig!("start receiving target-buffered packets into the CB");
                    cb.receive(&mut hdr, data);
                }
            }

            cb.enable_stats(&mut inner.recv_queue_stats, &mut inner.rps_stats);
        }

        let established: EstablishedSocket = EstablishedSocket::new(cb, new_qd, inner.dead_socket_tx.clone());
        let key: (SocketAddrV4, SocketAddrV4) = (local, remote);

        let socket: Socket = Socket::Established { local, remote };

        capy_log!("CONNECTION ESTABLISHED (REMOTE: {:?}, new_qd: {:?})", remote, new_qd);

        // TODO: Reset the connection if the following following check fails, instead of panicking.
        match inner.sockets.insert(new_qd, socket) {
            None => (),
            #[cfg(feature = "tcp-migration")]
            Some(Socket::MigratedOut { .. }) => {
                capy_log_mig!("migrated socket QD overwritten");
            },
            _ => panic!("duplicate queue descriptor in sockets table"),
        }

        assert!(
            inner.qds.insert((local, remote), new_qd).is_none(),
            "duplicate entry in qds table"
        );

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
            Some(Socket::MigratedOut { .. }) => {
                return Poll::Ready(Err(Fail::new(crate::ETCPMIG, "socket migrated out")))
            },
            None => return Poll::Ready(Err(Fail::new(EBADF, "bad queue descriptor"))),
        };

        capy_log!("\n\npolling POP on {:?} ({:?})", fd, key);
        match inner.established.get(&key) {
            Some(ref s) => s.poll_recv(ctx),
            None => Poll::Ready(Err(Fail::new(ENOTCONN, "connection not established"))),
        }
    }

    pub fn push(&self, fd: QDesc, buf: Buffer) -> PushFuture {
        capy_log!("[1] TCP pushing");
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
        capy_log!("send to fd: {:?}", fd);
        let key = match inner.sockets.get(&fd) {
            Some(Socket::Established { local, remote }) => (*local, *remote),
            #[cfg(feature = "tcp-migration")]
            Some(Socket::MigratedOut { .. }) => return Err(Fail::new(crate::ETCPMIG, "socket migrated out")),
            Some(..) => {
                eprintln!("connection not established");
                return Err(Fail::new(ENOTCONN, "connection not established"));
            },
            None => {
                eprintln!("bad queue descriptor");
                return Err(Fail::new(EBADF, "bad queue descriptor"));
            },
        };
        let send_result = match inner.established.get(&key) {
            Some(ref s) => s.send(buf),
            None => Err(Fail::new(ENOTCONN, "connection not established")),
        };

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

        #[cfg(feature = "tcp-migration")] tcpmig: TcpMigPeer,
        #[cfg(feature = "tcp-migration")] tcpmig_poll_state: Rc<TcpmigPollState>,

        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
        _dead_socket_rx: mpsc::UnboundedReceiver<QDesc>,
    ) -> Self {
        #[cfg(feature = "capybara-switch")]
        let backend_servers = arrayvec::ArrayVec::from([
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10000),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10001),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10002),
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 9), 10003),
        ]);

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
            recv_queue_stats: Stats::new(),
            #[cfg(feature = "tcp-migration")]
            rps_stats: Stats::new(),
            #[cfg(feature = "tcp-migration")]
            tcpmig_poll_state,
            #[cfg(feature = "tcp-migration")]
            min_threshold: std::env::var("MIN_THRESHOLD")
                .unwrap_or_else(|_| String::from("100"))
                .parse::<usize>()
                .expect("Invalid MIN_TOTAL_LOAD_FOR_MIG value"),
            #[cfg(feature = "tcp-migration")]
            rps_threshold: std::env::var("RPS_THRESHOLD")
                .unwrap_or_else(|_| String::from("0.55"))
                .parse::<f64>()
                .expect("Invalid RPS_THRESHOLD value"),
            #[cfg(feature = "tcp-migration")]
            threshold_epsilon: std::env::var("THRESHOLD_EPSILON")
                .unwrap_or_else(|_| String::from("0.05"))
                .parse::<f64>()
                .expect("Invalid THRESHOLD_EPSILON value"),
            #[cfg(feature = "tcp-migration")]
            reactive_migration_enabled: false,
            #[cfg(feature = "tcp-migration")]
            last_rps_signal: None,

            #[cfg(feature = "server-reply-analysis")]
            listening_port: 0,
            #[cfg(feature = "capybara-switch")]
            backend_servers: backend_servers,
            #[cfg(feature = "capybara-switch")]
            migration_directory: HashMap::new(),
            #[cfg(feature = "capybara-switch")]
            num_backends: std::env::var("NUM_BACKENDS")
                .unwrap_or_else(|_| String::from("1"))
                .parse::<usize>()
                .expect("Invalid NUM_BACKENDS value"),

            #[cfg(feature = "tcp-migration")]
            user_connection: None,
        }
    }

    #[cfg(feature = "tcp-migration")]
    pub fn enable_user_connection(&mut self) {
        let replaced = self.user_connection.replace(UserConnection {
            contexts: Default::default(),
            pending_migrate_in: Default::default(),
        });
        assert!(replaced.is_none())
    }

    #[cfg(not(feature = "capybara-switch"))]
    fn receive(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        capy_log!("\n\n[RX]");

        let (mut tcp_hdr, mut data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        debug!("TCP received {:?}", tcp_hdr);
        let local = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let remote = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);
        capy_log!("{:?} => {:?}", remote, local);
        capy_log!(
            "SERVER RECEIVE TCP seq_num: {}, data length: {}",
            tcp_hdr.seq_num,
            data.len()
        );

        if remote.ip().is_broadcast() || remote.ip().is_multicast() || remote.ip().is_unspecified() {
            return Err(Fail::new(EINVAL, "invalid address type"));
        }
        let key = (local, remote);

        if let Some(s) = self.established.get(&key) {
            /* activate this for recv_queue_len vs mig_lat eval */
            // let is_data_empty = data.is_empty();
            /* activate this for recv_queue_len vs mig_lat eval */

            #[cfg(feature = "server-reply-analysis")]
            {
                if data.len() >= 16 {
                    let data: &mut [u8] = &mut data;
                    data[0..2].copy_from_slice(&self.listening_port.to_be_bytes());

                    let connection_count: u16 = self.established.len().try_into().expect("connection count > u16::MAX");
                    data[2..4].copy_from_slice(&connection_count.to_be_bytes());

                    #[cfg(feature = "tcp-migration")]
                    let queue_len: u32 = self
                        .recv_queue_stats
                        .global_stat()
                        .try_into()
                        .expect("queue length is over 4 billion");
                    #[cfg(not(feature = "tcp-migration"))]
                    let queue_len: u32 = 0;
                    data[4..8].copy_from_slice(&queue_len.to_be_bytes());

                    let timestamp: u64 = chrono::Local::now()
                        .timestamp_nanos()
                        .try_into()
                        .expect("timestamp is negative");
                    data[8..16].copy_from_slice(&timestamp.to_be_bytes());
                }
            }

            debug!("Routing to established connection: {:?}", key);
            s.receive(&mut tcp_hdr, data);

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

            // #[cfg(feature = "tcp-migration")]
            // if self.tcpmig.should_migrate() {
            //     self.initiate_migration_by_addr((local, remote));
            // }

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

    #[cfg(feature = "capybara-switch")]
    fn receive(
        &mut self,
        ip_hdr: &mut Ipv4Header,
        buf: Buffer,
        #[cfg(feature = "capybara-switch")] mut eth_hdr: Ethernet2Header,
    ) -> Result<(), Fail> {
        #[cfg(feature = "capy-log")]
        {
            static mut COUNTER: usize = 0;
            unsafe {
                COUNTER += 1;
                if COUNTER % 100000 == 0 {
                    capy_time_log!("{}", COUNTER);
                }
            }
        }

        capy_log!("\n\nCAPYBARA_SWITCH [RX]");

        let (mut tcp_hdr, mut data) = TcpHeader::parse(ip_hdr, buf, self.tcp_config.get_rx_checksum_offload())?;
        let dst_addr = SocketAddrV4::new(ip_hdr.get_dest_addr(), tcp_hdr.dst_port);
        let src_addr = SocketAddrV4::new(ip_hdr.get_src_addr(), tcp_hdr.src_port);
        capy_log!("{:?} => {:?}", src_addr, dst_addr);
        capy_log!(
            "SERVER RECEIVE TCP seq_num: {}, data length: {}",
            tcp_hdr.seq_num,
            data.len()
        );
        if tcp_hdr.syn && !tcp_hdr.ack {
            static mut BE_IDX: usize = 0;
            capy_log!("SYN");

            let be_sockaddr = self.backend_servers[unsafe { BE_IDX }];
            let be_mac = match self.arp.try_query(*be_sockaddr.ip()) {
                Some(mac) => mac,
                None => panic!("MAC of this BE is not cached"),
            };

            // capy_log!("Element at index {}: {:?}", unsafe{BE_IDX}, self.backend_servers[unsafe{BE_IDX}]);

            // capy_log!("ETH: {:?}\n\nIP: {:?}\n\nTCP: {:?}\n", eth_hdr, ip_hdr, tcp_hdr);

            eth_hdr.set_dst_addr(be_mac);
            ip_hdr.set_dst_addr(*be_sockaddr.ip());
            tcp_hdr.set_dst_port(be_sockaddr.port());
            // capy_log!("ETH: {:?}\n\nIP: {:?}\n\nTCP: {:?}\n", eth_hdr, ip_hdr, tcp_hdr);

            if self.migration_directory.insert(src_addr, be_sockaddr).is_some() {
                panic!("duplicate queue descriptor in established sockets table");
            }

            let segment = TcpSegment {
                ethernet2_hdr: eth_hdr,
                ipv4_hdr: *ip_hdr,
                tcp_hdr,
                data: Some(data),
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            };
            self.rt.transmit(Box::new(segment));

            unsafe {
                BE_IDX = (BE_IDX + 1) % self.num_backends;
            }
        } else {
            capy_profile_total!("capybara-switch");
            let src_matching_addr = self.migration_directory.get(&src_addr);
            let dst_matching_addr = self.migration_directory.get(&dst_addr);
            // capy_log!("{:?}, {:?}", src_matching_addr, dst_matching_addr);
            match (src_matching_addr, dst_matching_addr) {
                (Some(target_sockaddr), None) => {
                    let target_mac = match self.arp.try_query(*target_sockaddr.ip()) {
                        Some(mac) => mac,
                        None => panic!("MAC of this BE is not cached"),
                    };
                    eth_hdr.set_dst_addr(target_mac);
                    ip_hdr.set_dst_addr(*target_sockaddr.ip());
                    tcp_hdr.set_dst_port(target_sockaddr.port());
                },
                (None, Some(_)) => {
                    eth_hdr.set_src_addr(self.local_link_addr);
                    ip_hdr.set_src_addr(self.local_ipv4_addr);
                    tcp_hdr.set_src_port(10000);
                    // capy_log!("ETH: {:?}\n\nIP: {:?}\n\nTCP: {:?}\n", eth_hdr, ip_hdr, tcp_hdr);
                },
                (Some(_), Some(_)) => panic!("Both src and dst are matching in migration directory"),
                (None, None) => panic!("No matching entry in migration_directory"),
            }

            let segment = TcpSegment {
                ethernet2_hdr: eth_hdr,
                ipv4_hdr: *ip_hdr,
                tcp_hdr,
                data: Some(data),
                tx_checksum_offload: self.tcp_config.get_rx_checksum_offload(),
            };
            self.rt.transmit(Box::new(segment));
        }

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

    pub fn receive_rps_signal(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        let sum = NetworkEndian::read_u32(&buf[12..16]) as usize;
        let individual = NetworkEndian::read_u32(&buf[16..20]) as usize;
        capy_time_log!("RPS_SIGNAL,{},{}", sum, individual);

        self.inner.borrow_mut().last_rps_signal = Some((sum, individual));
        Ok(())
    }

    pub fn rps_signal_action(&mut self) {
        {
            let mut inner = self.inner.borrow_mut();
            let (sum, individual) = match inner.last_rps_signal.take() {
                Some(val) => val,
                None => return,
            };

            let threshold = (sum as f64 * inner.rps_threshold) as usize;
            let threshold_epsilon = (sum as f64 * (inner.rps_threshold + inner.threshold_epsilon)) as usize;

            #[cfg(not(feature = "manual-tcp-migration"))]
            if sum > inner.min_threshold && individual > threshold_epsilon {
                inner.rps_stats.set_threshold(threshold);
                inner.reactive_migration_enabled = true;
                if let Some(conns_to_migrate) = inner.rps_stats.connections_to_proactively_migrate() {
                    drop(inner);
                    for conn in conns_to_migrate {
                        self.initiate_migration_by_addr(conn);
                    }
                }
            } else {
                inner.reactive_migration_enabled = false;
            }
        }
        self.inner.borrow_mut().rps_stats.reset_stats();
    }

    pub fn migrate_out_and_send(&mut self, qd: QDesc) {
        let mut inner = self.inner.borrow_mut();
        let state = inner.migrate_out_connection(qd).unwrap();
        inner.tcpmig.send_tcp_state(state);
    }

    pub fn initiate_migration_by_addr(&mut self, conn: (SocketAddrV4, SocketAddrV4)) {
        self.inner.borrow_mut().initiate_migration_by_addr(conn)
    }

    pub fn initiate_migration_by_qd(&mut self, qd: QDesc) -> Result<(), Fail> {
        self.inner.borrow_mut().initiate_migration_by_qd(qd)
    }

    pub fn poll_stats(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.recv_queue_stats.poll();
        inner.rps_stats.poll();

        // Send heartbeat.
        // let queue_len = inner.recv_queue_stats.avg_global_stat();
        // inner.tcpmig.send_heartbeat(queue_len);
    }

    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_proactively_migrate(
        &mut self,
    ) -> Option<arrayvec::ArrayVec<(SocketAddrV4, SocketAddrV4), { super::stats::MAX_EXTRACTED_CONNECTIONS }>> {
        self.inner.borrow_mut().rps_stats.connections_to_proactively_migrate()
    }

    #[cfg(not(feature = "manual-tcp-migration"))]
    pub fn connections_to_reactively_migrate(
        &mut self,
    ) -> Option<arrayvec::ArrayVec<(SocketAddrV4, SocketAddrV4), { super::stats::MAX_EXTRACTED_CONNECTIONS }>> {
        let mut inner = self.inner.borrow_mut();
        if inner.reactive_migration_enabled {
            inner.recv_queue_stats.connections_to_reactively_migrate()
        } else {
            None
        }
    }

    pub fn global_recv_queue_length(&self) -> usize {
        self.inner.borrow().recv_queue_stats.global_stat()
    }

    pub fn register_application_state(&mut self, qd: QDesc, state: Rc<RefCell<dyn ApplicationState>>) {
        let mut inner = self.inner.borrow_mut();
        let remote = match inner.sockets.get(&qd) {
            Some(Socket::Established { remote, .. }) => *remote,
            Some(..) => panic!("unsupported socket variant for migration"), // return Err(Fail::new(EBADF, "unsupported socket variant for migration")),
            None => panic!("socket does not exist"), // return Err(Fail::new(EBADF, "socket does not exist")),
        };
        inner.tcpmig.register_application_state(remote, state);
    }

    pub fn get_migrated_application_state<T: ApplicationState + 'static>(
        &mut self,
        qd: QDesc,
    ) -> Option<Rc<RefCell<T>>> {
        let mut inner = self.inner.borrow_mut();
        let remote = match inner.sockets.get(&qd) {
            Some(Socket::Established { remote, .. }) => *remote,
            Some(..) => panic!("unsupported socket variant for migration"), // return Err(Fail::new(EBADF, "unsupported socket variant for migration")),
            None => panic!("socket does not exist"), // return Err(Fail::new(EBADF, "socket does not exist")),
        };
        inner.tcpmig.get_migrated_application_state(remote)
    }
}

#[cfg(feature = "tcp-migration")]
pub struct TcpMigContext {
    pub is_under_load: bool,
}

#[cfg(feature = "tcp-migration")]
impl Inner {
    pub fn set_user_connection_context(&mut self, qd: QDesc, context: Rc<dyn UserConnectionContext>) {
        let Some(user_connection) = &mut self.user_connection else {
            unimplemented!("user connection is not enabled")
        };
        let remote = if let Socket::Established { remote, .. } = &self.sockets[&qd] {
            remote
        } else {
            unimplemented!("connection is not established")
        };
        if let Some(user_data) = user_connection.pending_migrate_in.remove(remote) {
            context.migrate_in(user_data)
        }
        let replaced = user_connection.contexts.insert(qd, context);
        assert!(replaced.is_none())
    }

    fn receive_tcpmig(&mut self, ip_hdr: &Ipv4Header, buf: Buffer) -> Result<(), Fail> {
        use super::super::tcpmig::TcpmigReceiveStatus;

        let ctx = TcpMigContext {
            // Avg queue len > 12.5% of threshold.
            is_under_load: self.recv_queue_stats.avg_global_stat() > (self.recv_queue_stats.threshold() >> 3),
        };

        match self.tcpmig.receive(ip_hdr, buf, ctx)? {
            TcpmigReceiveStatus::Ok | TcpmigReceiveStatus::MigrationCompleted | TcpmigReceiveStatus::SentReject => {},

            TcpmigReceiveStatus::Rejected(local, remote) => match self.established.get(&(local, remote)) {
                Some(s) => s.cb.enable_stats(&mut self.recv_queue_stats, &mut self.rps_stats),
                None => panic!("migration rejected for non-existent connection: {:?}", (local, remote)),
            },

            TcpmigReceiveStatus::ReturnedBySwitch(local, remote) => {
                // #[cfg(not(feature = "manual-tcp-migration"))]
                // match self.established.get(&(local, remote)) {
                //     Some(s) => s.cb.enable_stats(&mut self.recv_queue_stats, &mut self.rps_stats),
                //     None => panic!("migration rejected for non-existent connection: {:?}", (local, remote)),
                // }

                // // Re-initiate another migration if manual migration returned by switch.
                // #[cfg(feature = "manual-tcp-migration")]
                self.initiate_migration_by_addr((local, remote));
            },

            TcpmigReceiveStatus::PrepareMigrationAcked(qd) => {
                // Set the qd to be freed.
                self.tcpmig_poll_state.set_qd(qd);

                // If fast migration is allowed, migrate out the connection immediately.
                // if true { /* ACTIVATE THIS FOR MIG_DELAY EVAL */
                // if self.tcpmig_poll_state.is_fast_migrate_enabled() { /* COMMENT OUT THIS FOR MIG_DELAY EVAL */
                let state = self.migrate_out_connection(qd)?;
                self.tcpmig.send_tcp_state(state);
                // }
            },
            TcpmigReceiveStatus::StateReceived(state, buf) => {
                self.migrate_in_connection(state, buf)?;
            },
            TcpmigReceiveStatus::HeartbeatResponse(global_queue_len_sum) => {
                self.recv_queue_stats.update_threshold(global_queue_len_sum);
            },
        };
        Ok(())
    }

    /// 1) Mark this socket as migrated out.
    /// 2) Check if this connection is established one.
    /// 3) Remove socket from Established hashmap.
    fn migrate_out_connection(&mut self, qd: QDesc) -> Result<TcpState, Fail> {
        capy_profile!("PROF_EXPORT");
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
            },
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

        // Wake the scheduler task for this connection, if any.
        // The scheduler doesn't poll every qt every call,
        // only once at the start and then doesn't poll it
        // until it gets a notification from the CB that a packet has arrived.
        // But consider this case: polls qt -> no packet yet -> scheduler
        // won't poll that qt until new packet arrives -> connection is migrated.
        // Since we depend on the connection being polled to tell the application
        // that it was migrated, it's a deadlock that leads to that connection
        // never returning ETCPMIG error and never getting removed from Redis.
        // Fixed it by waking the scheduler task right before migrating the connection.
        cb.wake_scheduler_task();

        let user_conn = if let Some(user_connection) = &mut self.user_connection {
            let context = user_connection.contexts.remove(&qd).unwrap();
            context.migrate_out();
            Some(context)
        } else {
            None
        };
        Ok(TcpState::new(cb.as_ref().into(), user_conn))
    }

    fn migrate_in_connection(&mut self, state: TcpState, user_data: Buffer) -> Result<(), Fail> {
        capy_profile!("PROF_IMPORT");
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
            state.cb,
        );
        let remote = cb.get_remote();

        // Receive all target-buffered packets into the CB.
        /* for (mut hdr, data) in buffered {
            capy_log_mig!("start receiving target-buffered packets into the CB");
            cb.receive(&mut hdr, data);
        } */

        match self.passive.get_mut(&cb.get_local()) {
            Some(passive) => passive.push_migrated_in(cb),
            None => return Err(Fail::new(libc::EBADF, "socket not listening")),
        };

        if let Some(user_connection) = &mut self.user_connection {
            let replaced = user_connection.pending_migrate_in.insert(remote, user_data);
            assert!(replaced.is_none())
        } else {
            assert!(user_data.is_empty())
        }

        Ok(())
    }

    pub fn initiate_migration_by_addr(&mut self, conn: (SocketAddrV4, SocketAddrV4)) {
        // capy_profile!("prepare");
        capy_time_log!("INIT_MIG,({})", conn.1);

        let qd = *self.qds.get(&conn).expect("no QD found for connection");

        // Disable stats for this connection.
        self.established
            .get(&conn)
            .expect("connection not in established table")
            .cb
            .disable_stats();

        self.tcpmig.initiate_migration(conn, qd);
    }

    pub fn initiate_migration_by_qd(&mut self, qd: QDesc) -> Result<(), Fail> {
        // capy_profile!("prepare");
        // capy_log_mig!("INIT MIG");
        let conn = match self.sockets.get(&qd) {
            None => {
                debug!("No entry in `sockets` for fd: {:?}", qd);
                return Err(Fail::new(EBADF, "socket does not exist"));
            },
            Some(Socket::Established { local, remote }) => (*local, *remote),
            Some(..) => {
                return Err(Fail::new(EBADF, "unsupported socket variant for migrating out"));
            },
        };
        capy_time_log!("INIT_MIG,({})", conn.1);

        // Disable stats for this connection.
        self.established
            .get(&conn)
            .expect("connection not in established table")
            .cb
            .disable_stats();

        self.tcpmig.initiate_migration(conn, qd);
        Ok(())

        /* NON-CONCURRENT MIGRATION */
        /* if conn.1.port() == 203 {
            capy_time_log!("INIT_MIG,({})", conn.1);
            self.tcpmig.initiate_migration(conn, qd);
            Ok(())
        }else{
            return Err(Fail::new(EBADF, "this connection is not for migration"));
        } */
        /* NON-CONCURRENT MIGRATION */
    }
}

//==============================================================================
// TCP State
//==============================================================================

#[cfg(feature = "tcp-migration")]
pub mod state {
    use std::{
        cell::RefCell,
        net::SocketAddrV4,
        rc::Rc,
    };

    use crate::{
        capy_log_mig,
        capy_profile,
        inetstack::protocols::{
            tcp::established::ControlBlockState,
            tcpmig::{
                ApplicationState,
                MigratedApplicationState,
            },
        },
        runtime::memory::{
            Buffer,
            DataBuffer,
        },
    };

    pub trait Serialize {
        /// Serializes into the buffer and returns its unused part.
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8];
    }

    #[cfg(feature = "tcp-migration")]
    pub trait Deserialize: Sized {
        /// Deserializes and removes the deserialised part from the buffer.
        fn deserialize_from(buf: &mut Buffer) -> Self;
    }

    pub struct TcpState {
        pub cb: ControlBlockState,
        pub app_state: MigratedApplicationState,
        // None for a. disabled user connection b. incoming migrated connection whose user
        // connection is not specified yet
        // this results in a bit asymmetry between serialize and deserialize: deserialization does
        // not cover the user data, which is preserved and pass to user connection context as is
        // hope not too confusing
        pub user_conn: Option<Rc<dyn super::UserConnectionContext>>,
    }

    impl Serialize for TcpState {
        fn serialize_into<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
            self.cb.serialize_into(buf)
        }
    }

    impl TcpState {
        pub fn new(cb: ControlBlockState, user_conn: Option<Rc<dyn super::UserConnectionContext>>) -> Self {
            Self {
                cb,
                app_state: MigratedApplicationState::None,
                user_conn,
            }
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
            // capy_profile!("PROF_SERIALIZE");
            let mut buf = Buffer::Heap(DataBuffer::new(self.serialized_size()).unwrap());
            let remaining = self.cb.serialize_into(&mut buf);

            // match &self.app_state {
            //     MigratedApplicationState::Registered(state) if state.borrow().serialized_size() == 0 => {
            //         remaining[0] = 0;
            //     },
            //     MigratedApplicationState::Registered(state) => {
            //         remaining[0] = 1;
            //         state.borrow().serialize(&mut remaining[1..]);
            //     },
            //     MigratedApplicationState::None | MigratedApplicationState::MigratedIn(..) => {
            //         remaining[0] = 0;
            //     },
            // }
            assert!(matches!(self.app_state, MigratedApplicationState::None));
            if let Some(user_conn) = &self.user_conn {
                user_conn.write(remaining);
            }

            buf
        }

        pub fn deserialize(buf: &mut Buffer) -> Self {
            // capy_profile!("PROF_DESERIALIZE");
            let cb = ControlBlockState::deserialize_from(buf);
            // let app_state = if buf[0] == 0 {
            //     MigratedApplicationState::None
            // } else {
            //     buf.adjust(1);
            //     MigratedApplicationState::MigratedIn(buf)
            // };
            let app_state = MigratedApplicationState::None;
            Self {
                cb,
                app_state,
                user_conn: None,
            }
        }

        pub fn add_app_state(&mut self, app_state: Rc<RefCell<dyn ApplicationState>>) {
            self.app_state = MigratedApplicationState::Registered(app_state);
        }

        fn serialized_size(&self) -> usize {
            self.cb.serialized_size()
                // + 1
                // + match &self.app_state {
                //     MigratedApplicationState::Registered(state) => state.borrow().serialized_size(),
                //     MigratedApplicationState::None | MigratedApplicationState::MigratedIn(..) => 0,
                // }
                + self.user_conn.as_ref().map(|user_conn| user_conn.buf_size()).unwrap_or_default()
        }
    }

    impl PartialEq for TcpState {
        fn eq(&self, other: &Self) -> bool {
            self.cb == other.cb
        }
    }
    impl Eq for TcpState {}

    #[cfg(test)]
    mod test {
        use std::net::{
            Ipv4Addr,
            SocketAddrV4,
        };

        use crate::{
            capy_profile,
            capy_profile_dump,
            inetstack::protocols::{
                ethernet2::Ethernet2Header,
                ipv4::Ipv4Header,
                tcpmig::{
                    segment::{
                        TcpMigDefragmenter,
                        TcpMigHeader,
                        TcpMigSegment,
                    },
                    MigratedApplicationState,
                },
            },
            runtime::memory::{
                Buffer,
                DataBuffer,
            },
            MacAddress,
        };

        use super::TcpState;

        fn get_socket_addr() -> SocketAddrV4 {
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 10000)
        }

        fn get_buf() -> Buffer {
            Buffer::Heap(DataBuffer::from_slice(&vec![1; 64]))
        }

        fn get_state() -> TcpState {
            TcpState {
                cb: super::super::super::established::test_get_control_block_state(),
                app_state: MigratedApplicationState::None,
                user_conn: None,
            }
        }

        fn get_header() -> TcpMigHeader {
            TcpMigHeader::new(
                get_socket_addr(),
                get_socket_addr(),
                100,
                crate::inetstack::protocols::tcpmig::segment::MigrationStage::ConnectionState,
                10000,
                10000,
            )
        }

        #[inline(always)]
        fn create_segment(payload: Buffer) -> TcpMigSegment {
            let len = payload.len();
            TcpMigSegment::new(
                Ethernet2Header::new(
                    MacAddress::broadcast(),
                    MacAddress::broadcast(),
                    crate::inetstack::protocols::ethernet2::EtherType2::Ipv4,
                ),
                Ipv4Header::new(
                    Ipv4Addr::LOCALHOST,
                    Ipv4Addr::LOCALHOST,
                    crate::inetstack::protocols::ip::IpProtocol::UDP,
                ),
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

            let (hdr, mut buf) = segment.unwrap();
            let state = {
                // capy_profile!("deserialise");
                TcpState::deserialize(&mut buf)
            };
            assert_eq!(state.cb, get_state().cb);

            capy_profile_dump!(&mut std::io::stderr().lock());
            eprintln!();
        }
    }
}
