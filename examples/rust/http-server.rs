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

use std::{collections::{HashMap, HashSet, hash_map::Entry}, time::Instant};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;
use std::sync::Arc;

#[cfg(feature = "tcp-migration")]
use demikernel::demikernel::bindings::demi_print_queue_length_log;

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


    lazy_static! {
        static ref RESPONSE: String = {
            match std::fs::read_to_string("/var/www/demo/index.html") {
                Ok(contents) => {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents)
                },
                Err(_) => {
                    format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
                },
            }
        };
    }
    
    server_log!("PUSH: {}", RESPONSE.lines().next().unwrap_or(""));
    libos.push2(qd, RESPONSE.as_bytes()).expect("push success")
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


struct ConnectionState {
    pushing: usize,
    buffer: Buffer,
    is_popping: bool,
}

fn server(local: SocketAddrV4) -> Result<()> {
    let mut request_count = 0;
    let mut queue_length_vec: Vec<(usize, usize)> = Vec::new();
    let migration_per_n: i32 = env::var("MIG_PER_N")
            .unwrap_or(String::from("0")) // Default value is 0 if MIG_PER_N is not set
            .parse()
            .expect("MIG_PER_N must be a i32");
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        //r.store(false, Ordering::SeqCst);
        // println!("Received Ctrl-C signal. Total requests processed: {}", request_count);
        LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    // unsafe { START_TIME = Some(Instant::now()); }

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    #[cfg(feature = "mig-per-n-req")]
    let mut requests_remaining: HashMap<QDesc, i32> = HashMap::new();

    // Create qrs filled with garbage.
    let mut qrs: Vec<(usize, QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0, 0.into(), OperationResult::Connect));
    
    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        /* let result = libos.trywait_any_one2(&qts).expect("result");
        #[cfg(feature = "mig-per-n-req")]
        let mut qds_to_migrate = HashSet::new();
        if let Some((index, qd, result)) = result {
            server_log!("\n\n======= OS: one I/O operation have been completed, take result! =======");        
            qts.swap_remove(index);
            match result {
                OperationResult::Accept(new_qd) => {
                    server_log!("ACCEPT complete ==> issue POP and ACCEPT");

                    let buffer = {
                        let mut buffer = Buffer::new();

                        #[cfg(feature = "tcp-migration")]
                        if let Some(data) = libos.take_migrated_data(new_qd).expect("take_migrated_data failed") {
                            server_log!("Received migrated data ({} bytes)", data.len());

                            buffer.get_empty_buf()[..data.len()].copy_from_slice(&data);
                            buffer.push_data(data.len());
                        }

                        buffer
                    };

                    connstate.insert(new_qd, ConnectionState {
                        pushing: 0,
                        buffer,
                    });

                    // Pop from new_qd
                    match libos.pop(new_qd) {
                        Ok(pop_qt) => qts.push(pop_qt),
                        #[cfg(feature = "tcp-migration")]
                        Err(e) if e.errno == demikernel::ETCPMIG => (),
                        Err(e) => panic!("pop qt: {}", e),
                    }

                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },

                OperationResult::Push => {
                    connstate.get_mut(&qd).unwrap().pushing -= 1;
                    
                    // #[cfg(feature = "tcp-migration")]
                    // libos.pushed_response();

                    server_log!("PUSH complete ==> {} pushes are pending", connstate.get_mut(&qd).unwrap().pushing);
                    
                    #[cfg(feature = "mig-per-n-req")]
                    if migration_per_n > 0  && !requests_remaining.contains_key(&qd) {
                        // Server has been processed N requests from this qd 
                        qds_to_migrate.insert(qd);
                    }
                },

                OperationResult::Pop(_, recvbuf) => {
                    server_log!("POP complete");

                    let mut state = connstate.get_mut(&qd).unwrap();
                    let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                    state.pushing += sent;

                    server_log!("Issued PUSH => {} pushes pending", state.pushing);
                    
                    
                    #[cfg(feature = "mig-per-n-req")] {
                        let remaining = requests_remaining.entry(qd).or_insert(migration_per_n);
                        *remaining -= 1;
                        if *remaining > 0 {
                            // queue next pop
                            match libos.pop(qd) {
                                Ok(qt) => {
                                    qts.push(qt);
                                    server_log!("Issued POP");
                                },
                                Err(e) if e.errno == demikernel::ETCPMIG => (),
                                Err(e) => panic!("pop qt: {}", e),
                            }
                        } else{
                            server_log!("Should be migrated (no POP issued)");
                            requests_remaining.remove(&qd).unwrap();
                        }
                    }
                    #[cfg(not(feature = "mig-per-n-req"))]{
                        // queue next pop
                        match libos.pop(qd) {
                            Ok(qt) => {
                                qts.push(qt);
                                server_log!("Issued POP");
                            },
                            #[cfg(feature = "tcp-migration")]
                            Err(e) if e.errno == demikernel::ETCPMIG => (),
                            Err(e) => panic!("pop qt: {}", e),
                        }
                    }
                },

                OperationResult::Failed(e) => {
                    server_log!("operation failed: {}", e);
                }

                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        } */

        let results = libos.wait_any2(&qts, &mut qrs).expect("result");

        /* #[cfg(feature = "tcp-migration")]
        {
            let qds_without_results = {
                let mut qds = connstate.keys().map(|e| *e).collect::<HashSet<_>>();
                if let Some(result) = result.as_ref() {
                    for (_, qd, _) in result {
                        qds.remove(qd);
                    }
                }
                qds
            };
    
            for qd in qds_without_results {
                let state = connstate.get(&qd).unwrap();
                if state.is_popping || state.pushing > 0 {
                    continue;
                }
    
                let data = state.buffer.get_data();
                let data = if data.is_empty() { None } else { Some(data) };
                match libos.notify_migration_safety(qd, data) {
                    Ok(true) => { connstate.remove(&qd); },
                    Err(e) => panic!("notify_migration_safety() failed: {:?}", e.cause),
                    _ => (),
                };
            }
        } */

        /* if let Some(completed_results) = result */ {
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
            /* let indices_to_remove: Vec<usize> = completed_results.iter().map(|(index, _, _)| *index).collect();
            let new_qts: Vec<QToken> = qts.iter().enumerate().filter(|(i, _)| !indices_to_remove.contains(i)).map(|(_, qt)| *qt).collect(); //HERE!
            qts = new_qts; */

            let results = &qrs[..results];

            for (index, qd, result) in results.iter().rev() {
                let (index, qd) = (*index, *qd);
                qts.swap_remove(index);

                #[cfg(feature = "mig-per-n-req")]
                let mut should_migrate_this_qd = false;

                match result {
                    OperationResult::Accept(new_qd) => {
                        let new_qd = *new_qd;
                        server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);

                        let buffer = {
                            let mut buffer = Buffer::new();

                            #[cfg(feature = "tcp-migration")]
                            if let Some(data) = libos.take_migrated_data(new_qd).expect("take_migrated_data failed") {
                                server_log!("Received migrated data ({} bytes)", data.len());

                                buffer.get_empty_buf()[..data.len()].copy_from_slice(&data);
                                buffer.push_data(data.len());
                            }

                            buffer
                        };

                        let mut state = ConnectionState {
                            pushing: 0,
                            buffer,
                            is_popping: false,
                        };

                        // Pop from new_qd
                        /* comment out this for recv_queue_len vs mig_lat eval */
                        match libos.pop(new_qd) {
                            Ok(pop_qt) => {
                                state.is_popping = true;
                                qts.push(pop_qt)
                            },
                            #[cfg(feature = "tcp-migration")]
                            Err(e) if e.errno == demikernel::ETCPMIG => (),
                            Err(e) => panic!("pop qt: {}", e),
                        }
                        /* comment out this for recv_queue_len vs mig_lat eval */

                        connstate.insert(new_qd, state);
                        
                        #[cfg(feature = "mig-per-n-req")]
                        assert!(requests_remaining.insert(new_qd, migration_per_n).is_none());
    
                        // Re-arm accept
                        qts.push(libos.accept(qd).expect("accept qtoken"));
                    },

                    OperationResult::Push => {
                        /* let connstate = connstate.get_mut(&qd).unwrap();
                        connstate.pushing -= 1; */
                        
                        // #[cfg(feature = "tcp-migration")]
                        // libos.pushed_response();

                        //server_log!("PUSH complete ==> {} pushes are pending", connstate.pushing);
                        server_log!("PUSH complete");
                        
                        /* #[cfg(feature = "mig-per-n-req")]
                        if migration_per_n > 0  && !requests_remaining.contains_key(&qd) {
                            // Server has been processed N requests from this qd 
                            should_migrate_this_qd = true;
                        } */
                    },

                    OperationResult::Pop(_, recvbuf) => {
                        // pop_count -= 1;
                        server_log!("POP complete");

                        let mut state = connstate.get_mut(&qd).unwrap();
                        //state.is_popping = false;
                        let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                        //state.pushing += sent;

                        //server_log!("Issued PUSH => {} pushes pending", state.pushing);
                        server_log!("Issued PUSH");
                        
                        
                        #[cfg(feature = "mig-per-n-req")]
                        if let Entry::Occupied(mut entry) = requests_remaining.entry(qd) {
                            let remaining = entry.get_mut();
                            *remaining -= 1;
                            if *remaining > 0 {
                                // queue next pop
                                qts.push(libos.pop(qd).expect("pop qt"));
                                server_log!("Issued POP");
                            } else {
                                server_log!("Should be migrated (no POP issued)");
                                libos.initiate_migration(qd).unwrap();
                                entry.remove();


                                /* NON-CONCURRENT MIGRATION */
                                /* match libos.initiate_migration(qd) {
                                    Ok(()) => {
                                        entry.remove();
                                    },
                                    Err(e) => {
                                        // eprintln!("this conn is not for migration");
                                        qts.push(libos.pop(qd).expect("pop qt"));
                                        server_log!("Issued POP");
                                    },
                                } */
                                /* NON-CONCURRENT MIGRATION */
                            }
                        }
                        #[cfg(not(feature = "mig-per-n-req"))]
                        {
                            // queue next pop
                            qts.push(libos.pop(qd).expect("pop qt"));
                            server_log!("Issued POP");
                        }
                    },

                    OperationResult::Failed(e) => {
                        match e.errno {
                            #[cfg(feature = "tcp-migration")]
                            demikernel::ETCPMIG => eprintln!("migrated {:?} polled", qd),
                            _ => panic!("operation failed: {}", e),
                        }
                    }

                    _ => {
                        panic!("Unexpected op: RESULT: {:?}", result);
                    },
                }

                // Notify for this QD if no responses pending.
                /* #[cfg(feature = "tcp-migration")]
                {
                    let state = connstate.get(&qd).unwrap();
                    // Can't migrate a connection with outstanding TX.
                    if state.is_popping || state.pushing > 0 {
                        continue;
                    }

                    #[cfg(feature = "mig-per-n-req")]
                    if should_migrate_this_qd {
                        libos.initiate_migration(qd).unwrap();
                    }

                    let data = {
                        let data = state.buffer.get_data();
                        if data.is_empty() {
                            None
                        } else {
                            Some(data)
                        }
                    };

                    match libos.notify_migration_safety(qd, data) {
                        Ok(true) => { connstate.remove(&qd); },
                        Err(e) => panic!("notify_migration_safety() failed: {:?}", e.cause),
                        _ => (),
                    };
                } */
            }
            server_log!("******* APP: Okay, handled the results! *******");
        }
    }

    eprintln!("server stopping");

    // loop {}

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
