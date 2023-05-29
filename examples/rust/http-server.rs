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
    runtime::logging,
};

#[cfg(feature = "capybara-log")]
use ::demikernel::tcpmig_profiler::{tcp_log};

use log::debug;
use std::collections::HashMap;
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
    thread, time::Duration,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;
const ROOT: &str = "/var/www/demo";

use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;
use std::sync::Arc;
//======================================================================================================================
// server()
//======================================================================================================================

fn get_connection(libos: &mut LibOS, listen_qd: QDesc) -> QToken {
    match libos.accept(listen_qd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    }
}

fn get_request(libos: &mut LibOS, qd: QDesc) -> Option<QToken> {
    /* #[cfg(feature = "tcp-migration")]
    match libos.notify_migration_safety(qd) {
        Ok(true) => return None,
        Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
        _ => (),
    }; */

    let qt = match libos.pop(qd) {
        Ok(qt) => qt,
        Err(e) => panic!("pop failed: {:?}", e.cause),
    };
    Some(qt)
}

fn send_response(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    // let data_str = std::str::from_utf8(data).unwrap();
    let data_str = String::from_utf8_lossy(data);

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
    
    
    // println!("full_path: {}", full_path);
    
    let response = match std::fs::read_to_string(full_path.as_str()) {
        Ok(contents) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents),
        Err(err) => format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n")
    };
    #[cfg(feature = "capybara-log")]
    {
        tcp_log(format!("PUSH: {}", response.lines().next().unwrap_or("")));
    }
    match libos.push2(qd, response.as_bytes()) {
        Ok(qt) => qt,
        Err(e) => panic!("push failed: {:?}", e.cause),
    }
}

fn server(local: SocketAddrV4) -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => panic!("listen failed: {:?}", e.cause),
    };

    let mut qts = vec![get_connection(&mut libos, sockqd)];

    let mut migratable_qds: HashMap<QDesc, QToken> = HashMap::new();

    loop {
        if qts.is_empty() || !running.load(Ordering::SeqCst) {
            break;
        }

        let result = match libos.trywait_any2(&qts) {
            Ok(wait_result) => wait_result,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        if let Some((index, qd, result)) = result {
            qts.swap_remove(index);
            match result {
                OperationResult::Accept(new_qd) => {
                    if let Some(qt) = get_request(&mut libos, new_qd) {
                        qts.push(qt);
                        // qts.insert(0, qt);
                    }

                    qts.push(get_connection(&mut libos, qd));
                },
                OperationResult::Push => {
                    if let Some(qt) = get_request(&mut libos, qd) {
                        qts.push(qt);
                        // qts.insert(0, qt);

                        // This QDesc can be migrated. (waiting for new request)
                        migratable_qds.insert(qd, qt);
                    }
                },
                OperationResult::Pop(_, recvbuf) => {
                    // This QDesc can no longer be migrated. (currently processing a request)
                    migratable_qds.remove(&qd);

                    // Request Processing Delay
                    // thread::sleep(Duration::from_secs(10));

                    qts.push(send_response(&mut libos, qd, &recvbuf));
                    // qts.insert(0, send_response(&mut libos, qd, &recvbuf));
                },
                _ => {
                    println!("RESULT: {:?}", result);
                    unreachable!();
                },
            }
        }

        #[cfg(feature = "tcp-migration")]
        {
            let mut qd_to_remove = None;

            for (&qd, &pop_qt) in migratable_qds.iter() {
                match libos.notify_migration_safety(qd) {
                    Ok(true) => {
                        let index = qts.iter().position(|&qt| qt == pop_qt).expect("`pop_qt` should be in `qts`");
                        qts.swap_remove(index);
                        qd_to_remove = Some(qd);
                        break;
                    },
                    Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
                    _ => (),
                };
            }

            if let Some(qd) = qd_to_remove {
                migratable_qds.remove(&qd);
            }
        }
    }

    eprintln!("server stopping");

    // loop {}

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    #[cfg(feature = "tcp-migration-profiler")]
    demikernel::tcpmig_profiler::write_profiler_data(&mut std::io::stdout()).unwrap();

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
    #[cfg(feature = "capybara-log")]
    {
        tcp_log(format!("*** CAPYBARA LOGGING IS ON ***"));
    }
    // logging::initialize();

    #[cfg(feature = "tcp-migration-profiler")]
    demikernel::tcpmig_profiler::init_profiler();

    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
