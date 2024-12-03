// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use std::{
    cell::RefCell, collections::HashMap, env::args, fs::read, mem::take, net::SocketAddrV4,
    ptr::null_mut, slice,
};

use ctrlc;
use demikernel::{
    inetstack::protocols::tcpmig::{
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
        let context = unsafe { tlse::tls_import_context(buf.as_ptr(), buf.len() as _) };
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
        let size = unsafe { tlse::tls_export_context(self.0, null_mut(), 0, 1) };
        size as _
    }

    fn serialize<'buf>(&self, buf: &'buf mut [u8]) -> &'buf mut [u8] {
        let size = unsafe { tlse::tls_export_context(self.0, buf.as_mut_ptr(), buf.len() as _, 1) };
        &mut buf[size as usize..]
    }
}

impl Drop for MigrateOut {
    fn drop(&mut self) {
        unsafe { tlse::tls_destroy_context(self.0) }
    }
}

fn server(local: SocketAddrV4, migrate: bool) {
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

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

                    PEER.with_borrow_mut(|peer| {
                        peer.contexts.entry(new_qd).or_insert_with(|| {
                            server_log!("creating connection context: qd={new_qd:?}");
                            let context = unsafe { tlse::tls_accept(server_context) };
                            unsafe { tlse::tls_make_exportable(context, 1) }
                            context
                        });
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
