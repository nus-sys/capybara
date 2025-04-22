// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::MacAddress;
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};

use std::{collections::{HashMap, HashSet, hash_map::Entry, VecDeque}, time::Instant, net::Ipv4Addr};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use ctrlc;

#[cfg(feature = "tcp-migration")]
use demikernel::demikernel::bindings::demi_print_queue_length_log;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

#[macro_use]
extern crate lazy_static;
//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {{
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" {
                eprintln!("\x1B[32m{}\x1B[0m", format_args!($($arg)*));
            }
        }
    }};
}

macro_rules! server_log_mig {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" || val == "mig" {
                eprintln!("\x1B[33m{}\x1B[0m", format_args!($($arg)*));
            }
        }
    };
}

//=====================================================================================

const ROOT: &str = "/var/www/demo";
const BUFSZ: usize = 1024 * 256; // 128KB

static mut START_TIME: Option<Instant> = None;

//=====================================================================================

// Borrowed from Loadgen
struct AppBuffer {
    buf: Vec<u8>,
    head: usize,
    tail: usize,
}

impl AppBuffer {
    pub fn new() -> AppBuffer {
        AppBuffer {
            buf: vec![0; BUFSZ],
            head: 0,
            tail: 0,
        }
    }

    pub fn flush(&mut self) {
        self.head = 0;
        self.tail = 0;
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


fn response_size() -> usize {
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
                    let extra_bytes = "A".repeat((*N).saturating_sub(contents.len()));
                    let full_contents = format!("{}{}", contents, extra_bytes);
                    server_log!("full_contents len: {}", full_contents.len());
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                        full_contents.len(),
                        full_contents
                    )
                }
                Err(_) => {
                    format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
                }
            }
        };
    }

    RESPONSE.len()
}
//======================================================================================================================
// server()
//======================================================================================================================


struct ConnectionState {
    buffer: AppBuffer,
    is_backend: bool,
    client_qds: VecDeque<QDesc>,
}

fn server(local: SocketAddrV4) -> Result<()> {
    ctrlc::set_handler(move || {
        LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    let switch_addr: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 7), 5000);
    const FE_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]);
    let RESPONSE_SIZE = response_size();

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();
    let mut backends: VecDeque<QDesc> = VecDeque::new();

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    server_log!("LibOS initialised");

    // Listening socket.
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");
    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");
    qts.push(libos.accept(sockqd).expect("accept"));
    server_log!("Bound TCP and waiting for packets");

    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);

    let mut tmpbuf = vec![0u8; 1024];
    
    loop {
        let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices, None).expect("result");
            
        server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");

        let results = &qrs[..result_count];
        let indices = &indices[..result_count];

        for (index, (qd, result)) in indices.iter().zip(results.iter()).rev() {
            let (index, qd) = (*index, *qd);
            qts.swap_remove(index);

            match result {
                OperationResult::Accept(new_qd) => {
                    // This could be a client or a BE, so we wait for first pop that tells us if it's a BE.

                    let new_qd = *new_qd;
                    server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);

                    // Pop from new_qd
                    qts.push(libos.pop(new_qd).expect("pop qtoken"));

                    connstate.insert(new_qd, ConnectionState {
                        buffer: AppBuffer::new(),
                        is_backend: false,
                        client_qds: VecDeque::new(),
                    });

                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },

                OperationResult::Push => {
                    server_log!("PUSH complete");
                },

                // TCP packet popped.
                OperationResult::Pop(_, buf) => {
                    server_log!("POP complete");

                    // Re-arm POP.
                    qts.push(libos.pop(qd).expect("pop qtoken"));

                    // New BE.
                    if buf.as_ref() == &[0xCA, 0xFE, 0xDE, 0xAD] {
                        server_log_mig!("New BE found");
                        backends.push_back(qd);
                        connstate.get_mut(&qd).unwrap().is_backend = true;
                        continue;
                    }

                    // Response from BE, forward to client.
                    // if connstate.get(&qd).unwrap().is_backend {
                    //     // let client_qd = i32::from_be_bytes(buf[0..4].try_into().unwrap());
                    //     let buf = &buf[0..];
                    //     connstate.get_mut(&qd).unwrap().buffer
                    //     qts.push(libos.push2(client_qd.into(), buf).unwrap());
                    //     server_log!("Sending response to client ({client_qd}): {}", std::str::from_utf8(buf).unwrap());
                    //     continue;
                    // }
                    if connstate.get(&qd).unwrap().is_backend {
                        let state = connstate.get_mut(&qd).unwrap();
                        let app_buf = state.buffer.get_empty_buf();
                        app_buf[0..buf.len()].copy_from_slice(&buf[..]);
                        state.buffer.push_data(buf.len());
                        server_log!("Buffered {} bytes, now total {} bytes buffered, waiting for {} bytes", buf.len(),state.buffer.data_size(), RESPONSE_SIZE);
                        if state.buffer.data_size() == RESPONSE_SIZE {
                            server_log!("Buffered {} bytes, sending to client", state.buffer.data_size());
                            
                            let client_qd = state.client_qds.pop_front().unwrap();
                            qts.push(libos.push2(client_qd.into(), state.buffer.get_data()).unwrap());
                            server_log!("Sending response to client ({:?}): {}", client_qd, std::str::from_utf8(&state.buffer.get_data()[..300]).unwrap());
                            state.buffer.flush();
                        }
                        else if state.buffer.data_size() > RESPONSE_SIZE{
                            panic!("Buffered {} bytes, but expected {} bytes", state.buffer.data_size(), RESPONSE_SIZE);
                        }
                        continue;   
                    }





                    // New request from client, forward to a backend.
                    let be_qd = backends.pop_front().unwrap();
                    backends.push_back(be_qd);
                    connstate.get_mut(&be_qd).unwrap().client_qds.push_back(qd);
                    qts.push(libos.push2(be_qd, &buf).unwrap());
                    server_log!("Sending request to BE ({be_qd:?}): {}", std::str::from_utf8(buf).unwrap());
                },

                OperationResult::Failed(e) => {
                    match e.errno {
                        _ => panic!("operation failed: {}", e),
                    }
                }

                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        }
        server_log!("******* APP: Okay, handled the results! *******");
    }
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
        assert_eq!(sockaddr.port(), 10000, "FE must be on port 10000");
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
