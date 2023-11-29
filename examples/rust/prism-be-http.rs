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

use std::{collections::{HashMap, HashSet, hash_map::Entry}, time::{Instant, Duration}};
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
    buffer: Buffer,
    has_sent: bool
}

fn server(local: SocketAddrV4, fe: SocketAddrV4) -> Result<()> {
    ctrlc::set_handler(move || {
        LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    server_log!("LibOS initialised");

    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    // You don't need a listening socket for backend?
    /* libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");
    qts.push(libos.accept(sockqd).expect("accept")); */

    // Connect to FE.
    let qt = libos.connect(sockqd, fe).expect("connect");
    let (_, res) = libos.wait2(qt).unwrap();
    match res {
        OperationResult::Connect => (),
        e => panic!("Connection to FE failed: {:?}", e),
    }
    server_log!("Connected to FE");

    // Send magic values to inform FE that this is a BE.
    let qt = libos.push2(sockqd, &[0xCA, 0xFE, 0xDE, 0xAD]).unwrap();
    match libos.wait2(qt).unwrap().1 {
        OperationResult::Push => (),
        _ => panic!("error"),
    }
    server_log!("Sent magic values to FE");

    qts.push(libos.pop(sockqd).unwrap()); // wait for first connection from FE

    // Switch socket.
    let switch_qd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).expect("created socket");
    libos.bind(switch_qd, local).expect("bind socket");
    server_log!("Bound UDP");

    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);
    
    loop {
        let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices).expect("result");
            
        server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");

        let results = &qrs[..result_count];
        let indices = &indices[..result_count];

        for (index, (qd, result)) in indices.iter().zip(results.iter()).rev() {
            let (index, qd) = (*index, *qd);
            qts.swap_remove(index);

            match result {
                OperationResult::Push => {
                    server_log!("PUSH complete");
                },

                OperationResult::Pop(_, recvbuf) => {
                    server_log!("POP complete");

                    let mut state = connstate.get_mut(&qd).unwrap();

                    if state.has_sent {
                        server_log!("Migrating connection back to FE: {:?}", qd);
                        // TODO: Migrate back
                    } else {
                        // Send one response.
                        let sent = push_data_and_run(&mut libos, qd, &mut state.buffer, &recvbuf, &mut qts);
                        server_log!("Issued PUSH");
                        state.has_sent = true;
                        qts.push(libos.pop(qd).expect("pop qt"));
                    }
                },

                OperationResult::Failed(e) => {
                    match e.errno {
                        #[cfg(feature = "tcp-migration")]
                        demikernel::ETCPMIG => panic!("Should never poll after migrating"),
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
    #[cfg(not(feature = "tcp-migration"))]
    compile_error!("Prism needs TCP migration");
    
    server_log!("*** HTTP SERVER LOGGING IS ON ***");
    // logging::initialize();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let local: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        let fe: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
        assert_ne!(local.port(), 10000, "BE cannot be on port 10000");
        return server(local, fe);
    }

    usage(&args[0]);

    Ok(())
}
