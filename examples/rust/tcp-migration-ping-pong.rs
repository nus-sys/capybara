// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================
use std::{io, str::from_utf8};
use std::time::SystemTime;
use std::io::prelude::*;

use ::anyhow::Result;
use demikernel::inetstack::MigrationHandle;
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
    inetstack::{
        protocols::{
            ip,
            tcp::{
                SeqNumber,
                peer::Socket,
                migration::{TcpState, TcpMigrationHeader, TcpMigrationSegment}
            },
        }
    }
};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
    thread, time::Duration
};


#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;


//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;

fn pause() {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();

    // We want the cursor to stay at the end of the line, so we print without a newline and flush manually.
    write!(stdout, "Press any key to continue...").unwrap();
    stdout.flush().unwrap();

    // Read a single byte and discard
    let _ = stdin.read(&mut [0u8]).unwrap();
}
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
// server_origin()
//======================================================================================================================

fn server_origin(local: SocketAddrV4, origin: SocketAddrV4, dest: SocketAddrV4) -> Result<()> {
    eprintln!("Setup libos");
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
   
    // Setup local socket.
    eprintln!("Setup local socket");
    let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };
    match libos.bind(sockqd, local) {
        Ok(()) => (),
        Err(e) => panic!("bind failed: {:?}", e.cause),
    };

    // // Mark as a passive one.
    match libos.listen(sockqd, 16) {
        Ok(()) => (),
        Err(e) => panic!("listen failed: {:?}", e.cause),
    };

    // Accept incoming connections.
    eprintln!("Accept incoming connections.");
    let qt_connection_in: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    let qd_connection_in: QDesc = match libos.wait2(qt_connection_in) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };
    eprintln!("TCP Connection established with client");

    // Process client messages (before migration).
    let mut cnt: i32 = 0;
    let mut handle: Option<MigrationHandle> = None;
    loop {
        cnt+=1;
        
        let qtoken: QToken = match libos.pop(qd_connection_in) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.wait2(qtoken) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // i += recvbuf.len();
        let msg = from_utf8(&recvbuf).unwrap();
        eprintln!("ping: {}", msg);

        
        let qt: QToken = match libos.push2(qd_connection_in, &recvbuf[..]) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Push)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };
        eprintln!("pong: {}", msg);
        
        // thread::sleep(Duration::from_millis(1000));
        if cnt == 10 {
            // Connect to migration destination.
            eprintln!("Connect to migration destination");
            let qd_migration_out: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
                Ok(qd) => qd,
                Err(e) => panic!("failed to create socket: {:?}", e.cause),
            };
            match libos.bind(qd_migration_out, origin) {
                Ok(()) => (),
                Err(e) => panic!("bind failed: {:?}", e.cause),
            };

            let qt_migration_out: QToken = match libos.connect(qd_migration_out, dest) {
                Ok(qt) => qt,
                Err(e) => panic!("connect failed: {:?}", e.cause),
            };
            match libos.wait2(qt_migration_out) {
                Ok((_, OperationResult::Connect)) => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };
            eprintln!("TCP Connection established with dest");

            handle = Some(libos.initiate_tcp_migration_out_sync(
                qd_migration_out,
                qd_connection_in,
                local,
                dest,
            ).unwrap());
        }
        else if cnt > 10 {
            break;
        }
    }

    let handle = handle.expect("MigrationHandle not set");
    libos.complete_tcp_migration_out_sync(handle).unwrap();

    // eprintln!("Sleep 10s...");
    // thread::sleep(Duration::from_millis(10000));
    // eprintln!("Resume!");

    /* let state = libos.migrate_out_tcp_connection(qd_connection_in, Some(dest))?; // fd: queue descriptor of the connection to be migrated
    let serialized = state.serialize().unwrap();
    
    // Push TcpState.
    let qt_push_tcpstate: QToken = match libos.push2(qd_migration_out, &serialized[..]) {
        Ok(qt) => qt,
        Err(e) => panic!("push failed: {:?}", e.cause),
    };
    match libos.wait2(qt_push_tcpstate) {
        Ok((_, OperationResult::Push)) => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };
    eprintln!("Push TcpState (len: {})", serialized.len());
    eprintln!("header: {:?}", state.header);
    eprintln!("TcpState: {}", from_utf8(&state.payload)?); */
    
    
    eprintln!("Sleep 10s...");
    thread::sleep(Duration::from_millis(10000));
    eprintln!("Resume!");

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// server_dest()
//======================================================================================================================

fn server_dest(local: SocketAddrV4) -> Result<()> {
    eprintln!("Setup libos.");
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    eprintln!("Setup peer.");
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
    eprintln!("Accept incoming connections.");
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    let qd: QDesc = match libos.wait2(qt) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };
    eprintln!("TCP Connection established with origin");

    
    eprintln!("Migrating in connection");
    let dest_fd = libos.perform_tcp_migration_in_sync(qd).unwrap();
    
    eprintln!("Sleep 1s...");
    thread::sleep(Duration::from_millis(1000));
    eprintln!("Resume!");

    
    /* let qtoken: QToken = match libos.pop(qd) {
        Ok(qt) => qt,
        Err(e) => panic!("pop failed: {:?}", e.cause),
    };
    // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
    let recvbuf = match libos.wait2(qtoken) {
        Ok((_, OperationResult::Pop(_, buf))) => buf,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    // let msg = &recvbuf;
    // eprintln!("pop: {}", msg);
    let deserialized = TcpMigrationSegment::deserialize(&recvbuf).unwrap();
    eprintln!("header: {:?}", deserialized.header);
    eprintln!("TcpState: {}", from_utf8(&deserialized.payload)?);
    
    eprintln!("Sleep 1s...");
    thread::sleep(Duration::from_millis(1000));
    eprintln!("Resume!");
    eprintln!("Migrating in connection");
    let dest_fd = libos.migrate_in_tcp_connection(deserialized.clone()).unwrap(); */


    // Process client messages (before migration).
    let mut cnt: i32 = 0;
    loop {
        cnt+=1;
        
        let qtoken: QToken = match libos.pop(dest_fd) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        // eprintln!("Waiting pop...");
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.wait2(qtoken) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // i += recvbuf.len();
        let msg = from_utf8(&recvbuf).unwrap();
        eprintln!("ping: {}", msg);


        let qt: QToken = match libos.push2(dest_fd, &recvbuf[..]) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Push)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };
        eprintln!("pong: {}", msg);
        
        // thread::sleep(Duration::from_millis(1000));
        if cnt == 10{
            break;
        }
    }



    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}


//======================================================================================================================
// client()
//======================================================================================================================

fn client(remote: SocketAddrV4) -> Result<()> {
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    // let fill_char: u8 = 'a' as u8;
    // let nbytes: usize = 64 * 1024;

    // Setup peer.
    let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };

    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => panic!("connect failed: {:?}", e.cause),
    };
    match libos.wait2(qt) {
        Ok((_, OperationResult::Connect)) => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    // pause();    
    let mut cnt: i32 = 0;
    let mut retransmissions = 0;
    loop {
        cnt+=1;
        
        let mut msg = String::from("Hello ");
        msg.push_str(&cnt.to_string());
        
        let qt: QToken = match libos.push2(sockqd, msg.as_bytes()) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Push)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };
        eprintln!("ping: {}", msg);
        
        // Pop data.
        let qt: QToken = match libos.pop(sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.timedwait2(qt, Some(SystemTime::now() + Duration::from_millis(2000))) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            Err(e) => {
                if e.errno == libc::ETIMEDOUT {
                    if retransmissions == 5 {
                        panic!("Exceeded 5 timeout retransmissions");
                    }

                    eprintln!("TIMEOUT: pong {}", cnt);
                    cnt -= 1;
                    retransmissions += 1;
                    continue;
                }
                panic!("operation failed: {:?}", e.cause)
            },
            _ => unreachable!(),
        };

        retransmissions = 0;

        let msg = from_utf8(&recvbuf).unwrap();
        eprintln!("pong: {}", msg);
        
        // thread::sleep(Duration::from_millis(1000));
        if cnt == 20{
            break;
        }
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
    println!("Usage: {} MODE address\n", program_name);
    println!("Modes:\n");
    println!("  --client    Run program in client mode.\n");
    println!("  --server    Run program in server mode.\n");
}



//======================================================================================================================
// main()
//======================================================================================================================


pub fn main() -> Result<()> {
    eprintln!("Hello main!");
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        if args[1] == "--server" {
            let local_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            let origin_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[3])?;
            let dest_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[4])?;
            eprintln!("I'm server!");
            let ret: Result<()> = server_origin(local_sockaddr, origin_sockaddr, dest_sockaddr);
            return ret;
        } else if args[1] == "--client" {
            let server_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            eprintln!("I'm client!");
            let ret: Result<()> = client(server_sockaddr);
            return ret;
        } else if args[1] == "--dest" {
            let dest_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            eprintln!("I'm destination!");
            let ret: Result<()> = server_dest(dest_sockaddr);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
