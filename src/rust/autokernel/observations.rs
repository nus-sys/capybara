// observations.rs
use std::cell::RefCell;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AutokernelObservations {
    pub num_rx_pkts: usize,
    pub packet_drops: usize,
    pub avg_queue_len: f64,
}

thread_local! {
    pub static AK_OBS: RefCell<AutokernelObservations> = RefCell::new(AutokernelObservations {
        num_rx_pkts: 0,
        packet_drops: 0,
        avg_queue_len: 0.0,
    });
}

pub fn get_obs<T, F>(f: F) -> T
where
    F: Fn(&AutokernelObservations) -> T,
{
    AK_OBS.with(|obs| f(&obs.borrow()))
}

pub fn set_obs<F>(f: F)
where
    F: Fn(&mut AutokernelObservations),
{
    AK_OBS.with(|obs| f(&mut obs.borrow_mut()))
}
