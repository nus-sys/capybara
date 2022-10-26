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
    let fill_char: u8 = 'a' as u8;
    let nrounds: usize = 1024;
    let expectbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);

    // Setup peer.
    let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };

    let sendbuf: Vec<u8> = mkbuf(BUFFER_SIZE, fill_char);
    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => panic!("connect failed: {:?}", e.cause),
    };
    match libos.wait2(qt) {
        Ok((_, OperationResult::Connect)) => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    let mut instants = Vec::with_capacity(nrounds);

    // Issue n sends.
    for i in 0..nrounds {
        let begin = Instant::now();

        // Push data.
        let qt: QToken = match libos.push2(sockqd, &sendbuf[..]) {
            Ok(qt) => qt,
            Err(e) => panic!("push failed: {:?}", e.cause),
        };
        match libos.wait2(qt) {
            Ok((_, OperationResult::Push)) => (),
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        // Pop data.
        let qtoken: QToken = match libos.pop(sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("pop failed: {:?}", e.cause),
        };
        // TODO: add type annotation to the following variable once we have a common buffer abstraction across all libOSes.
        let recvbuf = match libos.wait2(qtoken) {
            Ok((_, OperationResult::Pop(_, buf))) => buf,
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => unreachable!(),
        };

        instants.push((begin, Instant::now()));

        // Sanity check received data.
        assert_eq!(expectbuf[..], recvbuf[..], "server expectbuf != recvbuf");

        println!("ping {:?}", i);

        #[cfg(feature = "profiler")]
        profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
    }

    let instants = instants.iter()
        .map(|e| (e.0 - instants[0].0, e.1 - instants[0].1))
        .collect::<Vec<(Duration, Duration)>>();
    benchmark(&instants);

    // TODO: close socket when we get close working properly in catnip.
    Ok(())
}

//======================================================================================================================
// benchmark()
//======================================================================================================================

fn benchmark(instants: &[(Duration, Duration)]) {
    const THROUGHPUT_DURATION_MS: u128 = 5;

    let latency = instants.iter()
    .map(|e| e.1 - e.0)
    .fold(Duration::ZERO, |sum, e| sum + e).as_secs_f64() / (instants.len() as f64);
    
    let end = instants[instants.len() - 1].1.as_millis();
    let mut throughputs = Vec::new();

    let mut last_index = 0;
    for i in 0..(end / THROUGHPUT_DURATION_MS) {
        let begin = i * THROUGHPUT_DURATION_MS;
        let end = begin + THROUGHPUT_DURATION_MS;
        
        let mut index = 0;
        while instants[last_index + index].1.as_millis() < end {
            index += 1;
        }
        throughputs.push(index);
        last_index = index + 1;
    }

    println!("Latency: {}", latency);
    println!("Throughputs: {:?}", throughputs);
    println!("Avg throughput: {}/5ms", throughputs.iter().fold(0, |sum, e| sum + e) as f64 / throughputs.len() as f64);
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
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
        if args[1] == "--server" {
            let ret: Result<()> = server(sockaddr);
            return ret;
        } else if args[1] == "--client" {
            let ret: Result<()> = client(sockaddr);
            return ret;
        }
    }

    usage(&args[0]);

    Ok(())
}
