// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use demikernel::{runtime::types::demi_opcode_t, demi_sgarray_t};
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use std::{time::{Instant, Duration, SystemTime}, sync::Mutex};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
    thread,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

//======================================================================================================================
// Constants
//======================================================================================================================

const BUFFER_SIZE: usize = 64;

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

    // Setup peer.
    let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
        Ok(qd) => qd,
        Err(e) => panic!("failed to create socket: {:?}", e.cause),
    };

    let qt: QToken = match libos.connect(sockqd, remote) {
        Ok(qt) => qt,
        Err(e) => panic!("connect failed: {:?}", e.cause),
    };
    match libos.wait(qt, None) {
        Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_CONNECT => (),
        Err(e) => panic!("operation failed: {:?}", e.cause),
        _ => unreachable!(),
    };

    //let mut instants = Vec::with_capacity(nrounds);

    const FREQ: Duration = Duration::from_micros(100);
    let rounds = 128;

    // Issue n sends.
    for j in 1..10000 {
        for i in 1..=rounds {
            //let begin = Instant::now();
            
            let sendbuf = mksga(&mut libos, BUFFER_SIZE, i).unwrap();
            // Push data.
            let qt: QToken = match libos.push(sockqd, &sendbuf) {
                Ok(qt) => qt,
                Err(e) => panic!("push failed: {:?}", e.cause),
            };
            match libos.wait(qt, None) {
                Ok(qr) if qr.qr_opcode == demi_opcode_t::DEMI_OPC_PUSH => (),
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            println!("ping {}", i);

            // Pop data.
            let qt: QToken = match libos.pop(sockqd, None) {
                Ok(qt) => qt,
                Err(e) => panic!("pop failed: {:?}", e.cause),
            };
            let recvbuf = match libos.wait(qt, None) {
                Ok(op) if op.qr_opcode == demi_opcode_t::DEMI_OPC_POP => {
                    let buf = unsafe { op.qr_value.sga.sga_segs[0] };
                    unsafe { std::slice::from_raw_parts(buf.sgaseg_buf as *const u8, buf.sgaseg_len as usize) }
                },
                Err(e) if e.errno == libc::ETIMEDOUT => continue,
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => unreachable!(),
            };

            println!("pong {}", recvbuf[0]);

            thread::sleep(FREQ);

            //instants.push((begin, Instant::now()));

            #[cfg(feature = "profiler")]
            profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");
        }
    }

    /* let instants = instants.iter()
        .map(|e| (e.0 - instants[0].0, e.1 - instants[0].0))
        .collect::<Vec<(Duration, Duration)>>(); */
    //benchmark(&instants);

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
        last_index = last_index + index;
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
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return client(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}
