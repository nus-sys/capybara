use std::{cell::RefCell, env};

pub const AK_MAX_RECEIVE_BATCH_SIZE: usize = 100;

pub struct AutokernelParameters {
    pub receive_batch_size: usize,
    pub timer_resolution: usize,

    pub default_body_pool_size: usize,
    pub default_cache_size: usize,
}

// Thread-local storage for AutokernelParameters
thread_local! {
    pub static AK_PARMS: RefCell<AutokernelParameters> = RefCell::new({
        let receive_batch_size = env::var("RECEIVE_BATCH_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(4);
        let timer_resolution = env::var("TIMER_RESOLUTION")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(64);

        let default_body_pool_size = env::var("DEFAULT_BODY_POOL_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(8192 - 1);
        let default_cache_size = env::var("DEFAULT_CACHE_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(250);

        eprintln!("RECEIVE_BATCH_SIZE: {}", receive_batch_size);
        eprintln!("TIMER_RESOLUTION: {}", timer_resolution);
        eprintln!("DEFAULT_BODY_POOL_SIZE: {}", default_body_pool_size);
        eprintln!("DEFAULT_CACHE_SIZE: {}", default_cache_size);

        AutokernelParameters {
            receive_batch_size,
            timer_resolution,
            default_body_pool_size,
            default_cache_size,
        }
    });
}

// Generic function to get any member variable of AutokernelParameters
pub fn get_param<T, F>(f: F) -> T
where
    F: Fn(&AutokernelParameters) -> T,
{
    AK_PARMS.with(|params| {
        let params = params.borrow();
        f(&params)
    })
}

// Example: Generic setter function for modifying a parameter
pub fn set_param<F>(f: F)
where
    F: Fn(&mut AutokernelParameters),
{
    AK_PARMS.with(|params| {
        let mut params = params.borrow_mut();
        f(&mut params);
    });
}