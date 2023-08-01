// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod background;
pub mod congestion_control;
mod ctrlblk;
mod rto;
mod sender;

pub use self::ctrlblk::{
    ControlBlock,
    State,
};

#[cfg(feature = "tcp-migration")]
pub use self::{
    sender::UnackedSegment,
    ctrlblk::ControlBlockState,
};

use crate::{
    inetstack::protocols::tcp::segment::TcpHeader,
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        queue::BackgroundTask,
        QDesc,
    },
    scheduler::TaskHandle,
};
use ::futures::channel::mpsc;
use ::std::{
    net::SocketAddrV4,
    rc::Rc,
    task::{
        Context,
        Poll,
    },
    time::Duration,
};

#[cfg(feature = "capybara-log")]
use crate::tcpmig_profiler::tcp_log;

#[derive(Clone)]
pub struct EstablishedSocket<const N: usize> {
    pub cb: Rc<ControlBlock<N>>,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: TaskHandle,
}

impl<const N: usize> EstablishedSocket<N> {
    pub fn new(cb: ControlBlock<N>, qd: QDesc, dead_socket_tx: mpsc::UnboundedSender<QDesc>) -> Self {
        let cb = Rc::new(cb);
        // TODO: Maybe add the queue descriptor here.
        let task: BackgroundTask = BackgroundTask::new(
            String::from("Inetstack::TCP::established::background"),
            Box::pin(background::background(cb.clone(), qd, dead_socket_tx)),
        );
        #[cfg(feature = "capybara-log")]
        {
            tcp_log(format!("Create new EstablishedSocket, scheduling background task"));
        }
        let handle: TaskHandle = match cb.scheduler.insert(task) {
            Some(handle) => handle,
            None => panic!("failed to insert task in the scheduler"),
        };
        Self {
            cb: cb.clone(),
            background: handle.clone(),
        }
    }

    pub fn receive(&self, header: &mut TcpHeader, data: DemiBuffer) {
        self.cb.receive(header, data)
    }

    pub fn send(&self, buf: DemiBuffer) -> Result<(), Fail> {
        self.cb.send(buf)
    }

    pub fn poll_recv(&self, ctx: &mut Context, size: Option<usize>) -> Poll<Result<DemiBuffer, Fail>> {
        self.cb.poll_recv(ctx, size)
    }

    pub fn close(&self) -> Result<(), Fail> {
        self.cb.close()
    }

    pub fn poll_close(&self) -> Poll<Result<(), Fail>> {
        self.cb.poll_close()
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

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

impl<const N: usize> Drop for EstablishedSocket<N> {
    fn drop(&mut self) {
        self.background.deschedule();
    }
}
