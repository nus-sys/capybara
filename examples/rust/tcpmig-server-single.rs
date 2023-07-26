// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::runtime::types::demi_opcode_t;
use ::demikernel::{
    LibOS,
    LibOSName,
    QDesc,
    QToken,
};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// server()
//======================================================================================================================

fn server(local: SocketAddrV4) -> Result<()> {
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

    // Accept incoming connections.
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    let qr = libos.wait(qt, None)?;
    let qd: QDesc = match qr.qr_opcode {
        demi_opcode_t::DEMI_OPC_ACCEPT => unsafe { qr.qr_value.ares.qd.into() },
        op => panic!("accept wait failed: {:?}", op),
    };

    loop {
        let qt = match libos.pop(qd, None) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };

        let result = match libos.wait(qt, Some(std::time::Duration::from_micros(1))) {
            Ok(wait_result) => Some(wait_result),
            Err(e) if e.errno == libc::ETIMEDOUT => None,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        if let Some(result) = result {
            let sga = match result.qr_opcode {
                demi_opcode_t::DEMI_OPC_POP => unsafe { result.qr_value.sga },
                _ => unreachable!("pop"),
            };
            let buf = unsafe { result.qr_value.sga.sga_segs[0] };
            let buf = unsafe { std::slice::from_raw_parts(buf.sgaseg_buf as *const u8, buf.sgaseg_len as usize) };

            // request processing time
            //thread::sleep(Duration::from_micros(1));

            println!("received ping {:?}", buf[0]);

            // Push data.
            let qt: QToken = match libos.push(qd, &sga) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            let result = libos.wait(qt, None).unwrap();
            match result.qr_opcode {
                demi_opcode_t::DEMI_OPC_PUSH => (),
                _ => unreachable!("push"),
            };

            println!("sent pong {:?}", buf[0]);
        }

        #[cfg(feature = "tcp-migration")]
        match libos.notify_migration_safety(qd) {
            Ok(true) => break,
            Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
            _ => (),
        };
    }

    eprintln!("server stopping");

    loop {}

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
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

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
