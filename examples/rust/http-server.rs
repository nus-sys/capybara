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

use std::{collections::{HashMap, HashSet}, time::Instant};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;
use std::sync::Arc;
use demikernel::demikernel::bindings::demi_print_queue_length_log;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        eprintln!("{}", format_args!($($arg)*));
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
    // let data_str = std::str::from_utf8(data).unwrap();
    // let data_str = String::from_utf8_lossy(data);
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

    libos.push2(qd, response.as_bytes()).expect("push success")
}

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {

    // fast path: no previous data in the stream and this request contains exactly one HTTP request
    if buffer.data_size() == 0 {
        if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
            let resp_qt = respond_to_request(libos, qd, data);
            qts.push(resp_qt);
            return 1;
        }
    }
    // println!("* CHECK *\n");
    // Copy new data into buffer
    buffer.get_empty_buf()[..data.len()].copy_from_slice(data);
    buffer.push_data(data.len());

    let mut sent = 0;

    loop {
        let dbuf = buffer.get_data();
        match find_subsequence(dbuf, b"\r\n\r\n") {
            Some(idx) => {
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
        r.store(false, Ordering::SeqCst);
        // println!("Received Ctrl-C signal. Total requests processed: {}", request_count);
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
    
    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        /* let result = libos.trywait_any_one2(&qts).expect("result");
        #[cfg(feature = "mig-per-n-req")]
        let mut qds_to_migrate = HashSet::new();
        if let Some((index, qd, result)) = result {
            qts.swap_remove(index);
            match result {
                OperationResult::Accept(new_qd) => {
                    // Pop from new_qd
                    let pop_qt = libos.pop(new_qd).expect("pop qt");
                    qts.push(pop_qt);
                    connstate.insert(new_qd, ConnectionState {
                        pushing: 0,
                        pop_qt: pop_qt,
                        buffer: Buffer::new(),
                    });

                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },
                OperationResult::Push => {
                    let mut state = connstate.get_mut(&qd).unwrap();
                    state.pushing -= 1;
                    // queue next pop
                    let pop_qt = libos.pop(qd).expect("pop qt");
                    qts.push(pop_qt);
                    state.pop_qt = pop_qt;
                },
                OperationResult::Pop(_, recvbuf) => {
                    let mut state = connstate.get_mut(&qd).unwrap();
                    let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                    state.pushing += sent;

                    #[cfg(feature = "tcp-migration")]{
                        request_count += sent;
                        if request_count % 100 == 0 {
                            // eprintln!("request_counnt: {} {}", request_count, libos.global_recv_queue_length());
                            queue_length_vec.push((request_count, libos.global_recv_queue_length()));
                        }
                    }
                },
                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        } */
        let result = libos.trywait_any2(&qts).expect("result");
        
        #[cfg(feature = "mig-per-n-req")]
        let mut qds_to_migrate = HashSet::new();

        if let Some(completed_results) = result {
            let mut pop_count = completed_results.iter().filter(|(_, _, result)| {
                matches!(result, OperationResult::Pop(_, _))
            }).count();
            /* #[cfg(feature = "tcp-migration")]{
                request_count += 1;
                if request_count % 1 == 0 {
                    eprintln!("request_counnt: {} {}", request_count, libos.global_recv_queue_length());
                queue_length_vec.push((request_count, libos.global_recv_queue_length()));
                }
            } */
            server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");
            let indices_to_remove: Vec<usize> = completed_results.iter().map(|(index, _, _)| *index).collect();
            /* server_log!("\n\n1, indicies_to_remove: {:?}", indices_to_remove); */
            let new_qts: Vec<QToken> = qts.iter().enumerate().filter(|(i, _)| !indices_to_remove.contains(i)).map(|(_, qt)| *qt).collect(); //HERE!
            qts = new_qts;
            for (index, qd, result) in completed_results {
                // qts.swap_remove(index);
    
                match result {
                    OperationResult::Accept(new_qd) => {
                        server_log!("ACCEPT complete ==> request POP and ACCEPT");

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
                        pop_count -= 1;
                        server_log!("POP complete ==> request PUSH and POP");

                        let mut state = connstate.get_mut(&qd).unwrap();
                        let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                        state.pushing += sent;
                        
                        
                        #[cfg(feature = "mig-per-n-req")] {
                            let remaining = requests_remaining.entry(qd).or_insert(migration_per_n);
                            *remaining -= 1;
                            if *remaining > 0 {
                                // queue next pop
                                match libos.pop(qd) {
                                    Ok(qt) => {
                                        qts.push(qt);
                                    },
                                    Err(e) if e.errno == demikernel::ETCPMIG => (),
                                    Err(e) => panic!("pop qt: {}", e),
                                }
                            } else{
                                requests_remaining.remove(&qd).unwrap();
                            }
                        }
                        #[cfg(not(feature = "mig-per-n-req"))]{
                            // queue next pop
                            match libos.pop(qd) {
                                Ok(qt) => {
                                    qts.push(qt);
                                    //state.pop_qt = qt;
                                },
                                Err(e) if e.errno == demikernel::ETCPMIG => (),
                                Err(e) => panic!("pop qt: {}", e),
                            }
                        }
                    },

                    _ => {
                        panic!("Unexpected op: RESULT: {:?}", result);
                    },
                }
            }
            server_log!("******* APP: Okay, handled the results! *******");
        }

        #[cfg(feature = "tcp-migration")]
        {
            #[cfg(feature = "mig-per-n-req")] {
                if migration_per_n > 0 {
                    for qd in qds_to_migrate.iter() {
                        // Can't migrate a connection with outstanding TX or partially processed HTTP requests in the TCP stream
                        if connstate.get_mut(&qd).unwrap().pushing == 0 && connstate.get_mut(&qd).unwrap().buffer.data_size() == 0 {
                            libos.initiate_migration(*qd);
                            connstate.remove(&qd);
                        }
                    }
                }
            }
            #[cfg(not(feature = "mig-per-n-req"))] {
                let mut qds_to_remove = Vec::new();
                for (&qd, state) in connstate.iter() {
                    // Can't migrate a connection with outstanding TX or partially processed HTTP requests in the TCP stream
                    if state.pushing > 0 {
                        continue;
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
                        Ok(true) => qds_to_remove.push(qd),
                        Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
                        _ => (),
                    };
                }
    
                for qd in qds_to_remove {
                    connstate.remove(&qd);
                }
            }
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
