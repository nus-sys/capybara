// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================
use std::io;
use std::io::prelude::*;

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
    println!("Setup libos");
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
   
    // Setup local socket.
    println!("Setup local socket");
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
    println!("Accept incoming connections.");
    let qt_connection_in: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    let qd_connection_in: QDesc = match libos.wait2(qt_connection_in) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };
    println!("TCP connection (with client) establisehd");

    println!("Sleep 10s...");
    thread::sleep(Duration::from_millis(10000));
    println!("Resume!");

    // Connect to migration destination.
    println!("Connect to migration destination");
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
    println!("Connection established with dest");    

    println!("Sleep 10s...");
    thread::sleep(Duration::from_millis(10000));
    println!("Resume!");

    
    let state = libos.migrate_out_tcp_connection(qd_connection_in)?; // fd: queue descriptor of the connection to be migrated
    let serialized = state.serialize();

    // Push TcpState.
    println!("Push TcpState");
    let qt_push_tcpstate: QToken = match libos.push2(qd_migration_out, &&serialized[..]) {
        Ok(qt) => qt,
        Err(e) => panic!("push failed: {:?}", e.cause),
    };
    match libos.wait2(qt_push_tcpstate) {
        Ok((_, OperationResult::Push)) => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    

    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// server_dest()
//======================================================================================================================

fn server_dest(local: SocketAddrV4) -> Result<()> {
    println!("Setup libos.");
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Setup peer.
    println!("Setup peer.");
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
    println!("Accept incoming connections.");
    let qt: QToken = match libos.accept(sockqd) {
        Ok(qt) => qt,
        Err(e) => panic!("accept failed: {:?}", e.cause),
    };
    let qd: QDesc = match libos.wait2(qt) {
        Ok((_, OperationResult::Accept(qd))) => qd,
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    pause();

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

    // let sendbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => panic!("connect failed: {:?}", e.cause),
    };
    match libos.wait2(qt) {
        Ok((_, OperationResult::Connect)) => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    pause();


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
    println!("Hello main!");
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        if args[1] == "--server" {
            let local_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            let origin_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[3])?;
            let dest_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[4])?;
            println!("I'm server!");
            let ret: Result<()> = server_origin(local_sockaddr, origin_sockaddr, dest_sockaddr);
            return ret;
        } else if args[1] == "--client" {
            let server_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            println!("I'm client!");
            let ret: Result<()> = client(server_sockaddr);
            return ret;
        } else if args[1] == "--dest" {
            let dest_sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
            println!("I'm destination!");
            let ret: Result<()> = server_dest(dest_sockaddr);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
