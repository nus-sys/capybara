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
    },
    inetstack::{MigrationHandle, TcpMigrationLock},
};
use ::std::{
    env,
    net::SocketAddrV4,
    time::SystemTime,
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
    pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any2(qts),
        }
    }

    /// Waits on a pending operation in an I/O queue.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait2(qt),
        }
    }

    /// Checks if an operation has completed and returns the result if it has.
    pub fn trywait2(&mut self, qt: QToken) -> Result<Option<(QDesc, OperationResult)>, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.trywait2(qt),
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
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.wait_any(qts),
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

    /// 
    /// Initiates the process (through TCP communication) to migrate out a tcp connection,
    /// provided the descriptor of a connection to the destination server.
    /// 
    /// `server_origin_listen`: Listening address for connection on origin server.
    /// 
    /// `server_dest_listen`: Listening address for connection on destination server.
    /// 
    pub fn initiate_tcp_migration_out(
        &mut self,
        server_target_fd: QDesc,
        conn_fd: QDesc,
        server_origin_listen: SocketAddrV4,
        server_target_listen: SocketAddrV4,
    ) -> Result<MigrationHandle, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.initiate_tcp_migration_out(
                server_target_fd,
                conn_fd,
                server_origin_listen,
                server_target_listen
            ),
        }
    }

    pub fn try_complete_tcp_migration_out(&mut self, handle: MigrationHandle) -> Result<bool, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.try_complete_tcp_migration_out(handle),
        }
    }

    /// Performs the complete process (synchronously, through TCP communication) to migrate in a tcp connection,
    /// provided the descriptor of a connection to the origin server.
    pub fn perform_tcp_migration_in_sync(&mut self, server_origin_fd: QDesc) -> Result<QDesc, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.perform_tcp_migration_in_sync(server_origin_fd),
        }
    }

    /// Prevent migration of the connection represented by `qd` until `tcp_migration_unlock()` is called.
    /// 
    /// Returns the lock to use when the connection needs to be unlocked.
    pub fn tcp_migration_lock(&mut self, qd: QDesc) -> Result<TcpMigrationLock, Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.tcp_migration_lock(qd),
        }
    }

    /// Allow migration of the migration-locked connection represented by `lock`.
    pub fn tcp_migration_unlock(&mut self, lock: TcpMigrationLock) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.tcp_migration_unlock(lock),
        }
    }

    pub fn notify_migration_safety(&mut self, fd: QDesc) -> Result<(), Fail> {
        match self {
            LibOS::NetworkLibOS(libos) => libos.notify_migration_safety(fd),
        }
    }
}
