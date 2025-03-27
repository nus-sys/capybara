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

fn respond_to_request(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    /* let data_str = std::str::from_utf8(data).unwrap();
    let data_str = String::from_utf8_lossy(data);
    let data_str = unsafe { std::str::from_utf8_unchecked(&data[..]) };

    let mut file_name = data_str
            .split_whitespace()
            .nth(1)
            .and_then(|file_path| {
                let mut path_parts = file_path.split('/');
                path_parts.next().and_then(|_| path_parts.next())
            })
            .unwrap_or("index.html");
    if file_name == "" {
        file_name = "index.html";
    }
    let full_path = format!("{}/{}", ROOT, file_name);
    
    let response = match std::fs::read_to_string(full_path.as_str()) {
        Ok(mut contents) => {
            // contents.push_str(unsafe { START_TIME.as_ref().unwrap().elapsed() }.as_nanos().to_string().as_str());
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents)
        },
        Err(_) => format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n"),
    }; 

    server_log!("PUSH: {}", response.lines().next().unwrap_or(""));
    libos.push2(qd, response.as_bytes()).expect("push success") */    

    #[cfg(not(feature = "server-reply-analysis"))]
    {
        lazy_static! {
            static ref N: usize = {
                env::var("DATA_SIZE")
                    .unwrap_or_else(|_| "0".to_string()) // Fallback to 0 if not set
                    .parse()
                    .expect("DATA_SIZE must be a valid number")
            };
            static ref RESPONSE: String = {
                let file_path = format!("{}/{}", ROOT, "index.html");
                match std::fs::read_to_string(file_path) {
                    Ok(contents) => {
                        let extra_bytes = "A".repeat(*N - contents.len());
                        let full_contents = format!("{}{}", contents, extra_bytes);
                        // server_log!("full_contents len: {}", full_contents.len());
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", full_contents.len(), full_contents)
                    },
                    Err(_) => {
                        format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
                    },
                }
            };
        }
        
        // Inho: this is to simulate request processing delay for some clients
        /* if libos.get_remote_port(qd) == 1124 {
            // eprintln!("qd: {:?}", qd);
            std::thread::sleep(std::time::Duration::from_micros(20));
        } */

        server_log!("PUSH: {}", RESPONSE.lines().next().unwrap_or(""));
        libos.push2(qd, RESPONSE.as_bytes()).expect("push success")
    }
    
    #[cfg(feature = "server-reply-analysis")]
    {
        const PREFILLED_METADATA_SIZE: usize = 16;
        const METADATA_SIZE: usize = PREFILLED_METADATA_SIZE + 4;

        static mut RESPONSE: Option<Vec<u8>> = None;
        let response = if let Some(response) = unsafe { RESPONSE.as_mut() } {
            response.as_mut_slice()
        } else {
            let msg = match std::fs::read_to_string("/var/www/demo/index.html") {
                Ok(contents) => {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents)
                },
                Err(_) => {
                    format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
                },
            };
            let mut buf = vec![0u8; METADATA_SIZE + msg.len()];
            buf[METADATA_SIZE..].copy_from_slice(msg.as_bytes());
            unsafe { 
                RESPONSE = Some(buf);
                RESPONSE.as_mut().unwrap().as_mut_slice()
            }
        };
        
        // Assume request has been overwritten with statistics.
        if data.len() >= 16 {
            response[0..PREFILLED_METADATA_SIZE].copy_from_slice(&data[0..PREFILLED_METADATA_SIZE]);
            
            // Add delta.
            let now: u64 = chrono::Local::now().timestamp_nanos().try_into().expect("timestamp is negative");
            let then = u64::from_be_bytes(data[8..16].try_into().unwrap());
            let delta: u32 = now.checked_sub(then).unwrap_or(0).try_into().expect("delta > 4.29 seconds");
            response[PREFILLED_METADATA_SIZE..METADATA_SIZE].copy_from_slice(&delta.to_be_bytes());
        
            server_log!("PUSH: ({}) {}", u32::from_be_bytes(data[4..8].try_into().unwrap()), std::str::from_utf8(&response[METADATA_SIZE..]).unwrap());
        }
        libos.push2(qd, &response).expect("push success")
    }
}

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {
    
    server_log!("buffer.data_size() {}", buffer.data_size());
    // fast path: no previous data in the stream and this request contains exactly one HTTP request
    if buffer.data_size() == 0 {
        if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
            server_log!("responding 1");
            let resp_qt = respond_to_request(libos, qd, data);
            qts.push(resp_qt);
            return 1;
        }
    }
    // println!("* CHECK *\n");
    // Copy new data into buffer
    buffer.get_empty_buf()[..data.len()].copy_from_slice(data);
    buffer.push_data(data.len());
    server_log!("buffer.data_size() {}", buffer.data_size());
    let mut sent = 0;

    loop {
        let dbuf = buffer.get_data();
        match find_subsequence(dbuf, b"\r\n\r\n") {
            Some(idx) => {
                server_log!("responding 2");
                let resp_qt = respond_to_request(libos, qd, &dbuf[..idx + 4]);
                qts.push(resp_qt);
                buffer.pull_data(idx + 4);
                buffer.try_shrink().unwrap();
                sent += 1;
            }
            None => {
                return sent;
            }
        }
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
                    server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);
                    last_migration_time = Instant::now();
                    
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
                    
                    // Pop from new_qd
                    match libos.pop(new_qd) {
                        Ok(pop_qt) => {
                            qts.push(pop_qt)
                        },
                        Err(e) => panic!("pop qt: {}", e),
                    }
                    #[cfg(feature = "manual-tcp-migration")]
                    assert!(requests_remaining.insert(new_qd, migration_per_n).is_none());
                    connstate.insert(new_qd, state);
                    /* COMMENT OUT THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */


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
                    server_log!("PUSH complete");
                },

                OperationResult::Pop(_, recvbuf) => {
                    server_log!("POP complete");

                    let mut state = connstate.get_mut(&qd).unwrap().borrow_mut();
                    
                    let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);

                    //server_log!("Issued PUSH => {} pushes pending", state.pushing);
                    server_log!("Issued {sent} PUSHes");

                    
                    #[cfg(feature = "manual-tcp-migration")]{
                        let elapsed = last_migration_time.elapsed();

                        if elapsed >= Duration::from_micros(migration_per_n) {
                            server_log!("Should be migrated (no POP issued)");
                            server_log!("BUFFER DATA SIZE = {}", state.buffer.data_size());
                            libos.initiate_migration(qd).unwrap();
                            
                            // Reset migration timer
                            last_migration_time = Instant::now();
                        } else {
                            server_log!(
                                "Skipping migration: Only {:?} elapsed since last migration",
                                elapsed
                            );
                            qts.push(libos.pop(qd).expect("pop qt"));
                            server_log!("Issued POP instead of migration");
                        }
                        // if let Entry::Occupied(mut entry) = requests_remaining.entry(qd) {
                        //     let remaining = entry.get_mut();
                        //     *remaining -= 1;
                        //     if *remaining > 0 {
                        //         // queue next pop
                        //         qts.push(libos.pop(qd).expect("pop qt"));
                        //         server_log!("Migrating after {} more requests, Issued POP", remaining);
                        //     } else {
                        //         server_log!("Should be migrated (no POP issued)");
                        //         server_log!("BUFFER DATA SIZE = {}", state.buffer.data_size());
                        //         libos.initiate_migration(qd).unwrap();
                        //         entry.remove();
                        //         /* NON-CONCURRENT MIGRATION */
                        //         /* match libos.initiate_migration(qd) {
                        //             Ok(()) => {
                        //                 entry.remove();
                        //             },
                        //             Err(e) => {
                        //                 // eprintln!("this conn is not for migration");
                        //                 qts.push(libos.pop(qd).expect("pop qt"));
                        //                 server_log!("Issued POP");
                        //             },
                        //         } */
                        //         /* NON-CONCURRENT MIGRATION */
                        //     }
                        // }
                    }
                    #[cfg(not(feature = "manual-tcp-migration"))]
                    {
                        // queue next pop
                        qts.push(libos.pop(qd).expect("pop qt"));
                        server_log!("Issued POP");
                    }
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
