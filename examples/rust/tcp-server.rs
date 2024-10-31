// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

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
    collections::{
        hash_map::Entry,
        HashMap,
    },
    env::args,
    hash::{
        BuildHasher as _,
        BuildHasherDefault,
        DefaultHasher,
    },
    mem::take,
};

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

// fn respond_to_request(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
//     let h = BuildHasherDefault::<DefaultHasher>::default().hash_one(data);
//     let data = format!("{h:08x}");
//     libos.push2(qd, data.as_bytes()).expect("push success")
// }

#[inline(always)]
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

// fn push_data_and_run(libos: &mut LibOS, qd: QDesc, buffer: &mut Buffer, data: &[u8], qts: &mut Vec<QToken>) -> usize {
//     server_log!("buffer.data_size() {}", buffer.data_size());
//     // fast path: no previous data in the stream and this request contains exactly one HTTP request
//     if buffer.data_size() == 0 {
//         if find_subsequence(data, b"\r\n\r\n").unwrap_or(data.len()) == data.len() - 4 {
//             server_log!("responding 1");
//             let resp_qt = respond_to_request(libos, qd, data);
//             qts.push(resp_qt);
//             return 1;
//         }
//     }
//     // println!("* CHECK *\n");
//     // Copy new data into buffer
//     buffer.get_empty_buf()[..data.len()].copy_from_slice(data);
//     buffer.push_data(data.len());
//     server_log!("buffer.data_size() {}", buffer.data_size());
//     for sent in 0.. {
//         let dbuf = buffer.get_data();
//         if let Some(idx) = find_subsequence(dbuf, b"\r\n\r\n") {
//             server_log!("responding 2");
//             let resp_qt = respond_to_request(libos, qd, &dbuf[..idx + 4]);
//             qts.push(resp_qt);
//             buffer.pull_data(idx + 4);
//             buffer.try_shrink().unwrap();
//         } else {
//             return sent;
//         }
//     }
//     unreachable!()
// }

fn push_data_and_run(buf: &mut Vec<u8>, mut data: &[u8], mut push: impl FnMut(&[u8])) -> usize {
    let eoi = b"\r\n\r\n";

    let offset = find_subsequence(data, eoi);
    if buf.is_empty() && offset == Some(data.len() - eoi.len()) {
        respond(data, push);
        return 1;
    }

    let Some(offset) = offset else {
        buf.extend(data);
        return 0;
    };
    buf.extend(&data[..offset + eoi.len()]);
    respond(&take(buf), &mut push);

    let mut count = 1;
    data = &data[offset + eoi.len()..];
    while let Some(offset) = find_subsequence(data, eoi) {
        respond(&data[..offset + eoi.len()], &mut push);
        count += 1;
        data = &data[offset + eoi.len()..];
    }
    buf.extend(data);
    count
}

fn respond(item: &[u8], mut push: impl FnMut(&[u8])) {
    let h = BuildHasherDefault::<DefaultHasher>::default().hash_one(item);
    let data = format!("{h:08x}");
    push(data.as_bytes())
}

fn server(local: SocketAddrV4) -> Result<()> {
    eprintln!(
        "TCP MIGRATION {}abled",
        if cfg!(feature = "tcp-migration") { "EN" } else { "DIS" }
    );

    let mig_after: i32 = env::var("MIG_AFTER").as_deref().unwrap_or("10").parse().unwrap();
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        // LibOS::dpdk_print_eth_stats();
        // LibOS::capylog_dump(&mut std::io::stderr().lock());
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let libos_name: LibOSName = LibOSName::from_env().unwrap();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos
        .socket(libc::AF_INET, libc::SOCK_STREAM, 0)
        .expect("created socket");

    libos.bind(sockqd, local).expect("bind socket");
    libos.listen(sockqd, 300).expect("listen socket");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate = HashMap::new();

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
                    connstate.insert(new_qd, Vec::new());
                    #[cfg(feature = "manual-tcp-migration")]
                    {
                        let replaced = requests_remaining.insert(new_qd, mig_after);
                        assert!(replaced.is_none());
                    }
                    qts.push(libos.pop(new_qd).unwrap());
                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },

                OperationResult::Push => {
                    server_log!("PUSH complete");
                },

                OperationResult::Pop(_, recvbuf) => {
                    server_log!("POP complete");
                    let mut state = connstate.get_mut(&qd).unwrap();
                    let sent = push_data_and_run(&mut state, &recvbuf, |bytes| {
                        let qt = libos.push2(qd, bytes).expect("can push");
                        qts.push(qt);
                    });
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
                            server_log!("BUFFER DATA SIZE = {}", state.len());
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

                _ => panic!("Unexpected op: RESULT: {:?}", result),
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
fn usage(program_name: &str) {
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
