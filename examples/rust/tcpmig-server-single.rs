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
    let qd: QDesc = match libos.wait2(qt) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    loop {
        let qt = match libos.pop(qd) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };

        let result = match libos.trywait_any2(&[qt]) {
            Ok(wait_result) => wait_result,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        if let Some((_, _, result)) = result {
            let buf = match result {
                OperationResult::Pop(_, recvbuf) => recvbuf,
                _ => unreachable!("pop"),
            };

            // request processing time
            //thread::sleep(Duration::from_micros(1));

            println!("received ping {:?}", buf[0]);

            // Push data.
            let qt: QToken = match libos.push2(qd, &buf) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            match libos.wait2(qt) {
                Ok((_, OperationResult::Push)) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            println!("sent pong {:?}", buf[0]);
        }

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
