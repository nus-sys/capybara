// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod futures;
mod iouring;
mod runtime;

//==============================================================================
// Exports
//==============================================================================

use crate::{
    catcollar::futures::{
        accept::AcceptFuture,
        connect::ConnectFuture,
        pop::PopFuture,
    },
    demikernel::dbuf::DataBuffer,
};

pub use self::{
    futures::OperationResult,
    runtime::IoUringRuntime,
};
use self::{
    futures::{
        push::PushFuture,
        pushto::PushtoFuture,
        Operation,
    },
    runtime::RequestId,
};

//==============================================================================
// Imports
//==============================================================================

use crate::Ipv4Endpoint;
use ::libc::c_int;
use ::nix::{
    sys::socket::{
        self,
        AddressFamily,
        InetAddr,
        SockAddr,
        SockFlag,
        SockProtocol,
        SockType,
    },
    unistd,
};
use ::runtime::{
    fail::Fail,
    logging,
    memory::{
        Buffer,
        MemoryRuntime,
    },
    network::types::{
        Ipv4Addr,
        Port16,
    },
    queue::IoQueueTable,
    task::SchedulerRuntime,
    types::{
        demi_accept_result_t,
        demi_opcode_t,
        demi_qr_value_t,
        demi_qresult_t,
        demi_sgarray_t,
    },
    QDesc,
    QToken,
    QType,
};
use ::scheduler::SchedulerHandle;
use ::std::{
    any::Any,
    collections::HashMap,
    mem,
    os::unix::prelude::RawFd,
    time::Instant,
};

#[cfg(feature = "profiler")]
use perftools::timer;

//==============================================================================
// Constants
//==============================================================================

// Size of receive buffers.
const CATCOLLAR_RECVBUF_SIZE: usize = 9000;

//==============================================================================
// Structures
//==============================================================================

/// Catcollar LibOS
pub struct CatcollarLibOS {
    /// Table of queue descriptors.
    qtable: IoQueueTable, // TODO: Move this to Demikernel module.
    /// Established sockets.
    sockets: HashMap<QDesc, RawFd>,
    /// Underlying runtime.
    runtime: IoUringRuntime,
}
//==============================================================================
// Associated Functions
//==============================================================================

/// Associate Functions for Catcollar LibOS
impl CatcollarLibOS {
    /// Instantiates a Catcollar LibOS.
    pub fn new() -> Self {
        logging::initialize();
        let qtable: IoQueueTable = IoQueueTable::new();
        let sockets: HashMap<QDesc, RawFd> = HashMap::new();
        let runtime: IoUringRuntime = IoUringRuntime::new(Instant::now());
        Self {
            qtable,
            sockets,
            runtime,
        }
    }

    /// Creates a socket.
    pub fn socket(&mut self, domain: c_int, typ: c_int, _protocol: c_int) -> Result<QDesc, Fail> {
        trace!("socket() domain={:?}, type={:?}, protocol={:?}", domain, typ, _protocol);

        // All operations are asynchronous.
        let flags: SockFlag = SockFlag::SOCK_NONBLOCK;

        // Parse communication domain.
        let domain: AddressFamily = match domain {
            libc::AF_INET => AddressFamily::Inet,
            _ => return Err(Fail::new(libc::ENOTSUP, "communication domain  not supported")),
        };

        // Parse socket type and protocol.
        let (ty, protocol): (SockType, SockProtocol) = match typ {
            libc::SOCK_STREAM => (SockType::Stream, SockProtocol::Tcp),
            libc::SOCK_DGRAM => (SockType::Datagram, SockProtocol::Udp),
            _ => {
                return Err(Fail::new(libc::ENOTSUP, "socket type not supported"));
            },
        };

        // Create socket.
        match socket::socket(domain, ty, flags, protocol) {
            Ok(fd) => {
                let qtype: QType = QType::TcpSocket;
                let qd: QDesc = self.qtable.alloc(qtype.into());
                assert_eq!(self.sockets.insert(qd, fd).is_none(), true);
                Ok(qd)
            },
            Err(err) => Err(Fail::new(err as i32, "failed to create socket")),
        }
    }

    /// Binds a socket to a local endpoint.
    pub fn bind(&mut self, qd: QDesc, local: Ipv4Endpoint) -> Result<(), Fail> {
        trace!("bind() qd={:?}, local={:?}", qd, local);

        // Issue bind operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let addr: SockAddr = parse_addr(local);
                socket::bind(fd, &addr).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Sets a socket as a passive one.
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        trace!("listen() qd={:?}, backlog={:?}", qd, backlog);

        // Issue listen operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                socket::listen(fd, backlog).unwrap();
                Ok(())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Accepts connections on a socket.
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("accept(): qd={:?}", qd);

        // Issue accept operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let new_qd: QDesc = self.qtable.alloc(QType::TcpSocket.into());
                let future: Operation = Operation::from(AcceptFuture::new(qd, fd, new_qd));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Establishes a connection to a remote endpoint.
    pub fn connect(&mut self, qd: QDesc, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        trace!("connect() qd={:?}, remote={:?}", qd, remote);

        // Issue connect operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let addr: SockAddr = parse_addr(remote);
                let future: Operation = Operation::from(ConnectFuture::new(qd, fd, addr));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Closes a socket.
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        trace!("close() qd={:?}", qd);
        match self.sockets.get(&qd) {
            Some(&fd) => match unistd::close(fd) {
                Ok(_) => Ok(()),
                _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    // Handles a push operation.
    fn do_push(&mut self, qd: QDesc, buf: DataBuffer) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(&fd) => {
                // Issue operation.
                let request_id: RequestId = self.runtime.push(fd, buf.clone())?;

                let future: Operation = Operation::from(PushFuture::new(self.runtime.clone(), request_id, qd));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        trace!("push() qd={:?}", qd);

        let buf: DataBuffer = self.runtime.clone_sgarray(sga)?;

        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue push operation.
        self.do_push(qd, buf)
    }

    // Pushes raw data to a socket.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        trace!("push2() qd={:?}", qd);

        let buf: DataBuffer = DataBuffer::from(data);
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue pushto operation.
        self.do_push(qd, buf)
    }

    /// Handles a pushto operation.
    fn do_pushto(&mut self, qd: QDesc, buf: DataBuffer, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        match self.sockets.get(&qd) {
            Some(&fd) => {
                // Issue operation.
                let addr: SockAddr = parse_addr(remote);
                let request_id: RequestId = self.runtime.pushto(fd, addr, buf.clone())?;

                let future: Operation = Operation::from(PushtoFuture::new(self.runtime.clone(), request_id, qd));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                Ok(handle.into_raw().into())
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Pushes a scatter-gather array to a socket.
    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        trace!("pushto() qd={:?}", qd);

        match self.runtime.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }

                // Issue pushto operation.
                self.do_pushto(qd, buf, remote)
            },
            Err(e) => Err(e),
        }
    }

    /// Pushes raw data to a socket.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: Ipv4Endpoint) -> Result<QToken, Fail> {
        trace!("pushto2() qd={:?}, remote={:?}", qd, remote);

        let buf: DataBuffer = DataBuffer::from_slice(data);
        if buf.len() == 0 {
            return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
        }

        // Issue pushto operation.
        self.do_pushto(qd, buf, remote)
    }

    /// Pops data from a socket.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        trace!("pop() qd={:?}", qd);

        let buf: DataBuffer = DataBuffer::new(CATCOLLAR_RECVBUF_SIZE)?;

        // Issue pop operation.
        match self.sockets.get(&qd) {
            Some(&fd) => {
                let request_id: RequestId = self.runtime.pop(fd, buf.clone())?;
                let future: Operation = Operation::from(PopFuture::new(self.runtime.clone(), request_id, qd, buf));
                let handle: SchedulerHandle = self.runtime.schedule(future);
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            _ => Err(Fail::new(libc::EBADF, "invalid queue descriptor")),
        }
    }

    /// Waits for an operation to complete.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catcollar::wait");
        trace!("wait() qt={:?}", qt);

        let (qd, result): (QDesc, OperationResult) = self.wait2(qt)?;
        Ok(pack_result(&self.runtime, result, qd, qt.into()))
    }

    /// Waits for an operation to complete.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catcollar::wait2");
        trace!("wait2() qt={:?}", qt);

        // Retrieve associated schedule handle.
        let handle: SchedulerHandle = match self.runtime.get_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.runtime.poll();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                return Ok(self.take_result(handle));
            }
        }
    }

    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken]) -> Result<(usize, demi_qresult_t), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catcollar::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        let (i, qd, r): (usize, QDesc, OperationResult) = self.wait_any2(qts)?;
        Ok((i, pack_result(&self.runtime, r, qd, qts[i].into())))
    }

    /// Waits for any operation to complete.
    pub fn wait_any2(&mut self, qts: &[QToken]) -> Result<(usize, QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("catcollar::wait_any2");
        trace!("wait_any2() {:?}", qts);

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.runtime.poll();

            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                // Retrieve associated schedule handle.
                // TODO: move this out of the loop.
                let mut handle: SchedulerHandle = match self.runtime.get_handle(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, r): (QDesc, OperationResult) = self.take_result(handle);
                    return Ok((i, qd, r));
                }

                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
            }
        }
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        trace!("sgalloc() size={:?}", size);
        self.runtime.alloc_sgarray(size)
    }

    /// Frees a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        trace!("sgafree()");
        self.runtime.free_sgarray(sga)
    }

    #[deprecated]
    pub fn local_ipv4_addr(&self) -> Ipv4Addr {
        todo!()
    }

    #[deprecated]
    pub fn rt(&self) -> &IoUringRuntime {
        &self.runtime
    }

    /// Takes out the operation result descriptor associated with the target scheduler handle.
    fn take_result(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.runtime.take(handle).as_any();
        let boxed_concrete_type: Operation = *boxed_future.downcast::<Operation>().expect("Wrong type!");

        let (qd, new_qd, new_fd, qr): (QDesc, Option<QDesc>, Option<RawFd>, OperationResult) =
            boxed_concrete_type.get_result();
        trace!("qd={:?}, new_qd={:?}, new_fd={:?}", qd, new_qd, new_fd,);

        // Handle accept operation.
        if let Some(new_qd) = new_qd {
            // Associate raw file descriptor with queue descriptor.
            if let Some(new_fd) = new_fd {
                assert_eq!(self.sockets.insert(new_qd, new_fd).is_none(), true);
            }
            // Release entry in queue table.
            else {
                self.qtable.free(new_qd);
            }
        }

        (qd, qr)
    }
}

//==============================================================================
// Standalone Functions
//==============================================================================

/// Parses a [Ipv4Endpoint] into a [SockAddr].
fn parse_addr(endpoint: Ipv4Endpoint) -> SockAddr {
    let ipv4: std::net::IpAddr = std::net::IpAddr::V4(endpoint.get_address());
    let ip: socket::IpAddr = socket::IpAddr::from_std(&ipv4);
    let portnum: Port16 = endpoint.get_port();
    let inet: InetAddr = InetAddr::new(ip, portnum.into());
    SockAddr::new_inet(inet)
}

/// Packs a [OperationResult] into a [demi_qresult_t].
fn pack_result(rt: &IoUringRuntime, result: OperationResult, qd: QDesc, qt: u64) -> demi_qresult_t {
    match result {
        OperationResult::Connect => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_CONNECT,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Accept(new_qd) => {
            let sin = unsafe { mem::zeroed() };
            let qr_value = demi_qr_value_t {
                ares: demi_accept_result_t {
                    qd: new_qd.into(),
                    addr: sin,
                },
            };
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_ACCEPT,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value,
            }
        },
        OperationResult::Push => demi_qresult_t {
            qr_opcode: demi_opcode_t::DEMI_OPC_PUSH,
            qr_qd: qd.into(),
            qr_qt: qt,
            qr_value: unsafe { mem::zeroed() },
        },
        OperationResult::Pop(addr, bytes) => match rt.into_sgarray(bytes) {
            Ok(mut sga) => {
                if let Some(endpoint) = addr {
                    sga.sga_addr.sin_port = endpoint.get_port().into();
                    sga.sga_addr.sin_addr.s_addr = u32::from_le_bytes(endpoint.get_address().octets());
                }
                let qr_value: demi_qr_value_t = demi_qr_value_t { sga };
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_POP,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value,
                }
            },
            Err(e) => {
                warn!("Operation Failed: {:?}", e);
                demi_qresult_t {
                    qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                    qr_qd: qd.into(),
                    qr_qt: qt,
                    qr_value: unsafe { mem::zeroed() },
                }
            },
        },
        OperationResult::Failed(e) => {
            warn!("Operation Failed: {:?}", e);
            demi_qresult_t {
                qr_opcode: demi_opcode_t::DEMI_OPC_FAILED,
                qr_qd: qd.into(),
                qr_qt: qt,
                qr_value: unsafe { mem::zeroed() },
            }
        },
    }
}