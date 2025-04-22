// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use num_traits::ToBytes;

use std::{cell::RefCell, collections::{HashMap, HashSet, hash_map::Entry}, rc::Rc, time::{Duration, Instant}};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use ctrlc;

#[cfg(feature = "tcp-migration")]
use demikernel::{
    inetstack::protocols::tcpmig::ApplicationState,
    demikernel::bindings::demi_print_queue_length_log,
};

use ::demikernel::capy_time_log;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

#[macro_use]
extern crate lazy_static;
//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" {
                eprintln!("{}", format!($($arg)*).green());
            }
        }
    };
}

//=====================================================================================

const ROOT: &str = "/var/www/demo";
const BUFSZ: usize = 4096;

static mut START_TIME: Option<Instant> = None;
static mut SERVER_PORT: u16 = 0;

//=====================================================================================

// Borrowed from Loadgen
struct Buffer {
    buf: Vec<u8>,
    head: usize,
    tail: usize,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            buf: vec![0; BUFSZ],
            head: 0,
            tail: 0,
        }
    }

    pub fn data_size(&self) -> usize {
        self.head - self.tail
    }

    pub fn get_data(&self) -> &[u8] {
        &self.buf[self.tail..self.head]
    }

    pub fn push_data(&mut self, size: usize) {
        self.head += size;
        assert!(self.head <= self.buf.len());
    }

    pub fn pull_data(&mut self, size: usize) {
        assert!(size <= self.data_size());
        self.tail += size;
    }

    pub fn get_empty_buf(&mut self) -> &mut [u8] {
        &mut self.buf[self.head..]
    }

    pub fn try_shrink(&mut self) -> Result<()> {
        if self.data_size() == 0 {
            self.head = 0;
            self.tail = 0;
            return Ok(());
        }

        if self.head < self.buf.len() {
            return Ok(());
        }

        if self.data_size() == self.buf.len() {
            panic!("Need larger buffer for HTTP messages");
        }

        self.buf.copy_within(self.tail..self.head, 0);
        self.head = self.data_size();
        self.tail = 0;
        Ok(())
    }
}

#[cfg(feature = "tcp-migration")]
impl ApplicationState for Buffer {
    fn serialized_size(&self) -> usize {
        4 + self.data_size()
    }

    fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&(self.data_size() as u32).to_be_bytes());
        buf[4..4 + self.data_size()].copy_from_slice(self.get_data());
    }

    fn deserialize(buf: &[u8]) -> Self where Self: Sized {
        let mut buffer = Buffer::new();
        let data_size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        buffer.get_empty_buf()[0..data_size].copy_from_slice(&buf[4..4 + data_size]);
        buffer.push_data(data_size);
        buffer
    }
}

//======================================================================================================================
// server()
//======================================================================================================================

struct SessionData {
    data: Vec<u8>,
}

impl SessionData {
    fn new(size: usize) -> Self {
        Self { data: vec![5; size] }
    }
}

struct ConnectionState {
    buffer: Buffer,
    session_data: SessionData,
}

#[cfg(feature = "tcp-migration")]
impl ApplicationState for SessionData {
    fn serialized_size(&self) -> usize {
        4 + self.data.len()
    }

    fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&(self.data.len() as u32).to_be_bytes());
        buf[4..4 + self.data.len()].copy_from_slice(&self.data);
    }

    fn deserialize(buf: &[u8]) -> Self where Self: Sized {
        let size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        let mut data = Vec::with_capacity(size);
        data.extend_from_slice(&buf[4..4 + size]);
        Self { data }
    }
}

#[cfg(feature = "tcp-migration")]
impl ApplicationState for ConnectionState {
    fn serialized_size(&self) -> usize {
        self.buffer.serialized_size() + self.session_data.serialized_size()
    }

    fn serialize(&self, buf: &mut [u8]) {
        self.buffer.serialize(&mut buf[0..self.buffer.serialized_size()]);
        self.session_data.serialize(&mut buf[self.buffer.serialized_size()..])
    }

    fn deserialize(buf: &[u8]) -> Self where Self: Sized {
        let buffer = Buffer::deserialize(buf);
        let session_data = SessionData::deserialize(&buf[buffer.serialized_size()..]);
        Self { buffer, session_data }
    }
}

fn server(local: SocketAddrV4) -> Result<()> {
    #[cfg(feature = "tcp-migration")]{
        eprintln!("TCP MIGRATION ENABLED");
    }
    #[cfg(not(feature = "tcp-migration"))]{
        eprintln!("TCP MIGRATION DISABLED");
    }
    
    unsafe{ SERVER_PORT = local.port() };
    let mut request_count = 0;
    let mut queue_length_vec: Vec<(usize, usize)> = Vec::new();
    let migration_per_n: u64 = env::var("MIG_PER_N")
            .unwrap_or(String::from("10")) // Default value is 10 if MIG_PER_N is not set
            .parse()
            .expect("MIG_PER_N must be a u64");
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        // LibOS::dpdk_print_eth_stats();
        // LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    // unsafe { START_TIME = Some(Instant::now()); }

    let session_data_size: usize = std::env::var("SESSION_DATA_SIZE").map_or(0, |v| v.parse().unwrap());

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, Rc<RefCell<ConnectionState>>> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    let mut requests_remaining: HashMap<QDesc, u64> = HashMap::new();
    let mut last_migration_time = Instant::now();
    
    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);
    
    loop {
        let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices, None).expect("result");

        /* let mut pop_count = completed_results.iter().filter(|(_, _, result)| {
            matches!(result, OperationResult::Pop(_, _))
        }).count(); */
        /* #[cfg(feature = "tcp-migration")]{
            request_count += 1;
            if request_count % 1 == 0 {
                eprintln!("request_counnt: {} {}", request_count, libos.global_recv_queue_length());
            queue_length_vec.push((request_count, libos.global_recv_queue_length()));
            }
        } */
            
        server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");

        let results = &qrs[..result_count];
        let indices = &indices[..result_count];

        for (index, (qd, result)) in indices.iter().zip(results.iter()).rev() {
            let (index, qd) = (*index, *qd);
            qts.swap_remove(index);

            #[cfg(feature = "manual-tcp-migration")]
            let mut should_migrate_this_qd = false;

            match result {
                OperationResult::Accept(new_qd) => {
                    let new_qd = *new_qd;
                    server_log!("ACCEPT complete {:?}", new_qd);
                    
                    /* COMMENT OUT THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                    #[cfg(feature = "tcp-migration")]
                    let state = if let Some(data) = libos.get_migrated_application_state::<ConnectionState>(new_qd) {
                        server_log!("Connection State LOG: Received migrated app data ({} bytes)", data.borrow().serialized_size() - 8);
                        data
                    } else {
                        server_log!("Connection State LOG: No migrated app data, creating new ConnectionState");
                        let state = Rc::new(RefCell::new(ConnectionState {
                            buffer: Buffer::new(),
                            session_data: SessionData::new(session_data_size),
                        }));
                        libos.register_application_state(new_qd, state.clone());
                        state
                    };

                    #[cfg(not(feature = "tcp-migration"))]
                    let state = Rc::new(RefCell::new(ConnectionState {
                        buffer: Buffer::new(),
                        session_data: SessionData::new(session_data_size),
                    }));
                    // capy_time_log!("APP_STATE_REGISTERED,(n/a)");
                    
                    
                    connstate.insert(new_qd, state);
                    /* COMMENT OUT THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                    server_log!("Migrate {:?}", new_qd);
                    libos.initiate_migration(new_qd).unwrap();

                    /* ACTIVATE THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                    // static mut NUM_MIG: u32 = 0;
                    // unsafe{ NUM_MIG += 1; }
                    // if unsafe{ NUM_MIG } < 11000 {
                    //     libos.initiate_migration(new_qd).unwrap();
                    // }
                    // qts.push(libos.pop(new_qd).unwrap());
                    /* ACTIVATE THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                    
                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },

                OperationResult::Push => {
                    panic!("PUSH complete");
                },

                OperationResult::Pop(_, recvbuf) => {
                    panic!("POP complete");
                },

                OperationResult::Failed(e) => {
                    match e.errno {
                        #[cfg(feature = "tcp-migration")]
                        demikernel::ETCPMIG => { server_log!("migrated {:?} polled", qd) },
                        _ => panic!("operation failed: {}", e),
                    }
                }

                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        }
        // #[cfg(feature = "profiler")]
        // profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
        
        server_log!("******* APP: Okay, handled the results! *******");
    }

    eprintln!("server stopping");

    /* #[cfg(feature = "tcp-migration")]
    // Get the length of the vector
    let vec_len = queue_length_vec.len();

    // Calculate the starting index for the last 10,000 elements
    let start_index = if vec_len >= 5_000 {
        vec_len - 5_000
    } else {
        0 // If the vector has fewer than 10,000 elements, start from the beginning
    };

    // Create a slice of the last 10,000 elements
    let last_10_000 = &queue_length_vec[start_index..];

    // Iterate over the slice and print the elements
    let mut cnt = 0;
    for (idx, qlen) in last_10_000.iter() {
        println!("{},{}", cnt, qlen);
        cnt+=1;
    } */
    #[cfg(feature = "tcp-migration")]
    demi_print_queue_length_log();

    LibOS::capylog_dump(&mut std::io::stderr().lock());

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &String) {
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    server_log!("*** HTTP SERVER LOGGING IS ON ***");
    // logging::initialize();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
