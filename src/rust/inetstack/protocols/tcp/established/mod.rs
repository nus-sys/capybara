// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod background;
pub mod congestion_control;
mod ctrlblk;
mod rto;
mod sender;

pub use self::{
    ctrlblk::{
        ControlBlock,
        Receiver,
        State,
    },
    sender::{
        Sender,
        UnackedSegment,
    },
};

use self::background::background;
use crate::{
    inetstack::{
        futures::FutureOperation,
        protocols::tcp::segment::TcpHeader,
    },
    runtime::{
        fail::Fail,
        memory::Buffer,
        QDesc,
    },
    scheduler::SchedulerHandle,
};
use ::futures::{
    channel::mpsc,
    FutureExt,
};
use ::std::{
    net::SocketAddrV4,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

use crate::capy_log;

#[cfg(feature = "tcp-migration")]
pub use ctrlblk::state::ControlBlockState;

#[cfg(all(feature = "tcp-migration", test))]
pub use ctrlblk::state::test::get_state as test_get_control_block_state;

#[cfg(feature = "tcp-migration")]
use crate::inetstack::protocols::tcpmig::TcpMigPeer;

pub struct EstablishedSocket {
    pub cb: Rc<ControlBlock>,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: SchedulerHandle,
}

impl EstablishedSocket {
    pub fn new(cb: ControlBlock, fd: QDesc, dead_socket_tx: mpsc::UnboundedSender<QDesc>) -> Self {
        let cb = Rc::new(cb);
        let future = background(cb.clone(), fd, dead_socket_tx);
        capy_log!("Create new EstablishedSocket, scheduling background task");
        let handle: SchedulerHandle = match cb.scheduler.insert(FutureOperation::Background(future.boxed_local())) {
            Some(handle) => handle,
            None => panic!("failed to insert task in the scheduler"),
        };
        Self {
            cb: cb.clone(),
            background: handle,
        }
    }

    pub fn receive(
        &self, 
        header: &mut TcpHeader, 
        data: Buffer,
        #[cfg(feature = "tcp-migration")]
        tcpmig: &TcpMigPeer,
    ) {
        self.cb.receive(
            header, 
            data,
            #[cfg(feature = "tcp-migration")]
            tcpmig,
        )
    }

    pub fn send(&self, buf: Buffer, #[cfg(feature = "tcp-migration")] tcpmig: &TcpMigPeer) -> Result<(), Fail> {
        self.cb.send(buf, #[cfg(feature = "tcp-migration")] Some(tcpmig))
    }

    pub fn poll_recv(
        &self, 
        ctx: &mut Context,
        #[cfg(feature = "tcp-migration")]
        tcpmig: &TcpMigPeer,
    ) -> Poll<Result<Buffer, Fail>> {
        self.cb.poll_recv(
            ctx,
            #[cfg(feature = "tcp-migration")]
            tcpmig,
        )
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.cb.close()
    }

    pub fn remote_mss(&self) -> usize {
        self.cb.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.cb.rto()
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.cb.get_local(), self.cb.get_remote())
    }
}
