// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
use ::anyhow::Result;
use std::{
    cell::RefCell, collections::HashMap, env::args, fs::read, mem::take, net::SocketAddrV4,
    ptr::null_mut, slice,
    rc::Rc,
};

use ctrlc;
use demikernel::{
    inetstack::protocols::tcpmig::{
        ApplicationState,
        set_user_connection_peer, UserConnectionMigrateOut, UserConnectionPeer,
    },
    runtime::memory::Buffer,
    LibOS, LibOSName, OperationResult, QDesc, QToken,
};

//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" {
                eprintln!("[https] {}", format!($($arg)*));
            }
        }
    };
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_data(buf: &mut Vec<u8>, mut data: &[u8], mut push: impl FnMut(&[u8])) -> usize {
    let eoi = b"\r\n\r\n"; // end of input i guess

    let offset = find_subsequence(data, eoi);
    if buf.is_empty() && offset == Some(data.len() - eoi.len()) {
        push(data);
        return 1;
    }

    let Some(offset) = offset else {
        buf.extend(data);
        return 0;
    };
    buf.extend(&data[..offset + eoi.len()]);
    push(&take(buf));

    let mut count = 1;
    data = &data[offset + eoi.len()..];
    while let Some(offset) = find_subsequence(data, eoi) {
        push(&data[..offset + eoi.len()]);
        count += 1;
        data = &data[offset + eoi.len()..];
    }
    buf.extend(data);
    count
}

fn respond(item: &[u8]) -> Vec<u8> {
    let Ok(_s) = std::str::from_utf8(item) else {
        return b"error".into();
    };
    // TODO return 404 for corresponded cases
    static INDEX_HTML: &str = include_str!("index.html");
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        INDEX_HTML.len(),
        INDEX_HTML
    )
    .into()
}

#[derive(Default)]
struct PeerData {
    incoming: HashMap<SocketAddrV4, Buffer>,
    contexts: HashMap<QDesc, *mut tlse::TLSContext>,
    // TODO partial HTTP requests
}

thread_local! {
    static PEER: RefCell<PeerData> = RefCell::new(Default::default());
}

impl UserConnectionPeer for PeerData {
    fn migrate_in(remote: SocketAddrV4, buffer: Buffer) {
        let replaced = PEER.with_borrow_mut(|peer| peer.incoming.insert(remote, buffer));
        assert!(replaced.is_none())
    }

    fn migration_complete(remote: SocketAddrV4, qd: QDesc) {
        let buf = PEER
            .with_borrow_mut(|peer| peer.incoming.remove(&remote))
            .expect("exists incoming connection data");
        // let start = std::time::Instant::now(); 
        let context = unsafe { tlse::tls_import_context(buf.as_ptr(), buf.len() as _) };
        // let duration = start.elapsed(); 
        // println!("tls_import_context time: {} microseconds", duration.as_nanos());

        // tls_import_context:
        // 1. allocate tls context
        // 2. init tls context with buf
        
        assert!(!context.is_null());
        let replaced = PEER.with_borrow_mut(|peer| peer.contexts.insert(qd, context));
        assert!(replaced.is_none())
    }

    type MigrateOut = MigrateOut;
    fn migrate_out(qd: QDesc) -> Self::MigrateOut {
        let context = PEER
            .with_borrow_mut(|peer| peer.contexts.remove(&qd))
            .expect("exists connection data");
        MigrateOut(context)
    }
}

struct MigrateOut(*mut tlse::TLSContext);

impl UserConnectionMigrateOut for MigrateOut {
    fn serialized_size(&self) -> usize {
        // let start = std::time::Instant::now(); // Start the timer
        let size = unsafe { tlse::tls_export_context(self.0, null_mut(), 0, 1) };
        // let duration = start.elapsed(); // Calculate the elapsed time
        // println!("tls_export_context1 time: {} microseconds", duration.as_nanos());

        size as _
    }

    fn serialize<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        // let start = std::time::Instant::now(); // Start the timer
        let size = unsafe { tlse::tls_export_context(self.0, buf.as_mut_ptr(), buf.len() as _, 1) };
        // let duration = start.elapsed(); // Calculate the elapsed time
        // println!("tls_export_context2 time: {} microseconds", duration.as_nanos());

        &mut buf[size as usize..]
    }
}

impl Drop for MigrateOut {
    fn drop(&mut self) {
        unsafe { tlse::tls_destroy_context(self.0) }
    }
}

const BUFSZ: usize = 4096;
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

#[cfg(feature = "tcp-migration")]
impl ApplicationState for AppBuffer {
    fn serialized_size(&self) -> usize {
        4 + self.data_size()
    }

    fn serialize(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&(self.data_size() as u32).to_be_bytes());
        buf[4..4 + self.data_size()].copy_from_slice(self.get_data());
    }

    fn deserialize(buf: &[u8]) -> Self where Self: Sized {
        let mut buffer = AppBuffer::new();
        let data_size = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        buffer.get_empty_buf()[0..data_size].copy_from_slice(&buf[4..4 + data_size]);
        buffer.push_data(data_size);
        buffer
    }
}

struct SessionData {
    data: Vec<u8>,
}

impl SessionData {
    fn new(size: usize) -> Self {
        Self { data: vec![5; size] }
    }
}

struct ConnectionState {
    buffer: AppBuffer,
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
        let buffer = AppBuffer::deserialize(buf);
        let session_data = SessionData::deserialize(&buf[buffer.serialized_size()..]);
        Self { buffer, session_data }
    }
}


fn server(local: SocketAddrV4, migrate: bool) {
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let session_data_size: usize = std::env::var("SESSION_DATA_SIZE").map_or(0, |v| v.parse().unwrap());


    let libos_name: LibOSName = LibOSName::from_env().unwrap();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    set_user_connection_peer::<PeerData>(&libos);

    let sockqd: QDesc = libos
        .socket(libc::AF_INET, libc::SOCK_STREAM, 0)
        .expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = vec![libos.accept(sockqd).expect("accept")];
    // Create qrs filled with garbage.
    let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
    qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
    let mut indices: Vec<usize> = Vec::with_capacity(2000);
    indices.resize(2000, 0);

    let server_context = unsafe { tlse::tls_create_context(1, tlse::TLS_V12 as _) };
    let cert = read("/usr/local/tls/svr.crt").expect("can read certificate file");
    unsafe { tlse::tls_load_certificates(server_context, cert.as_ptr(), cert.len() as _) };
    let key = read("/usr/local/tls/svr.key").expect("can read key file");
    unsafe { tlse::tls_load_private_key(server_context, key.as_ptr(), key.len() as _) };

    loop {
        let result_count = libos
            .wait_any2(&qts, &mut qrs, &mut indices, None)
            .expect("result");
        server_log!("OS: I/O operations have been completed, take the results!");

        let results = &qrs[..result_count];
        let indices = &indices[..result_count];

        for (index, (qd, result)) in indices.iter().zip(results.iter()).rev() {
            let (index, qd) = (*index, *qd);
            qts.swap_remove(index);

            match result {
                OperationResult::Accept(new_qd) => {
                    let new_qd = *new_qd;
                    server_log!("ACCEPT complete {:?} ==> issue POP and ACCEPT", new_qd);

                    // #[cfg(feature = "tcp-migration")]
                    // let state = if let Some(data) = libos.get_migrated_application_state::<ConnectionState>(new_qd) {
                    //     server_log!("Connection State LOG: Received migrated app data ({} bytes)", data.borrow().serialized_size());
                    //     data
                    // } else {
                    //     server_log!("Connection State LOG: No migrated app data, creating new ConnectionState");
                    //     let state = Rc::new(RefCell::new(ConnectionState {
                    //         buffer: AppBuffer::new(),
                    //         session_data: SessionData::new(session_data_size),
                    //     }));
                    //     libos.register_application_state(new_qd, state.clone());
                    //     state
                    // }; 
                    // Inho: it's not working. Need to be debugged. 

                    PEER.with_borrow_mut(|peer| {
                        if peer.contexts.contains_key(&new_qd) {
                            // If the context already exists, it means this conneciton is migrated in (i.e., migraiton_complete inserted the new_qd before)
                            /* ACTIVATE THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                            static mut NUM_MIG: u32 = 0;
                            unsafe{ NUM_MIG += 1; }
                            if unsafe{ NUM_MIG } < 11000 {
                                libos.initiate_migration(new_qd).unwrap();
                            }
                            else {
                                eprintln!("{} migrations are done", unsafe{ NUM_MIG });
                            }
                            /* ACTIVATE THIS FOR APP_STATE_SIZE VS MIG_LAT EVAL */
                        } else {
                            // Otherwise, create the connection context as before
                            peer.contexts.entry(new_qd).or_insert_with(|| {
                                server_log!("creating connection context: qd={new_qd:?}");
                                let context = unsafe { tlse::tls_accept(server_context) };
                                unsafe { tlse::tls_make_exportable(context, 1) };
                                context
                            });
                        }
                    });

                    qts.push(libos.pop(new_qd).unwrap());
                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                }

                OperationResult::Push => {
                    server_log!("PUSH complete");
                }

                OperationResult::Pop(_, recvbuf) => {
                    server_log!("POP complete");
                    // server_log!("{:?}", &**recvbuf);

                    let context = PEER.with_borrow(|peer| peer.contexts[&qd]);
                    unsafe {
                        tlse::tls_consume_stream(
                            context,
                            recvbuf.as_ptr(),
                            recvbuf.len() as _,
                            None,
                        )
                    };
                    if unsafe { tlse::tls_established(context) } != 0 {
                        if migrate {
                            libos.initiate_migration(qd).expect("can migrate");
                        } else {
                            let mut inputs = Vec::new();
                            let mut count = 0;

                            let mut buf = vec![0; 4 << 10];
                            loop {
                                let len = unsafe {
                                    tlse::tls_read(context, buf.as_mut_ptr(), buf.len() as _)
                                };
                                assert!(len >= 0);
                                if len == 0 {
                                    break;
                                }
                                // TODO save partial HTTP requests
                                count +=
                                    push_data(&mut Vec::new(), &buf[..len as usize], |input| {
                                        inputs.push(input.to_vec())
                                    });
                            }

                            server_log!("responding {count} inputs");
                            for input in inputs {
                                let output = respond(&input);
                                unsafe {
                                    tlse::tls_write(context, output.as_ptr(), output.len() as _)
                                };
                            }
                        }
                    }

                    let mut write_buf_len = 0u32;
                    let write_buf = unsafe {
                        tlse::tls_get_write_buffer(context, &mut write_buf_len as *mut _)
                    };
                    if write_buf_len > 0 {
                        let qt = libos
                            .push2(qd, unsafe {
                                slice::from_raw_parts(write_buf, write_buf_len as _)
                            })
                            .expect("can push");
                        qts.push(qt);
                        unsafe { tlse::tls_buffer_clear(context) }
                    }

                    qts.push(libos.pop(qd).expect("pop qt"));
                }

                OperationResult::Failed(err) => match err.errno {
                    demikernel::ETCPMIG => {
                        server_log!("migrated {:?} polled", qd)
                    }
                    _ => panic!("operation failed: {}", err),
                },

                _ => panic!("Unexpected op: RESULT: {:?}", result),
            }
        }
        server_log!("APP: Okay, handled the results!");
    }
}

//======================================================================================================================
// usage()
//======================================================================================================================

/// Prints program usage and exits.
fn usage(program_name: &str) {
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() {
    server_log!("*** HTTP SERVER LOGGING IS ON ***");
    let Some(addr) = args().nth(1) else {
        usage(&args().nth(0).unwrap());
        return;
    };
    let flag = args().nth(2);
    let migrate = flag.as_deref() == Some("migrate");
    server(addr.parse().expect("valid ipv4 socket address"), migrate)
}
