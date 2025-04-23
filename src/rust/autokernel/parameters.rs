// parameters.rs
use std::cell::RefCell;
use std::env;

pub const AK_MAX_RECEIVE_BATCH_SIZE: usize = 128;
pub const POP_SIZE: usize = 9216;
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct AutokernelParameters {
    pub timer_resolution: usize,
    pub max_recv_iters: usize,
    pub max_out_of_order: usize,
    pub rto_alpha: f64,
    pub rto_beta: f64,
    pub rto_granularity: f64,
    pub rto_lower_bound_sec: f64,
    pub rto_upper_bound_sec: f64,
    pub unsent_queue_cutoff: usize,
    pub beta_cubic: f32,
    pub cubic_c: f32,
    pub dup_ack_threshold: u32,
    pub waker_page_size: usize,
    pub first_slot_size: usize,
    pub waker_bit_length_shift: usize,
    pub fallback_mss: usize,
    pub receive_batch_size: usize,
    pub pop_size: usize,
}

thread_local! {
    pub static AK_PARMS: RefCell<AutokernelParameters> = RefCell::new(AutokernelParameters {
        timer_resolution: env::var("TIMER_RESOLUTION").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        max_recv_iters: env::var("MAX_RECV_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(2),
        max_out_of_order: env::var("MAX_OUT_OF_ORDER").ok().and_then(|v| v.parse().ok()).unwrap_or(2048),
        rto_alpha: env::var("RTO_ALPHA").ok().and_then(|v| v.parse().ok()).unwrap_or(0.125),
        rto_beta: env::var("RTO_BETA").ok().and_then(|v| v.parse().ok()).unwrap_or(0.25),
        rto_granularity: env::var("RTO_GRANULARITY").ok().and_then(|v| v.parse().ok()).unwrap_or(0.001),
        rto_lower_bound_sec: env::var("RTO_LOWER_BOUND_SEC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.1),
        rto_upper_bound_sec: env::var("RTO_UPPER_BOUND_SEC").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0),
        unsent_queue_cutoff: env::var("UNSENT_QUEUE_CUTOFF").ok().and_then(|v| v.parse().ok()).unwrap_or(1024),
        beta_cubic: env::var("BETA_CUBIC").ok().and_then(|v| v.parse().ok()).unwrap_or(0.7),
        cubic_c: env::var("C").ok().and_then(|v| v.parse().ok()).unwrap_or(0.4),
        dup_ack_threshold: env::var("DUP_ACK_THRESHOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(3),
        waker_page_size: env::var("WAKER_PAGE_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(64),
        first_slot_size: env::var("FIRST_SLOT_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(16),
        waker_bit_length_shift: env::var("WAKER_BIT_LENGTH_SHIFT").ok().and_then(|v| v.parse().ok()).unwrap_or(6),
        fallback_mss: env::var("FALLBACK_MSS").ok().and_then(|v| v.parse().ok()).unwrap_or(536),
        receive_batch_size: env::var("RECEIVE_BATCH_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(100),
        pop_size: env::var("POP_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(POP_SIZE),
    });
}

pub fn get_param<T, F>(f: F) -> T
where
    F: Fn(&AutokernelParameters) -> T,
{
    AK_PARMS.with(|params| f(&params.borrow()))
}

pub fn set_param<F>(f: F)
where
    F: Fn(&mut AutokernelParameters),
{
    AK_PARMS.with(|params| f(&mut params.borrow_mut()))
}
