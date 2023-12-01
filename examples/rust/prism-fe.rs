// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

mod prism;

use ::anyhow::Result;
use demikernel::{MacAddress, inetstack::protocols::tcp::peer::state::TcpState};
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

use crate::prism::PrismPacket;

use demikernel::runtime::memory::Buffer;

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
const BUFSZ: usize = 4096;

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

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut AppBuffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {
    
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
    buffer: AppBuffer,
}

struct Migration {
    qd: QDesc,
    request: Buffer,
}

fn server(local: SocketAddrV4) -> Result<()> {
    ctrlc::set_handler(move || {
        LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");

    let switch_addr: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 7), 5000);
    const FE_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]);
    
    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();
    let mut backends: VecDeque<(QDesc, SocketAddrV4)> = VecDeque::new();

    // Client addr -> QDesc.
    let mut migrations: HashMap<SocketAddrV4, Migration> = HashMap::new();

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    server_log!("LibOS initialised");

    // Listening socket.
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");
    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");
    qts.push(libos.accept(sockqd).expect("accept"));
    server_log!("Bound TCP and waiting for packets");

    // Switch socket.
    let switch_qd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_DGRAM, 0).expect("created socket");
    libos.bind(switch_qd, local).expect("bind socket");
    server_log!("Bound UDP");

    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);

    let mut prism_buf = [0u8; 64];
    let mut state_buf = vec![0u8; 4096];
    
    loop {
        let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices).expect("result");
            
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
                    });

                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },

                OperationResult::Push => {
                    server_log!("PUSH complete");
                },

                // TCP packet popped.
                OperationResult::Pop(None, buf) => {
                    server_log!("POP complete");

                    // New BE.
                    if buf.as_ref() == &[0xCA, 0xFE, 0xDE, 0xAD] {
                        server_log_mig!("New BE found");
                        backends.push_back((qd, libos.get_tcp_endpoints(qd).1));

                        // Re-arm POP from BE.
                        qts.push(libos.pop(qd).expect("pop qtoken"));
                        continue;
                    }

                    // This is TCP state from BE.
                    if TcpState::is_valid(buf) {
                        // Get next BE to migrate to.
                        let (be_qd, be_addr) = backends.pop_front().expect("no backends");
                        backends.push_back((be_qd, be_addr));

                        server_log_mig!("Received TCP State from BE, sending to next BE {}", be_addr);
                        qts.push(libos.push2(be_qd, buf).expect("push state"));

                        // Re-arm POP from old BE.
                        qts.push(libos.pop(qd).expect("pop qtoken"));
                        continue;
                    }

                    // New request from client.
                    // Save this request.
                    let request = buf.clone();

                    // Send ADD to switch.
                    server_log_mig!("New POP from client, sending PRISM ADD to SWITCH");
                    let (_, client) = libos.get_tcp_endpoints(qd);
                    let prism_buf = PrismPacket::add(client, local, FE_MAC).serialize(&mut prism_buf);
                    qts.push(libos.pushto2(switch_qd, prism_buf, switch_addr).expect("push2"));

                    // Wait for ADD back from switch.
                    qts.push(libos.pop(switch_qd).expect("pop switch"));

                    // Start tracking new migration.
                    assert!(migrations.insert(client, Migration { qd, request }).is_none(), "existing migration");
                },

                // UDP packet popped.
                OperationResult::Pop(Some(_), buf) => {
                    let pkt = PrismPacket::deserialize(buf).expect("invalid prism packet");
                    assert!(pkt.is_add());
                    server_log_mig!("Received PRISM ADD from SWITCH ({})", pkt.client);
                    
                    // Get BE to migrate to.
                    let (be_qd, be_addr) = backends.pop_front().expect("no backends");
                    backends.push_back((be_qd, be_addr));

                    // Get connection to migrate.
                    let Migration { qd: conn_qd, request } = migrations.remove(&pkt.client).expect("no such migration");
                    let mut state = libos.migrate_out_tcp_connection(conn_qd);
                    server_log_mig!("Migrated out {:#?}", state);
                    state.cb.push_front_recv_queue(request); // Push request back in.
                    let state_buf = state.serialize(&mut state_buf);

                    // Send state to BE.
                    qts.push(libos.push2(be_qd, state_buf).expect("push state"));
                    server_log_mig!("Sent TCP state to {}", be_addr);
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
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        assert_eq!(sockaddr.port(), 10000, "FE must be on port 10000");
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
