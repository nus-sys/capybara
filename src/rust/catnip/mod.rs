// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod config;
pub mod interop;
pub mod runtime;

//==============================================================================
// Imports
//==============================================================================

use self::{
    interop::pack_result,
    runtime::DPDKRuntime,
};
use crate::{
    capy_log, demikernel::config::Config, inetstack::{
        operations::OperationResult,
        InetStack,
    }, runtime::{
        fail::Fail,
        libdpdk::load_mlx_driver,
        memory::MemoryRuntime,
        timer::{
            Timer,
            TimerRc,
        },
        types::{
            demi_qresult_t,
            demi_sgarray_t,
        },
        QDesc,
        QToken,
    }, scheduler::{
        Scheduler,
        SchedulerHandle,
    }
};
use std::time::Duration;
use ::std::{
    net::SocketAddrV4,
    ops::{
        Deref,
        DerefMut,
    },
    rc::Rc,
    time::{
        Instant,
        SystemTime,
    },
};

#[cfg(feature = "profiler")]
use crate::timer;
//==============================================================================
// Structures
//==============================================================================

/// Catnip LibOS
pub struct CatnipLibOS {
    scheduler: Scheduler,
    inetstack: InetStack,
    rt: Rc<DPDKRuntime>,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for Catnip LibOS
impl CatnipLibOS {
    pub fn new(config: &Config) -> Self {
        load_mlx_driver();
        let rt: Rc<DPDKRuntime> = Rc::new(DPDKRuntime::new(
            config.local_ipv4_addr(),
            &config.eal_init_args(),
            config.arp_table(),
            config.disable_arp(),
            config.use_jumbo_frames(),
            config.mtu(),
            config.mss(),
            config.tcp_checksum_offload(),
            config.udp_checksum_offload(),
        ));
        let mtu = config.mtu();
        capy_log!("CatnipLibOS::MTU: {}", mtu);
        let use_jumbo_frames = config.use_jumbo_frames();
        capy_log!("CatnipLibOS::Use Jumbo Frames?: {:?}", use_jumbo_frames);
        let now: Instant = Instant::now();
        let clock: TimerRc = TimerRc(Rc::new(Timer::new(now)));
        let scheduler: Scheduler = Scheduler::default();
        
        /* capy_log!("Created default scheduler"); */
        
        let rng_seed: [u8; 32] = [0; 32];
        let inetstack: InetStack = InetStack::new(
            rt.clone(),
            scheduler.clone(),
            clock,
            rt.link_addr,
            rt.ipv4_addr,
            rt.udp_options.clone(),
            rt.tcp_options.clone(),
            rng_seed,
            rt.arp_options.clone(),
        )
        .unwrap();
        CatnipLibOS {
            inetstack,
            scheduler,
            rt,
        }
    }

    /// Create a push request for Demikernel to asynchronously write data from `sga` to the
    /// IO connection represented by `qd`. This operation returns immediately with a `QToken`.
    /// The data has been written when [`wait`ing](Self::wait) on the QToken returns.
    pub fn push(&mut self, qd: QDesc, sga: &demi_sgarray_t) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::push");
        trace!("push(): qd={:?}", qd);
        capy_log!("[0] pushing");
        match self.rt.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_push(qd, buf)?;
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    pub fn pushto(&mut self, qd: QDesc, sga: &demi_sgarray_t, to: SocketAddrV4) -> Result<QToken, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::pushto");
        trace!("pushto2(): qd={:?}", qd);
        match self.rt.clone_sgarray(sga) {
            Ok(buf) => {
                if buf.len() == 0 {
                    return Err(Fail::new(libc::EINVAL, "zero-length buffer"));
                }
                let future = self.do_pushto(qd, buf, to)?;
                let handle: SchedulerHandle = match self.scheduler.insert(future) {
                    Some(handle) => handle,
                    None => return Err(Fail::new(libc::EAGAIN, "cannot schedule co-routine")),
                };
                let qt: QToken = handle.into_raw().into();
                Ok(qt)
            },
            Err(e) => Err(e),
        }
    }

    /// Waits for an operation to complete.
    pub fn wait(&mut self, qt: QToken) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::wait");
        trace!("wait(): qt={:?}", qt);

        let (qd, result): (QDesc, OperationResult) = self.wait2(qt)?;
        Ok(pack_result(self.rt.clone(), result, qd, qt.into()))
    }

    /// Waits for an I/O operation to complete or a timeout to expire.
    pub fn timedwait(&mut self, qt: QToken, abstime: Option<SystemTime>) -> Result<demi_qresult_t, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::timedwait");
        trace!("timedwait() qt={:?}, timeout={:?}", qt, abstime);

        let (qd, result): (QDesc, OperationResult) = self.timedwait2(qt, abstime)?;
        Ok(pack_result(self.rt.clone(), result, qd, qt.into()))
    }

    /// Waits for any operation to complete.
    pub fn wait_any(&mut self, qts: &[QToken], qrs: &mut [demi_qresult_t], indices: &mut [usize], timeout: Option<Duration>) -> Result<usize, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::wait_any");
        trace!("wait_any(): qts={:?}", qts);

        self.inetstack.wait_any(self.rt.clone(), qts, qrs, indices, timeout)
    }

    pub fn try_wait_any(&mut self, qts: &[QToken]) -> Result<Option<Vec<(usize, demi_qresult_t)>>, Fail> {
        #[cfg(feature = "profiler")]
        timer!("catnip::try_wait_any");
        trace!("try_wait_any(): qts={:?}", qts);

        unimplemented!("C API for try_wait_any()");
    }

    /// Allocates a scatter-gather array.
    pub fn sgaalloc(&self, size: usize) -> Result<demi_sgarray_t, Fail> {
        self.rt.alloc_sgarray(size)
    }

    /// Releases a scatter-gather array.
    pub fn sgafree(&self, sga: demi_sgarray_t) -> Result<(), Fail> {
        self.rt.free_sgarray(sga)
    }

    /// Releases a scatter-gather array.
    pub fn dpdk_print_eth_stats() {
        DPDKRuntime::dpdk_print_eth_stats();
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

/// De-Reference Trait Implementation for Catnip LibOS
impl Deref for CatnipLibOS {
    type Target = InetStack;

    fn deref(&self) -> &Self::Target {
        &self.inetstack
    }
}

/// Mutable De-Reference Trait Implementation for Catnip LibOS
impl DerefMut for CatnipLibOS {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inetstack
    }
}
