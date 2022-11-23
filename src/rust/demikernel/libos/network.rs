// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use crate::{
    runtime::{
        fail::Fail,
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
// Exports
//======================================================================================================================

pub use crate::inetstack::operations::OperationResult;

//======================================================================================================================
// Structures
//======================================================================================================================

/// Network LIBOS.
pub enum NetworkLibOS {
    #[cfg(feature = "catpowder-libos")]
    Catpowder(CatpowderLibOS),
    #[cfg(feature = "catnap-libos")]
    Catnap(CatnapLibOS),
    #[cfg(feature = "catcollar-libos")]
    Catcollar(CatcollarLibOS),
    #[cfg(feature = "catnip-libos")]
    Catnip(CatnipLibOS),
}

//======================================================================================================================
// Associated Functions
//======================================================================================================================

/// Associated functions for network LibOSes.
impl NetworkLibOS {
    /// Waits on a pending operation in an I/O queue.
    pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.wait_any2(qts),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.wait_any2(qts),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.wait_any2(qts),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.wait_any2(qts),
        }
    }

    /// Waits on a pending operation in an I/O queue.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.wait2(qt),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.wait2(qt),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.wait2(qt),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.wait2(qt),
        }
    }

    /// Checks if an operation has completed and returns the result if it has.
    pub fn trywait2(&mut self, qt: QToken) -> Result<Option<(QDesc, OperationResult)>, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => todo!("catpowder `trywait2()`"),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => todo!("catnap `trywait2()`"),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => todo!("catcollar `trywait2()`"),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.trywait2(qt),
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait2(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<(QDesc, OperationResult), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => todo!("catpowder `timedwait2()`"),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => todo!("catnap `timedwait2()`"),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => todo!("catcollar `timedwait2()`"),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.timedwait2(qt, abstime),
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
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.socket(domain, socket_type, protocol),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.socket(domain, socket_type, protocol),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.socket(domain, socket_type, protocol),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.socket(domain, socket_type, protocol),
        }
    }

    /// Binds a socket to a local address.
    pub fn bind(&mut self, fd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.bind(fd, local),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.bind(fd, local),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.bind(fd, local),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.bind(fd, local),
        }
    }

    /// Marks a socket as a passive one.
    pub fn listen(&mut self, fd: QDesc, backlog: usize) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.listen(fd, backlog),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.listen(fd, backlog),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.listen(fd, backlog),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.listen(fd, backlog),
        }
    }

    /// Accepts an incoming connection on a TCP socket.
    pub fn accept(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.accept(fd),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.accept(fd),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.accept(fd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.accept(fd),
        }
    }

    /// Initiates a connection with a remote TCP pper.
    pub fn connect(&mut self, fd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.connect(fd, remote),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.connect(fd, remote),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.connect(fd, remote),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.connect(fd, remote),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, fd: QDesc) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.close(fd),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.close(fd),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.close(fd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.close(fd),
        }
    }

    /// Pushes a scatter-gather array to a TCP socket.
    pub fn push(&mut self, fd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.push(fd, sga),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.push(fd, sga),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.push(fd, sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.push(fd, sga),
        }
    }

    /// Pushes raw data to a TCP socket.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.push2(qd, data),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.push2(qd, data),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.push2(qd, data),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.push2(qd, data),
        }
    }

    /// Pushes a scatter-gather array to a UDP socket.
    pub fn pushto(&mut self, fd: QDesc, sga: &demi_sgarray_t, to: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.pushto(fd, sga, to),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.pushto(fd, sga, to),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.pushto(fd, sga, to),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.pushto(fd, sga, to),
        }
    }

    /// Pushes raw data to a UDP socket.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: SocketAddrV4) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.pushto2(qd, data, remote),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.pushto2(qd, data, remote),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.pushto2(qd, data, remote),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.pushto2(qd, data, remote),
        }
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, fd: QDesc) -> Result<QToken, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.pop(fd),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.pop(fd),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.pop(fd),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.pop(fd),
        }
    }

    /// Waits for a pending operation in an I/O queue.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.wait(qt),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.wait(qt),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.wait(qt),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.wait(qt),
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    #[allow(unreachable_patterns)]
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.timedwait(qt, abstime),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.timedwait(qt, abstime),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.timedwait(qt, abstime),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.timedwait(qt, abstime),
        }
    }

    /// Waits for any operation in an I/O queue.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.wait_any(qts),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.wait_any(qts),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.wait_any(qts),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.wait_any(qts),
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.sgaalloc(size),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.sgaalloc(size),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.sgaalloc(size),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.sgaalloc(size),
        }
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(libos) => libos.sgafree(sga),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(libos) => libos.sgafree(sga),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(libos) => libos.sgafree(sga),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.sgafree(sga),
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
        _server_target_fd: QDesc,
        _conn_fd: QDesc,
        _server_origin_listen: SocketAddrV4,
        _server_target_listen: SocketAddrV4,
    ) -> Result<MigrationHandle, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.initiate_tcp_migration_out(_server_target_fd, _conn_fd, _server_origin_listen, _server_target_listen),
        }
    }

    pub fn try_complete_tcp_migration_out(&mut self, _handle: MigrationHandle) -> Result<bool, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.try_complete_tcp_migration_out(_handle),
        }
    }

    pub fn perform_tcp_migration_in_sync(&mut self, _server_origin_fd: QDesc) -> Result<QDesc, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.perform_tcp_migration_in_sync(_server_origin_fd),
        }
    }

    pub fn tcp_migration_lock(&mut self, _qd: QDesc) -> Result<TcpMigrationLock, Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.tcp_migration_lock(_qd),
        }
    }

    pub fn tcp_migration_unlock(&mut self, _lock: TcpMigrationLock) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.tcp_migration_unlock(_lock),
        }
    }

    #[cfg(feature = "tcp-migration")]
    pub fn notify_migration_safety(&mut self, _fd: QDesc) -> Result<(), Fail> {
        match self {
            #[cfg(feature = "catpowder-libos")]
            NetworkLibOS::Catpowder(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnap-libos")]
            NetworkLibOS::Catnap(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catcollar-libos")]
            NetworkLibOS::Catcollar(_libos) => Err(Fail::new(libc::EOPNOTSUPP, "tcp migration only supported for catnip")),
            #[cfg(feature = "catnip-libos")]
            NetworkLibOS::Catnip(libos) => libos.notify_migration_safety(_fd),
        }
    }
}
