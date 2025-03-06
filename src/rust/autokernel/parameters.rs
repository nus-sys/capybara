use std::{cell::RefCell, env};

pub const AK_MAX_RECEIVE_BATCH_SIZE: usize = 128;
pub const POP_SIZE: usize = 9216;

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

// Thread-local storage for AutokernelParameters
thread_local! {
    pub static AK_PARMS: RefCell<AutokernelParameters> = RefCell::new({
        
        let timer_resolution = env::var("TIMER_RESOLUTION")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(64);

        let max_recv_iters = env::var("MAX_RECV_ITERS")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(2);

        let max_out_of_order = env::var("MAX_OUT_OF_ORDER")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(2048);

        let rto_alpha = env::var("RTO_ALPHA")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.125);
        let rto_beta = env::var("RTO_BETA")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.25);
        let rto_granularity = env::var("RTO_GRANULARITY")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.001f64);
        let rto_lower_bound_sec = env::var("RTO_LOWER_BOUND_SEC")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.100f64);
        let rto_upper_bound_sec = env::var("RTO_UPPER_BOUND_SEC")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(60.0f64);
    
        let unsent_queue_cutoff = env::var("UNSENT_QUEUE_CUTOFF")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(1024);

        let beta_cubic = env::var("BETA_CUBIC")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.7);
        let cubic_c = env::var("C")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(0.4);

        let dup_ack_threshold = env::var("DUP_ACK_THRESHOLD")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(3);

        let waker_page_size = env::var("WAKER_PAGE_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(64);

        let first_slot_size = env::var("FIRST_SLOT_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(16);

        let waker_bit_length_shift = env::var("WAKER_BIT_LENGTH_SHIFT")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(6);

        let fallback_mss = env::var("FALLBACK_MSS")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(536);

        let receive_batch_size = env::var("RECEIVE_BATCH_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(100);

        let pop_size = env::var("POP_SIZE")
            .ok()
            .and_then(|val| val.parse().ok())
            .unwrap_or(POP_SIZE);
     


        eprintln!("TIMER_RESOLUTION: {}", timer_resolution);
        eprintln!("MAX_RECV_ITERS: {}", max_recv_iters);
        eprintln!("MAX_OUT_OF_ORDER: {}", max_out_of_order);
        eprintln!("RTO_ALPHA: {}", rto_alpha);
        eprintln!("RTO_BETA: {}", rto_beta);
        eprintln!("RTO_GRANULARITY: {}", rto_granularity);
        eprintln!("RTO_LOWER_BOUND_SEC: {}", rto_lower_bound_sec);
        eprintln!("RTO_UPPER_BOUND_SEC: {}", rto_upper_bound_sec);
        eprintln!("UNSENT_QUEUE_CUTOFF: {}", unsent_queue_cutoff);
        eprintln!("BETA_CUBIC: {}", beta_cubic);
        eprintln!("C: {}", cubic_c);
        eprintln!("DUP_ACK_THRESHOLD: {}", dup_ack_threshold);
        eprintln!("WAKER_PAGE_SIZE: {}", waker_page_size);
        eprintln!("FIRST_SLOT_SIZE: {}", first_slot_size);
        eprintln!("WAKER_BIT_LENGTH_SHIFT: {}", waker_bit_length_shift);
        eprintln!("FALLBACK_MSS: {}", fallback_mss);
        eprintln!("RECEIVE_BATCH_SIZE: {}", receive_batch_size);
        eprintln!("POP_SIZE: {}", pop_size);
        

        AutokernelParameters {
            timer_resolution,
            max_recv_iters,
            max_out_of_order,
            rto_alpha,
            rto_beta,
            rto_granularity,
            rto_lower_bound_sec,
            rto_upper_bound_sec,
            unsent_queue_cutoff,
            beta_cubic,
            cubic_c,
            dup_ack_threshold,
            waker_page_size,
            first_slot_size,
            waker_bit_length_shift,
            fallback_mss,
            receive_batch_size,
            pop_size,
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


use std::os::unix::net::UnixStream;
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/controller_socket";

pub fn controller_test() {
    println!("controller_test(): Attempting to connect to controller...");
    
    match UnixStream::connect(SOCKET_PATH) {
        Ok(mut stream) => {
            println!("test.rs: Connected to controller.");
            
            let message = "Hello from server";
            stream.write_all(message.as_bytes()).expect("Failed to send message");
            println!("server sent message: {}", message);
            
            let mut buffer = [0; 256];
            let bytes_read = stream.read(&mut buffer).expect("Failed to read response");
            let response = String::from_utf8_lossy(&buffer[..bytes_read]);
            println!("server recv response: {}", response);
        }
        Err(err) => {
            eprintln!("test.rs: Failed to connect: {}", err);
        }
    }
    
    thread::sleep(Duration::from_secs(1));
}