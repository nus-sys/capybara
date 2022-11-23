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
use std::time::{Instant, Duration};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;

//======================================================================================================================
// mkbuf()
//======================================================================================================================

fn mkbuf(buffer_size: usize, fill_char: u8) -> Vec<u8> {
    let mut data: Vec<u8> = Vec::<u8>::with_capacity(buffer_size);

    for _ in 0..buffer_size {
        data.push(fill_char);
    }

    data
}

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
    let fill_char: u8 = 'a' as u8;
    let nrounds: usize = 1024;
    let expectbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);

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
    let qd: QDesc = match libos.wait2(qt) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    // Perform multiple ping-pong rounds.
    for i in 0..nrounds {
        // Pop data.
        let qtoken: QToken = match libos.pop(qd) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.wait2(qtoken) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // Sanity check received data.
        assert_eq!(expectbuf[..], recvbuf[..], "server expectbuf != recvbuf");

        // Push data.
        let qt: QToken = match libos.push2(qd, &recvbuf[..]) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Push)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        println!("pong {:?}", i);

        libos.notify_migration_safety(qd)?;
    }

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

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
