// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use crate::{
    capy_log_mig, capy_profile, capy_profile_total, capy_time_log, inetstack::{
        futures::operation::FutureOperation,
        operations::OperationResult,
        protocols::{
            arp::ArpPeer,
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            tcp::operations::ConnectFuture,
            udp::UdpOperation,
            Peer,
        },
    }, runtime::{
        fail::Fail, memory::{
            Buffer,
            DataBuffer,
        }, network::{
            config::{
                ArpConfig,
                TcpConfig,
                UdpConfig,
            },
            types::MacAddress,
            NetworkRuntime,
        }, queue::IoQueueTable, timer::TimerRc, types::demi_qresult_t, QDesc, QToken, QType
    }, scheduler::{
        FutureResult,
        Scheduler,
        SchedulerHandle,
    }
};
use ::libc::{
    c_int,
    EBADF,
    EINVAL,
    ENOTSUP,
};

use std::{cell::RefCell, collections::HashSet, time::Duration};
use ::std::{
    any::Any,
    convert::TryFrom,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    rc::Rc,
    time::{
        Instant,
        SystemTime,
    },
};

#[cfg(feature = "profiler")]
use crate::timer;

#[cfg(feature = "tcp-migration")]
use protocols::tcpmig::{TcpmigPollState, ApplicationState};

use crate::capy_log;

use chrono::NaiveTime;

//==============================================================================
// Exports
//==============================================================================

#[cfg(test)]
pub mod test_helpers;

pub mod collections;
pub mod futures;
pub mod operations;
pub mod options;
pub mod protocols;

//==============================================================================
// Constants
//==============================================================================

const TIMER_RESOLUTION: usize = 64;
const MAX_RECV_ITERS: usize = 2;

//==============================================================================
// InetStack
//==============================================================================

pub struct InetStack {
    arp: ArpPeer,
    ipv4: Peer,
    file_table: IoQueueTable,
    rt: Rc<dyn NetworkRuntime>,
    local_link_addr: MacAddress,
    scheduler: Scheduler,
    clock: TimerRc,
    ts_iters: usize,
    prev_time: NaiveTime,

    #[cfg(feature = "tcp-migration")]
    tcpmig_state: TcpMigState,
}

#[cfg(feature = "tcp-migration")]
struct TcpMigState {
    poll_state: Rc<TcpmigPollState>,

    // These QDs need to be freed at the correct point, since the connections they represent have been migrated.
    migrated_qds: HashSet<QDesc>,
}

impl InetStack {
    pub fn new(
        rt: Rc<dyn NetworkRuntime>,
        scheduler: Scheduler,
        clock: TimerRc,
        local_link_addr: MacAddress,
        local_ipv4_addr: Ipv4Addr,
        udp_config: UdpConfig,
        tcp_config: TcpConfig,
        rng_seed: [u8; 32],
        arp_config: ArpConfig,
    ) -> Result<Self, Fail> {
        let file_table: IoQueueTable = IoQueueTable::new();

        #[cfg(feature = "tcp-migration")]
        let tcpmig_state = TcpMigState {
            poll_state: Rc::new(TcpmigPollState::default()),
            migrated_qds: HashSet::new(),
        };

        let arp: ArpPeer = ArpPeer::new(
            rt.clone(),
            scheduler.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            arp_config,
        )?;
        let ipv4: Peer = Peer::new(
            rt.clone(),
            scheduler.clone(),
            clock.clone(),
            local_link_addr,
            local_ipv4_addr,
            udp_config,
            tcp_config,
            arp.clone(),
            rng_seed,

            #[cfg(feature = "tcp-migration")]
            tcpmig_state.poll_state.clone()
        )?;
        Ok(Self {
            arp,
            ipv4,
            file_table,
            rt,
            local_link_addr,
            scheduler,
            clock,
            ts_iters: 0,
            prev_time: chrono::Local::now().time(),

            #[cfg(feature = "tcp-migration")]
            tcpmig_state
        })
    }

    ///
    /// **Brief**
    ///
    /// Creates an endpoint for communication and returns a file descriptor that
    /// refers to that endpoint. The file descriptor returned by a successful
    /// call will be the lowest numbered file descriptor not currently open for
    /// the process.
    ///
    /// The domain argument specifies a communication domain; this selects the
    /// protocol family which will be used for communication. These families are
    /// defined in the libc crate. Currently, the following families are supported:
    ///
    /// - AF_INET Internet Protocol Version 4 (IPv4)
    ///
    /// **Return Vale**
    ///
    /// Upon successful completion, a file descriptor for the newly created
    /// socket is returned. Upon failure, `Fail` is returned instead.
    ///
    pub fn socket(&mut self, domain: c_int, socket_type: c_int, _protocol: c_int) -> Result<QDesc, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::socket");
        trace!(
            "socket(): domain={:?} type={:?} protocol={:?}",
            domain,
            socket_type,
            _protocol
        );
        if domain != libc::AF_INET {
            return Err(Fail::new(ENOTSUP, "address family not supported"));
        }
        match socket_type {
            libc::SOCK_STREAM => {
                let qd: QDesc = self.file_table.alloc(QType::TcpSocket.into());
                if let Err(e) = self.ipv4.tcp.do_socket(qd) {
                    self.file_table.free(qd);
                    Err(e)
                } else {
                    Ok(qd)
                }
            },
            libc::SOCK_DGRAM => {
                let qd: QDesc = self.file_table.alloc(QType::UdpSocket.into());
                if let Err(e) = self.ipv4.udp.do_socket(qd) {
                    self.file_table.free(qd);
                    Err(e)
                } else {
                    Ok(qd)
                }
            },
            _ => Err(Fail::new(ENOTSUP, "socket type not supported")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Binds the socket referred to by `qd` to the local endpoint specified by
    /// `local`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn bind(&mut self, qd: QDesc, local: SocketAddrV4) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::bind");
        trace!("bind(): qd={:?} local={:?}", qd, local);
        match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => self.ipv4.tcp.bind(qd, local),
                Ok(QType::UdpSocket) => self.ipv4.udp.do_bind(qd, local),
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Marks the socket referred to by `qd` as a socket that will be used to
    /// accept incoming connection requests using [accept](Self::accept). The `qd` should
    /// refer to a socket of type `SOCK_STREAM`. The `backlog` argument defines
    /// the maximum length to which the queue of pending connections for `qd`
    /// may grow. If a connection request arrives when the queue is full, the
    /// client may receive an error with an indication that the connection was
    /// refused.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn listen(&mut self, qd: QDesc, backlog: usize) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::listen");
        trace!("listen(): qd={:?} backlog={:?}", qd, backlog);
        if backlog == 0 {
            return Err(Fail::new(EINVAL, "invalid backlog length"));
        }
        match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => self.ipv4.tcp.listen(qd, backlog),
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Accepts an incoming connection request on the queue of pending
    /// connections for the listening socket referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to wait for a connection request to arrive. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn accept(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::accept");
        trace!("accept(): {:?}", qd);

        // Search for target queue descriptor.
        match self.file_table.get(qd) {
            // Found, check if it concerns a TCP socket.
            Some(qtype) => match QType::try_from(qtype) {
                // It does, so allocate a new queue descriptor and issue accept operation.
                Ok(QType::TcpSocket) => {
                    let new_qd: QDesc = {
                        #[cfg(feature = "tcp-migration")]
                        if let Some(new_qd) = self.tcpmig_state.migrated_qds.iter().next().copied() {
                            assert!(self.tcpmig_state.migrated_qds.remove(&new_qd));
                            new_qd
                        } else {
                            self.file_table.alloc(QType::TcpSocket.into())
                        }

                        #[cfg(not(feature = "tcp-migration"))]
                        self.file_table.alloc(QType::TcpSocket.into())
                    };
                    let future: FutureOperation = FutureOperation::from(self.ipv4.tcp.do_accept(qd, new_qd));
                    /* capy_log!("Scheduling Accept operation"); */
                    let handle: SchedulerHandle = match self.scheduler.insert(future) {
                        Some(handle) => handle,
                        None => {
                            self.file_table.free(new_qd);
                            return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine"));
                        },
                    };
                    Ok(handle.into_raw().into())
                },
                // This queue descriptor does not concern a TCP socket.
                _ => Err(Fail::new(libc::EINVAL, "invalid queue type")),
            },
            // The queue descriptor was not found.
            _ => Err(Fail::new(libc::EBADF, "bad queue descriptor")),
        }
    }

    ///
    /// **Brief**
    ///
    /// Connects the socket referred to by `qd` to the remote endpoint specified by `remote`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, a queue token is returned. This token can be
    /// used to push and pop data to/from the queue that connects the local and
    /// remote endpoints. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn connect(&mut self, qd: QDesc, remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::connect");
        trace!("connect(): qd={:?} remote={:?}", qd, remote);
        let future = match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => {
                    let fut: ConnectFuture = self.ipv4.tcp.connect(qd, remote)?;
                    Ok(FutureOperation::from(fut))
                },
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor")),
        }?;

        let handle: SchedulerHandle = match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.into_raw().into();
        trace!("connect() qt={:?}", qt);
        Ok(qt)
    }

    ///
    /// **Brief**
    ///
    /// Closes a connection referred to by `qd`.
    ///
    /// **Return Value**
    ///
    /// Upon successful completion, `Ok(())` is returned. Upon failure, `Fail` is
    /// returned instead.
    ///
    pub fn close(&mut self, qd: QDesc) -> Result<(), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::close");
        trace!("close(): qd={:?}", qd);

        match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => self.ipv4.tcp.do_close(qd)?,
                Ok(QType::UdpSocket) => self.ipv4.udp.do_close(qd)?,
                _ => Err(Fail::new(EINVAL, "invalid queue type"))?,
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor"))?,
        }

        self.file_table.free(qd);

        Ok(())
    }

    /// Pushes a buffer to a TCP socket.
    /// TODO: Rename this function to push() once we have a common representation across all libOSes.
    pub fn do_push(&mut self, qd: QDesc, buf: Buffer) -> Result<FutureOperation, Fail> {
        match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => Ok(FutureOperation::from(self.ipv4.tcp.push(qd, buf))),
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor")),
        }
    }

    /// Pushes raw data to a TCP socket.
    /// TODO: Move this function to demikernel repo once we have a common buffer representation across all libOSes.
    pub fn push2(&mut self, qd: QDesc, data: &[u8]) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::push2");
        trace!("push2(): qd={:?}", qd);

        // Convert raw data to a buffer representation.
        let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(data));
        if buf.is_empty() {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        // Issue operation.
        let future: FutureOperation = self.do_push(qd, buf)?;
        capy_log!("Scheduling PUSH");
        let handle: SchedulerHandle = match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.into_raw().into();
        trace!("push2() qt={:?}", qt);
        Ok(qt)
    }

    /// Pushes a buffer to a UDP socket.
    /// TODO: Rename this function to pushto() once we have a common buffer representation across all libOSes.
    pub fn do_pushto(&mut self, qd: QDesc, buf: Buffer, to: SocketAddrV4) -> Result<FutureOperation, Fail> {
        match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::UdpSocket) => {
                    let udp_op = UdpOperation::Pushto(qd, self.ipv4.udp.do_pushto(qd, buf, to));
                    Ok(FutureOperation::Udp(udp_op))
                },
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => Err(Fail::new(EBADF, "bad queue descriptor")),
        }
    }

    /// Pushes raw data to a UDP socket.
    /// TODO: Move this function to demikernel repo once we have a common buffer representation across all libOSes.
    pub fn pushto2(&mut self, qd: QDesc, data: &[u8], remote: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::pushto2");
        trace!("pushto2(): qd={:?}", qd);

        // Convert raw data to a buffer representation.
        let buf: Buffer = Buffer::Heap(DataBuffer::from_slice(data));
        if buf.is_empty() {
            return Err(Fail::new(EINVAL, "zero-length buffer"));
        }

        // Issue operation.
        let future: FutureOperation = self.do_pushto(qd, buf, remote)?;
        let handle: SchedulerHandle = match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.into_raw().into();
        trace!("pushto2() qt={:?}", qt);
        Ok(qt)
    }

    /// Create a pop request to write data from IO connection represented by `qd` into a buffer
    /// allocated by the application.
    pub fn pop(&mut self, qd: QDesc) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::pop");

        trace!("pop(): qd={:?}", qd);

        let future = match self.file_table.get(qd) {
            Some(qtype) => match QType::try_from(qtype) {
                Ok(QType::TcpSocket) => Ok(FutureOperation::from(self.ipv4.tcp.pop(qd)?)),
                Ok(QType::UdpSocket) => {
                    let udp_op = UdpOperation::Pop(FutureResult::new(self.ipv4.udp.do_pop(qd), None));
                    Ok(FutureOperation::Udp(udp_op))
                },
                _ => Err(Fail::new(EINVAL, "invalid queue type")),
            },
            _ => {
                capy_log!("EBADF {qd:?}");
                Err(Fail::new(EBADF, "bad queue descriptor"))
            },
        }?;
        capy_log!("Scheduling POP");
        let handle: SchedulerHandle = match self.scheduler.insert(future) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
        };
        let qt: QToken = handle.into_raw().into();
        trace!("pop() qt={:?}", qt);
        Ok(qt)
    }

    /// Waits for an operation to complete.
    pub fn wait2(&mut self, qt: QToken) -> Result<(QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::wait2");
        trace!("wait2(): qt={:?}", qt);

        // Retrieve associated schedule handle.
        let handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                trace!("wait2() qt={:?} completed!", qt);
                return Ok(self.take_operation(handle));
            }
        }
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait2(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<(QDesc, OperationResult), Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::timedwait2");
        trace!("timedwait2() qt={:?}, timeout={:?}", qt, abstime);

        // Retrieve associated schedule handle.
        let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
            Some(handle) => handle,
            None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
        };

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            // The operation has completed, so extract the result and return.
            if handle.has_completed() {
                return Ok(self.take_operation(handle));
            }

            if abstime.is_none() || SystemTime::now() >= abstime.unwrap() {
                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
                return Err(Fail::new(libc::ETIMEDOUT, "timer expired"));
            }
        }
    }

    /// Waits for any operation to complete.
    /// 
    /// The length of `qrs` and `indices` needs to be at least as big as `qts`. If `qrs` is not big enough, all results are not written to it.
    /// 
    /// Returns the number of results written to `qrs`.
    pub fn wait_any2(&mut self, qts: &[QToken], qrs: &mut [(QDesc, OperationResult)], indices: &mut [usize], timeout: Option<Duration>) -> Result<usize, Fail> {
        #[cfg(feature = "profiler")]
        timer!("inetstack::wait_any2");
        let begin = Instant::now();

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            let mut completed = 0;
            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                if completed == qrs.len() {
                    break;
                }

                // Retrieve associated schedule handle.
                // TODO: move this out of the loop.
                let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
                    qrs[completed] = (qd, r); // Store the completed handle result.
                    indices[completed] = i; // Store the index of the result.
                    completed += 1;
                } else {
                    // Return this operation to the scheduling queue by removing the associated key
                    // (which would otherwise cause the operation to be freed).
                    handle.take_key();
                }
            }

            if completed > 0 {
                return Ok(completed);
            }

            if let Some(timeout) = timeout {
                if timeout <= begin.elapsed() {
                    return Ok(0);
                }
            }
        }
    }

    pub fn capybara_switch(&mut self) {
        loop {
            let batch = self.rt.receive();
            if batch.is_empty() {
                continue;
            }
            for pkt in batch {
                capy_log!("[RX]");
                if let Err(e) = self.do_receive(pkt) {
                    warn!("Dropped packet: {:?}", e);
                }
            }
        }
    }

    /// Waits for any operation to complete.
    /// 
    /// The length of `qrs` and `indices` needs to be at least as big as `qts`. If `qrs` is not big enough, all results are not written to it.
    /// 
    /// Returns the number of results written to `qrs`.
    #[cfg(feature = "catnip-libos")]
    pub fn wait_any(
        &mut self,
        rt: Rc<crate::catnip::runtime::DPDKRuntime>,
        qts: &[QToken], qrs: &mut [demi_qresult_t], indices: &mut [usize], timeout: Option<Duration>,
    ) -> Result<usize, Fail> {
        let begin = Instant::now();

        loop {
            // Poll first, so as to give pending operations a chance to complete.
            self.poll_bg_work();

            let mut completed = 0;
            // Search for any operation that has completed.
            for (i, &qt) in qts.iter().enumerate() {
                if completed == qrs.len() {
                    break;
                }

                // Retrieve associated schedule handle.
                let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
                };

                // Found one, so extract the result and return.
                if handle.has_completed() {
                    let (qd, result): (QDesc, OperationResult) = self.take_operation(handle);
                    qrs[completed] = super::catnip::interop::pack_result(rt.clone(), result, qd, qt.into()); // Store the completed handle result.
                    indices[completed] = i; // Store the index of the result.
                    completed += 1;
                } else {
                    // Return this operation to the scheduling queue by removing the associated key
                    // (which would otherwise cause the operation to be freed).
                    handle.take_key();
                }
            }

            if completed > 0 {
                return Ok(completed);
            }

            if let Some(timeout) = timeout {
                if timeout <= begin.elapsed() {
                    return Ok(0);
                }
            }
        }
    }

    /// Checks if any operation is complete. If not, immediately returns.
    /// 
    /// The length of `qrs` and `indices` needs to be at least as big as `qts`. If `qrs` is not big enough, all results are not written to it.
    /// 
    /// Returns the number of results written to `qrs`.
    pub fn wait_any_nonblocking2(&mut self, qts: &[QToken], qrs: &mut [(QDesc, OperationResult)], indices: &mut [usize]) -> Result<usize, Fail> {
        // Poll first, so as to give pending operations a chance to complete.
        self.poll_bg_work();

        let mut completed = 0;
        // Search for any operation that has completed.
        for (i, &qt) in qts.iter().enumerate() {
            // Retrieve associated schedule handle.
            // TODO: move this out of the loop.
            let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
                Some(handle) => handle,
                None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
            };

            // Found one, so extract the result and return.
            if handle.has_completed() {
                let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
                qrs[completed] = (qd, r); // Store the completed handle result.
                indices[completed] = i; // Store the index of the result.
                completed += 1;
            } else {
                // Return this operation to the scheduling queue by removing the associated key
                // (which would otherwise cause the operation to be freed).
                handle.take_key();
            }
        }
        Ok(completed)
    }

    pub fn wait_any_nonblocking_one2(&mut self, qts: &[QToken]) -> Result<Option<(usize, QDesc, OperationResult)>, Fail> {
        // Poll first, so as to give pending operations a chance to complete.
        self.poll_bg_work();
        // Search for any operation that has completed.
        for (i, &qt) in qts.iter().enumerate() {
            // Retrieve associated schedule handle.
            // TODO: move this out of the loop.
            let mut handle: SchedulerHandle = match self.scheduler.from_raw_handle(qt.into()) {
                Some(handle) => handle,
                None => return Err(Fail::new(libc::EINVAL, "invalid queue token")),
            };

            // Found one, so extract the result and return.
            if handle.has_completed() {
                let (qd, r): (QDesc, OperationResult) = self.take_operation(handle);
                return Ok(Some((i, qd, r)));
            }
            // Return this operation to the scheduling queue by removing the associated key
            // (which would otherwise cause the operation to be freed).
            handle.take_key();
        } 
        Ok(None)
    }

    /// Given a handle representing a task in our scheduler. Return the results of this future
    /// and the file descriptor for this connection.
    ///
    /// This function will panic if the specified future had not completed or is _background_ future.
    pub fn take_operation(&mut self, handle: SchedulerHandle) -> (QDesc, OperationResult) {
        let boxed_future: Box<dyn Any> = self.scheduler.take(handle).as_any();
        let boxed_concrete_type: FutureOperation = *boxed_future.downcast::<FutureOperation>().expect("Wrong type!");

        match boxed_concrete_type {
            FutureOperation::Tcp(f) => {
                let (qd, new_qd, qr): (QDesc, Option<QDesc>, OperationResult) = f.expect_result();

                // Handle accept failures.
                if let Some(new_qd) = new_qd {
                    match qr {
                        // Operation failed, so release queue descriptor.
                        OperationResult::Failed(_) => {
                            self.file_table.free(new_qd);
                        },
                        // Operation succeeded.
                        _ => (),
                    }
                }

                (qd, qr)
            },
            FutureOperation::Udp(f) => f.get_result(),
            FutureOperation::Background(..) => {
                panic!("`take_operation` attempted on background task!")
            },
        }
    }

    /// New incoming data has arrived. Route it to the correct parse out the Ethernet header and
    /// allow the correct protocol to handle it. The underlying protocol will futher parse the data
    /// and inform the correct task that its data has arrived.
    fn do_receive(&mut self, bytes: Buffer) -> Result<(), Fail> {
        capy_profile_total!("do_receive");
        #[cfg(feature = "profiler")]
        timer!("inetstack::engine::receive");
        let (header, payload) = Ethernet2Header::parse(bytes)?;
        debug!("Engine received {:?}", header);
        // if self.local_link_addr != header.dst_addr() && !header.dst_addr().is_broadcast() {
        //     // ToDo: Add support for is_multicast() to MacAddress type.  Then remove following trace and restore return.
        //     trace!("Need to add && !header.dst_addr().is_multicast()");
        //     //return Err(Fail::new(EINVAL, "physical destination address mismatch"));
        // }
        match header.ether_type() {
            EtherType2::Arp => self.arp.receive(payload),
            EtherType2::Ipv4 => self.ipv4.receive(
                payload, 
                #[cfg(feature = "capybara-switch")]
                header,
            ),
            EtherType2::Ipv6 => Ok(()), // Ignore for now.
        }
    }

    /// Scheduler will poll all futures that are ready to make progress.
    /// Then ask the runtime to receive new data which we will forward to the engine to parse and
    /// route to the correct protocol.
    pub fn poll_bg_work(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("inetstack::poll_bg_work");

        // #[cfg(feature = "tcp-migration")]
        // let was_runtime_polled = self.poll_runtime_tcpmig();
        // self.prev_time = chrono::Local::now().time();
        
        // Inho: After using a single RX queue, 
        // Redis shows poor tail latency without this on for some reason. 
        // So, purposely add this line for eval.  
        // #[cfg(feature = "tcp-migration")]{
        self.poll_runtime_no_scheduler_poll();
        {
            #[cfg(feature = "profiler")]
            timer!("inetstack::poll_bg_work::poll");
            self.scheduler.poll();
        }
        // }
        // #[cfg(not(feature = "tcp-migration"))]
        // self.poll_runtime();

        // #[cfg(feature = "tcp-migration")]
        // if !was_runtime_polled {
        //     self.poll_runtime();
        // }
        

        #[cfg(feature = "tcp-migration")]
        {
            // Poll stats updates.
            self.ipv4.tcp.poll_stats();

            // Perform RPS Signal reception action.
            self.ipv4.tcp.rps_signal_action();
        
            // If overloaded, start migrations.
            /* comment out this for recv_queue_len vs mig_lat eval */
            #[cfg(not(feature = "manual-tcp-migration"))]
            if let Some(conns_to_migrate) = self.ipv4.tcp.connections_to_reactively_migrate() {
                for conn in conns_to_migrate {
                    self.ipv4.tcp.initiate_migration_by_addr(conn);
                }
            }
            /* comment out this for recv_queue_len vs mig_lat eval */
        }

        if self.ts_iters == 0 {
            self.clock.advance_clock(Instant::now());
        }
        self.ts_iters = (self.ts_iters + 1) % TIMER_RESOLUTION;
    }

    fn poll_runtime(&mut self) {
        #[cfg(feature = "profiler")]
        timer!("inetstack::poll_bg_work::for");
        
        for _ in 0..MAX_RECV_ITERS {
            let batch = {
                #[cfg(feature = "profiler")]
                timer!("inetstack::poll_bg_work::for::receive");
                self.rt.receive()
            };
            if batch.is_empty() {
                break;
            }
            for pkt in batch {
                if let Err(e) = self.do_receive(pkt) {
                    warn!("Dropped packet: {:?}", e);
                }
                // TODO: This is a workaround for https://github.com/demikernel/inetstack/issues/149.
                self.scheduler.poll();
            }
        }
    }
    /// Exactly the same as `poll_runtime()` but does not poll the scheduler after every packet.
    fn poll_runtime_no_scheduler_poll(&mut self) {
        for _ in 0..MAX_RECV_ITERS {
            let batch = {
                #[cfg(feature = "profiler")]
                timer!("inetstack::poll_bg_work::for::receive");
                self.rt.receive()
            };
            if batch.is_empty() {
                break;
            }
            for pkt in batch {
                capy_log!("[RX] pkt");
                if let Err(e) = self.do_receive(pkt) {
                    warn!("Dropped packet: {:?}", e);
                }
            }
        }
    }
}

#[cfg(feature = "tcp-migration")]
impl InetStack {
    /// Returns if TCP DPDK queue was also polled.
    pub fn poll_runtime_tcpmig(&mut self) -> bool {
        // capy_profile!("poll_tcpmig()");
        // let recv_time: NaiveTime = chrono::Local::now().time();

        #[cfg(feature = "catnip-libos")]{
            // Get TCPMIG packets.
            let tcpmig_batch = {
                // capy_profile!("receive_tcpmig");

                // Soundness: Inetstack runtime is always DPDKRuntime.
                unsafe { self.rt.as_dpdk_runtime().unwrap_unchecked() }.receive_tcpmig()
            };
            
            //capy_time_log!("poll_dpdk_interval,{},{}", recv_time, self.prev_time);

            // Reset poll state, and disallow fast migrations initially since TCP DPDK queue might have unpolled packets.
            self.tcpmig_state.poll_state.reset();
            
            for pkt in tcpmig_batch {
                // Parse the packet. If it is a PREPARE_MIG_ACK, 
                if let Err(e) = self.do_receive(pkt) {
                    warn!("Dropped packet: {:?}", e);
                }
                // Get qd to free if a connection was/will be migrated out.
                let qd_to_remove = match self.tcpmig_state.poll_state.take_qd() {
                    None => continue,
                    Some(qd) => qd,
                };

                // Control only enters here if a PREPARE_ACK was received.

                // Fast migration is disabled, so TCP queue hasn't been polled yet. Poll it.
                /* COMMENT OUT THIS FOR MIG_DELAY EVAL */
                if !self.tcpmig_state.poll_state.is_fast_migrate_enabled() {
                    // Poll the TCP DPDK queue.
                    capy_log_mig!("PREPARE_MIG_ACK slow path entered");
                    self.poll_runtime_no_scheduler_poll();

                    // Migrate out the pending connection.
                    self.ipv4.tcp.migrate_out_and_send(qd_to_remove);

                    // Enable fast migration now that TCP queue has been flushed.
                    self.tcpmig_state.poll_state.enable_fast_migrate();
                }
                /* COMMENT OUT THIS FOR MIG_DELAY EVAL */
                
                // Track the QD as migrated.
                assert!(self.tcpmig_state.migrated_qds.insert(qd_to_remove));
                capy_log_mig!("Freeing QD {qd_to_remove:?}");
            }
        }
        // self.prev_time = recv_time;

        // The state of fast migrate also indicates if any migration occured.
        self.tcpmig_state.poll_state.is_fast_migrate_enabled()
    }


    pub fn initiate_migration(&mut self, qd: QDesc) -> Result<(), Fail> {
        self.ipv4.tcp.initiate_migration_by_qd(qd)
    }

    pub fn global_recv_queue_length(&self) -> usize {
        self.ipv4.tcp.global_recv_queue_length()
    }

    pub fn register_application_state(&mut self, qd: QDesc, state: Rc<RefCell<dyn ApplicationState>>) {
        self.ipv4.tcp.register_application_state(qd, state);
    }

    pub fn get_migrated_application_state<T: ApplicationState + 'static>(&mut self, qd: QDesc) -> Option<Rc<RefCell<T>>> {
        self.ipv4.tcp.get_migrated_application_state(qd)
    }
}