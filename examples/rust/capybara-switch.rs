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

use std::{cell::RefCell, collections::{HashMap, HashSet, hash_map::Entry}, rc::Rc, time::Instant};
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

fn push_data_and_run(libos: &mut LibOS, qd: QDesc, data: &[u8], qts: &mut Vec<QToken>) {
    match std::str::from_utf8(data) {
        Ok(string_data) => {
            eprintln!("data: {}", string_data);
        }
        Err(_) => {
            eprintln!("data: (invalid UTF-8)");
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

fn capybara_switch(tcp_address: SocketAddrV4, udp_address: SocketAddrV4) -> Result<()> {
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    
    
    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");

    let mut qts: Vec<QToken> = Vec::new();
    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);
    
    // let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices, None).expect("result");
    libos.capybara_switch();
    // loop {
    //     let result_count = libos.wait_any2(&qts, &mut qrs, &mut indices, None).expect("result");
            
    //     server_log!("\n\n======= OS: I/O operations have been completed, take the results! =======");

    //     let results = &qrs[..result_count];
    //     let indices = &indices[..result_count];

    //     for (index, (qd, result)) in indices.iter().zip(results.iter()).rev() {
    //         let (index, qd) = (*index, *qd);
    //         qts.swap_remove(index);

    //         #[cfg(feature = "manual-tcp-migration")]
    //         let mut should_migrate_this_qd = false;

    //         match result {
    //             OperationResult::Accept(new_qd) => {
    //                 let new_qd = *new_qd;
    //                 server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);                    
    //                 // Pop from new_qd
    //                 match libos.pop(new_qd) {
    //                     Ok(pop_qt) => {
    //                         qts.push(pop_qt)
    //                     },
    //                     Err(e) => panic!("pop qt: {}", e),
    //                 }
    //                 // Re-arm accept
    //                 qts.push(libos.accept(qd).expect("accept qtoken"));

    //             },

    //             OperationResult::Push => {
    //                 server_log!("PUSH complete");
    //             },

    //             OperationResult::Pop(None, recvbuf) => {
    //                 server_log!("POP complete");
    //                 // let mut state = connstate.get_mut(&qd).unwrap().borrow_mut();
                    
    //                 push_data_and_run(&mut libos, qd, &recvbuf, &mut qts);

    //                 //server_log!("Issued PUSH => {} pushes pending", state.pushing);
    //                 // server_log!("Issued {sent} PUSHes");
                    
    //                 // queue next pop
    //                 qts.push(libos.pop(qd).expect("pop qt"));
    //                 server_log!("Issued POP");
    //             },

    //             OperationResult::Failed(e) => {
    //                 match e.errno {
    //                     #[cfg(feature = "tcp-migration")]
    //                     demikernel::ETCPMIG => { server_log!("migrated {:?} polled", qd) },
    //                     _ => panic!("operation failed: {}", e),
    //                 }
    //             }

    //             _ => {
    //                 panic!("Unexpected op: RESULT: {:?}", result);
    //             },
    //         }
    //     }
    //     // #[cfg(feature = "profiler")]
    //     // profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
        
    //     server_log!("******* APP: Okay, handled the results! *******");
    // }

    eprintln!("server stopping");

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
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let tcp_address = SocketAddrV4::from_str(&args[1]).expect("Invalid TCP address");
        let udp_address = SocketAddrV4::from_str(&args[2]).expect("Invalid UDP address");

        return capybara_switch(tcp_address, udp_address);
    }

    usage(&args[0]);

    Ok(())
}
