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

fn get_connection(libos: &mut LibOS, listen_qd: QDesc) -> QToken {
    match libos.accept(listen_qd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    }
}

fn get_request(libos: &mut LibOS, qd: QDesc) -> Option<QToken> {
    match libos.notify_migration_safety(qd) {
        Ok(true) => return None,
        Err(e) => panic!("notify migration safety failed: {:?}", e.cause),
        _ => (),
    };

    let qt = match libos.pop(qd) {
        Ok(qt) => qt,
        Err(e) => panic!("pop failed: {:?}", e.cause),
    };
    Some(qt)
}

fn send_response(libos: &mut LibOS, qd: QDesc, data: &[u8]) -> QToken {
    match libos.push2(qd, data) {
        Ok(qt) => qt,
        Err(e) => panic!("push failed: {:?}", e.cause),
    }
}

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

    let mut qts = vec![get_connection(&mut libos, sockqd)];

    loop {
        let (index, qd, result) = match libos.wait_any2(&qts) {
            Ok(wait_result) => wait_result,
            Err(e) => panic!("operation failed: {:?}", e.cause),
        };

        qts.swap_remove(index);
        match result {
            OperationResult::Accept(new_qd) => {
                if let Some(qt) = get_request(&mut libos, new_qd) {
                    qts.push(qt);
                }

                qts.push(get_connection(&mut libos, qd));
            },
            OperationResult::Push => {
                if let Some(qt) = get_request(&mut libos, qd) {
                    qts.push(qt);
                }
            },
            OperationResult::Pop(_, recvbuf) => {
                qts.push(send_response(&mut libos, qd, &recvbuf));
            },
            _ => unreachable!(),
        }
    }

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    //Ok(())
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
