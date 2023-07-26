// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::{demi_sgarray_t, runtime::types::demi_opcode_t};
use ::demikernel::{
    LibOS,
    LibOSName,
    QDesc,
    QToken,
    runtime::logging,
};
use log::debug;
use std::collections::HashMap;
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
    time::Duration,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use std::sync::atomic::{AtomicBool, Ordering};
use ctrlc;
use std::sync::Arc;

//======================================================================================================================
// mksga()
//======================================================================================================================

// Makes a scatter-gather array.
fn mksga(libos: &mut LibOS, size: usize, value: u8) -> Result<demi_sgarray_t> {
    // Allocate scatter-gather array.
    let sga = match libos.sgaalloc(size) {
        Ok(sga) => sga,
        Err(e) => anyhow::bail!("failed to allocate scatter-gather array: {:?}", e),
    };

    // Ensure that scatter-gather array has the requested size.
    assert!(sga.sga_segs[0].sgaseg_len as usize == size);

    // Fill in scatter-gather array.
    let ptr: *mut u8 = sga.sga_segs[0].sgaseg_buf as *mut u8;
    let len: usize = sga.sga_segs[0].sgaseg_len as usize;
    let slice: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.fill(value);

    Ok(sga)
}

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

    let qt = match libos.pop(qd, None) {
        Ok(qt) => qt,
        Err(e) => panic!("pop failed: {:?}", e.cause),
    };
    Some(qt)
}

fn send_response(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    let sga = mksga(libos, data.len(), 0).unwrap();
    for (byte, data) in unsafe { std::slice::from_raw_parts_mut(sga.sga_segs[0].sgaseg_buf as *mut u8, data.len()) }.iter_mut().zip(data) {
        *byte = *data;
    }

    match libos.push(qd, &sga) {
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

        let result = match libos.wait_any(&qts, Some(Duration::from_micros(1))) {
            Ok(wait_result) => Some(wait_result),
            Err(e) if e.errno == libc::ETIMEDOUT => None,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        if let Some((index, result)) = result {
            qts.swap_remove(index);
            let qd = result.qr_qd.into();
            match result.qr_opcode {
                demi_opcode_t::DEMI_OPC_ACCEPT => {
                    let new_qd = unsafe { result.qr_value.ares.qd.into() };
                    if let Some(qt) = get_request(&mut libos, new_qd) {
                        qts.push(qt);
                    }

                    qts.push(get_connection(&mut libos, qd));
                },
                demi_opcode_t::DEMI_OPC_PUSH => {
                    if let Some(qt) = get_request(&mut libos, qd) {
                        qts.push(qt);

                        // This QDesc can be migrated. (waiting for new request)
                        migratable_qds.insert(qd, qt);
                    }
                },
                demi_opcode_t::DEMI_OPC_POP => {
                    // This QDesc can no longer be migrated. (currently processing a request)
                    migratable_qds.remove(&qd);

                    let sga = unsafe { result.qr_value.sga.sga_segs[0] };
                    let recvbuf = unsafe { std::slice::from_raw_parts(sga.sgaseg_buf as *const u8, sga.sgaseg_len as usize) };

                    // Request Processing Delay
                    // thread::sleep(Duration::from_micros(1));

                    qts.push(send_response(&mut libos, qd, recvbuf));
                },
                _ => {
                    unreachable!("RESULT: {:?}", result.qr_opcode);
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
    logging::initialize();
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
