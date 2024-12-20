// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod name;
pub mod network;

//======================================================================================================================
// Imports
//======================================================================================================================

use self::{
    name::LibOSName,
    network::{
        NetworkLibOS,
        OperationResult,
    },
};
use crate::{
    demikernel::config::Config,
    runtime::{
        fail::Fail,
        logging,
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
    }
};
use std::{cell::RefCell, os::raw::c_void, rc::Rc, time::Duration};
use ::std::{
    env,
    net::SocketAddrV4,
    time::SystemTime,
    collections::HashSet,
};

#[cfg(feature = "catcollar-libos")]
use crate::catcollar::CatcollarLibOS;
#[cfg(feature = "catnap-libos")]
use crate::catnap::CatnapLibOS;
#[cfg(feature = "catnip-libos")]
use crate::catnip::CatnipLibOS;
#[cfg(feature = "catpowder-libos")]
use crate::catpowder::CatpowderLibOS;

//======================================================================================================================
// Structures
//======================================================================================================================

/// LibOS
pub enum LibOS {
    /// Network LibOS
    NetworkLibOS(NetworkLibOS),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for LibOS.
impl LibOS {
    /// Instantiates a new LibOS.
    pub fn new(libos_name: LibOSName) -> Result<Self, Fail> {
        logging::initialize();
        crate::capylog::init();

        assert_eq!(unsafe { libc::atexit(exit_dump) }, 0, "atexit() call failed");

        // Read in configuration file.
        let config_path: String = match env::var("CONFIG_PATH") {
            Ok(config_path) => config_path,
            Err(_) => {
                return Err(Fail::new(
                    libc::EINVAL,
                    "missing value for CONFIG_PATH environment variable",
                ))
            },
        };
        let config: Config = Config::new(config_path);

        // Instantiate LibOS.
        #[allow(unreachable_patterns)]
        let libos: LibOS = match libos_name {
            #[cfg(feature = "catnap-libos")]
            LibOSName::Catnap => Self::NetworkLibOS(NetworkLibOS::Catnap(CatnapLibOS::new(&config))),
            #[cfg(feature = "catcollar-libos")]
            LibOSName::Catcollar => Self::NetworkLibOS(NetworkLibOS::Catcollar(CatcollarLibOS::new(&config))),
            #[cfg(feature = "catpowder-libos")]
            LibOSName::Catpowder => Self::NetworkLibOS(NetworkLibOS::Catpowder(CatpowderLibOS::new(&config))),
            #[cfg(feature = "catnip-libos")]
            LibOSName::Catnip => Self::NetworkLibOS(NetworkLibOS::Catnip(CatnipLibOS::new(&config))),
            _ => panic!("unsupported libos"),
        };

        Ok(libos)
    }

    /// Waits on a pending operation in an I/O queue.
    pub fn wait_any2(&mut self, qts: &[QToken], qrs: &mut [(QDesc, OperationResult)], indices: &mut [usize], timeout: Option<Duration>) -> Result<usize, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any2(qts, qrs, indices, timeout),
        }
    }

    pub fn capybara_switch(&mut self) {
        match self {
            LibOS::NetworkLibOS(libos) => libos.capybara_switch(),
        }
    }

    pub fn wait_any_nonblocking2(&mut self, qts: &[QToken], qrs: &mut [(QDesc, OperationResult)], indices: &mut [usize]) -> Result<usize, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any_nonblocking2(qts, qrs, indices),
        }
    }

    pub fn wait_any_nonblocking_one2(&mut self, qts: &[QToken]) -> Result<Option<(usize, QDesc, OperationResult)>, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any_nonblocking_one2(qts),
        }
    }

    /// Waits on a pending operation in an I/O queue.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait2(qt),
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait2(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<(QDesc, OperationResult), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.timedwait2(qt, abstime),
        }
    }

    /// Creates a socket.
    pub fn socket(
        &mut self,
        domain: libc::c_int,
        socket_type: libc::c_int,
        protocol: libc::c_int,
    ) -> Result<QDesc, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.socket(domain, socket_type, protocol),
        }
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, fd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.bind(fd, local),
        }
    }

    /// Marks a socket as a passive one.
    pub fn listen(&mut self, fd: QDesc, backlog: usize) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.listen(fd, backlog),
        }
    }

    /// Accepts an incoming connection on a TCP socket.
    pub fn accept(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.accept(fd),
        }
    }

    /// Initiates a connection with a remote TCP pper.
    pub fn connect(&mut self, fd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.connect(fd, remote),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, fd: QDesc) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.close(fd),
        }
    }

    /// Pushes a scatter-gather array to a TCP socket.
    pub fn push(&mut self, fd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.push(fd, sga),
        }
    }

    /// Pushes raw data to a TCP socket.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.push2(qd, data),
        }
    }

    /// Pushes a scatter-gather array to a UDP socket.
    pub fn pushto(&mut self, fd: QDesc, sga: &demi_sgarray_t, to: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto(fd, sga, to),
        }
    }

    /// Pushes raw data to a UDP socket.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pushto2(qd, data, remote),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.pop(fd),
        }
    }

    /// Waits for a pending operation in an I/O queue.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait(qt),
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.timedwait(qt, abstime),
        }
    }

    /// Waits for any operation in an I/O queue.
    pub fn wait_any(&mut self, qts: &[QToken], qrs: &mut [demi_qresult_t], indices: &mut [usize], timeout: Option<Duration>) -> Result<usize, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts, qrs, indices, timeout),
        }
    }

    pub fn try_wait_any(&mut self, qts: &[QToken]) -> Result<Option<Vec<(usize, demi_qresult_t)>>, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.try_wait_any(qts),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.sgaalloc(size),
        }
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.sgafree(sga),
        }
    }    
}

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcpmig::ApplicationState;

#[cfg(feature = "tcp-migration")]
impl LibOS {
    pub fn initiate_migration(&mut self, qd: QDesc) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.initiate_migration(qd),
        }
    }

    pub fn global_recv_queue_length(&self) -> usize {
        match self {
            LibOS::NetworkLibOS(libos) => libos.global_recv_queue_length(),
        }
    }

    pub fn register_application_state(&mut self, qd: QDesc, state: Rc<RefCell<dyn ApplicationState>>) {
        match self {
            LibOS::NetworkLibOS(libos) => libos.register_application_state(qd, state)
        }
    }

    pub fn get_migrated_application_state<T: ApplicationState + 'static>(&mut self, qd: QDesc) -> Option<Rc<RefCell<T>>> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.get_migrated_application_state(qd)
        }
    }
}

// For logging-related functions.

impl LibOS {
    pub fn capylog_dump<W: std::io::Write>(dump: &mut W) {
        crate::capy_profile_dump!(dump);
        crate::capy_time_log_dump!(dump);
    }

    #[cfg(feature = "catnip-libos")]
    pub fn dpdk_print_eth_stats() {
        CatnipLibOS::dpdk_print_eth_stats();
    }
}

extern "C" fn exit_dump() {
    eprintln!("Exiting process.");
    #[cfg(feature = "catnip-libos")]
    LibOS::dpdk_print_eth_stats();
    LibOS::capylog_dump(&mut std::io::stderr().lock());
}