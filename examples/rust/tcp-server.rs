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

use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use ctrlc;
use std::{
    cell::RefCell,
    collections::{
        hash_map::Entry,
        HashMap,
    },
    env::{
        args,
        var,
    },
    hash::{
        BuildHasher as _,
        BuildHasherDefault,
        DefaultHasher,
    },
    rc::Rc,
};

#[cfg(feature = "tcp-migration")]
use demikernel::inetstack::protocols::tcpmig::ApplicationState;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

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

// Borrowed from Loadgen
struct Buffer {
    buf: Vec<u8>,
    head: usize,
    tail: usize,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            buf: vec![0; 4 << 10],
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

    fn deserialize(buf: &[u8]) -> Self
    where
        Self: Sized,
    {
        let mut buffer = Buffer::new();
        let data_size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        buffer.get_empty_buf()[0..data_size].copy_from_slice(&buf[4..4 + data_size]);
        buffer.push_data(data_size);
        buffer
    }
}

fn respond_to_request(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    let h = BuildHasherDefault::<DefaultHasher>::default().hash_one(data);
    let data = format!("{h:08x}");
    libos.push2(qd, data.as_bytes()).expect("push success")
}

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
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
    for sent in 0.. {
        let dbuf = buffer.get_data();
        if let Some(idx) = find_subsequence(dbuf, b"\r\n\r\n") {
            server_log!("responding 2");
            let resp_qt = respond_to_request(libos, qd, &dbuf[..idx + 4]);
            qts.push(resp_qt);
            buffer.pull_data(idx + 4);
            buffer.try_shrink().unwrap();
        } else {
            return sent;
        }
    }
    unreachable!()
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

    fn deserialize(buf: &[u8]) -> Self
    where
        Self: Sized,
    {
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

    fn deserialize(buf: &[u8]) -> Self
    where
        Self: Sized,
    {
        let buffer = Buffer::deserialize(buf);
        let session_data = SessionData::deserialize(&buf[buffer.serialized_size()..]);
        Self { buffer, session_data }
    }
}

fn server(local: SocketAddrV4) -> Result<()> {
    #[cfg(feature = "tcp-migration")]
    {
        eprintln!("TCP MIGRATION ENABLED");
    }
    #[cfg(not(feature = "tcp-migration"))]
    {
        eprintln!("TCP MIGRATION DISABLED");
    }

    let mig_after: i32 = env::var("MIG_AFTER").as_deref()
            .unwrap_or("10") // Default value is 10 if MIG_PER_N is not set
            .parse()
            .unwrap();
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        // LibOS::dpdk_print_eth_stats();
        // LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let session_data_size: usize = var("SESSION_DATA_SIZE").map_or(1024, |v| v.parse().unwrap());

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos
        .socket(libc::AF_INET, libc::SOCK_STREAM, 0)
        .expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, Rc<RefCell<ConnectionState>>> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    #[cfg(feature = "manual-tcp-migration")]
    let mut requests_remaining: HashMap<QDesc, i32> = HashMap::new();

    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);

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
                    let new_qd = *new_qd;
                    server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);

                    #[cfg(feature = "tcp-migration")]
                    let state = if let Some(data) = libos.get_migrated_application_state::<ConnectionState>(new_qd) {
                        server_log!(
                            "Connection State LOG: Received migrated app data ({} bytes)",
                            data.borrow().serialized_size() - 8
                        );
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

                    // Pop from new_qd
                    /* COMMENT OUT THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                    qts.push(libos.pop(new_qd).unwrap());
                    #[cfg(feature = "manual-tcp-migration")]
                    {
                        let replaced = requests_remaining.insert(new_qd, mig_after);
                        assert!(replaced.is_none());
                    }
                    /* COMMENT OUT THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */

                    connstate.insert(new_qd, state);

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

                    #[cfg(feature = "manual-tcp-migration")]
                    if let Entry::Occupied(mut entry) = requests_remaining.entry(qd) {
                        let remaining = entry.get_mut();
                        *remaining -= 1;
                        if *remaining > 0 {
                            // queue next pop
                            qts.push(libos.pop(qd).expect("pop qt"));
                            server_log!("Migrating after {} more requests, Issued POP", remaining);
                        } else {
                            server_log!("Should be migrated (no POP issued)");
                            server_log!("BUFFER DATA SIZE = {}", state.buffer.data_size());
                            libos.initiate_migration(qd).unwrap();
                            entry.remove();
                        }
                    }
                },

                OperationResult::Failed(e) => match e.errno {
                    #[cfg(feature = "tcp-migration")]
                    demikernel::ETCPMIG => {
                        server_log!("migrated {:?} polled", qd)
                    },
                    _ => panic!("operation failed: {}", e),
                },

                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        }
        // #[cfg(feature = "profiler")]
        // profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

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
    if let Some(addr) = args().nth(1) {
        return server(SocketAddrV4::from_str(&addr)?);
    }
    usage(&args().nth(0).unwrap());
    Ok(())
}
