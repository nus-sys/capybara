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

#[cfg(feature = "capybara-log")]
use ::demikernel::tcpmig_profiler::{tcp_log};

use std::collections::HashMap;
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
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
    
    let response = match std::fs::read_to_string(full_path.as_str()) {
        Ok(contents) => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", contents.len(), contents),
        Err(_) => format!("HTTP/1.1 404 NOT FOUND\r\n\r\nDebug: Invalid path\n"),
    };
    #[cfg(feature = "capybara-log")]
    {
        tcp_log(format!("PUSH: {}", response.lines().next().unwrap_or("")));
    }

    libos.push2(qd, response.as_bytes()).expect("push success")
}

struct ConnectionState {
    pushing: bool,
    pop_qt: QToken,
}

fn server(local: SocketAddrV4) -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    let sockqd: QDesc = libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0).expect("created socket");

    libos.bind(sockqd, local).expect("bind failed");
    libos.listen(sockqd, 16).expect("listen failed");

    let mut qts: Vec<QToken> = Vec::new();
    let mut connstate: HashMap<QDesc, ConnectionState> = HashMap::new();

    qts.push(libos.accept(sockqd).expect("accept"));

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let result = libos.trywait_any2(&qts).expect("result");

        if let Some((index, qd, result)) = result {
            qts.swap_remove(index);
            match result {
                OperationResult::Accept(new_qd) => {
                    // Pop from new_qd
                    let pop_qt = libos.pop(new_qd).expect("pop qt");
                    qts.push(pop_qt);
                    connstate.insert(new_qd, ConnectionState {
                        pushing: false,
                        pop_qt: pop_qt,
                    });

                    // Re-arm accept
                    qts.push(libos.accept(qd).expect("accept qtoken"));
                },
                OperationResult::Push => {
                    connstate.get_mut(&qd).unwrap().pushing = false;
                },
                OperationResult::Pop(_, recvbuf) => {
                    let send_qt = send_response(&mut libos, qd, &recvbuf);
                    qts.push(send_qt);

                    // queue next pop
                    let pop_qt = libos.pop(qd).expect("pop qt");
                    qts.push(pop_qt);

                    let mut state = connstate.get_mut(&qd).unwrap();
                    state.pushing = true;
                    state.pop_qt = pop_qt;
                },
                _ => {
                    panic!("Unexpected op: RESULT: {:?}", result);
                },
            }
        }

        #[cfg(feature = "tcp-migration")]
        {
            let mut qd_to_remove = None;

            for (qd, state) in connstate.iter() {
                if state.pushing {
                    continue;
                }
                match libos.notify_migration_safety(*qd) {
                    Ok(true) => {
                        let index = qts.iter().position(|&qt| qt == state.pop_qt).expect("`pop_qt` should be in `qts`");
                        qts.swap_remove(index);
                        qd_to_remove = Some(*qd);
                        break;
                    },
                    Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
                    _ => (),
                };
            }

            if let Some(qd) = qd_to_remove {
                connstate.remove(&qd);
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
