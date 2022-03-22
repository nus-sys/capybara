// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(try_blocks)]

mod common;

//==============================================================================
// Imports
//==============================================================================

use self::common::Test;
use ::demikernel::{
    Ipv4Endpoint,
    OperationResult,
};
use ::std::{
    panic,
    process,
    sync::mpsc,
    thread,
    time::Duration,
};

//==============================================================================
// Ping Pong
//==============================================================================

#[test]
fn udp_ping_pong() {
    let mut test = Test::new();
    let mut npongs: usize = 1000;
    let payload: u8 = 'a' as u8;
    let local_addr: Ipv4Endpoint = test.local_addr();
    let remote_addr: Ipv4Endpoint = test.remote_addr();

    // Setup peer.
    let sockfd = test
        .libos
        .socket(libc::AF_INET, libc::SOCK_DGRAM, 0)
        .unwrap();
    test.libos.bind(sockfd, local_addr).unwrap();

    // Run peers.
    if test.is_server() {
        let mut npongs: usize = 0;
        loop {
            let sendbuf = test.mkbuf(payload);
            let mut qtoken = test.libos.pop(sockfd).expect("server failed to pop()");

            // Spawn timeout thread.
            let (sender, receiver) = mpsc::channel();
            let t = thread::spawn(
                move || match receiver.recv_timeout(Duration::from_secs(60)) {
                    Ok(_) => {},
                    _ => process::exit(0),
                },
            );

            // Wait for incoming data,
            let recvbuf = match test.libos.wait2(qtoken) {
                Ok((_, OperationResult::Pop(_, buf))) => buf,
                _ => panic!("server failed to wait()"),
            };

            // Join timeout thread.
            sender.send(0).unwrap();
            t.join().expect("timeout");

            // Sanity check contents of received buffer.
            assert!(
                Test::bufcmp(&sendbuf, recvbuf),
                "server sendbuf != recevbuf"
            );

            // Send data.
            qtoken = test
                .libos
                .pushto2(sockfd, sendbuf.as_slice(), remote_addr)
                .expect("server failed to pushto2()");
            test.libos.wait(qtoken).unwrap();

            npongs += 1;
            println!("pong {:?}", npongs);
        }
    } else {
        let mut npings: usize = 0;
        let mut qtokens = Vec::new();
        let sendbuf = test.mkbuf(payload);

        // Push pop first packet.
        let (qt_push, qt_pop) = {
            let qt_push = test
                .libos
                .pushto2(sockfd, sendbuf.as_slice(), remote_addr)
                .expect("client failed to pushto2()");
            let qt_pop = test.libos.pop(sockfd).expect("client failed to pop()");
            (qt_push, qt_pop)
        };
        qtokens.push(qt_push);
        qtokens.push(qt_pop);

        // Send packets.
        while npongs > 0 {
            // FIXME: If any packet is lost this will hang.

            let (ix, _, result) = test.libos.wait_any2(&qtokens);
            qtokens.swap_remove(ix);

            // Parse result.
            match result {
                OperationResult::Push => {
                    let (qt_push, qt_pop) = {
                        let qt_push = test
                            .libos
                            .pushto2(sockfd, sendbuf.as_slice(), remote_addr)
                            .expect("client failed to pushto2()");
                        let qt_pop = test.libos.pop(sockfd).expect("client failed to pop()");
                        (qt_push, qt_pop)
                    };
                    qtokens.push(qt_push);
                    qtokens.push(qt_pop);
                    println!("ping {:?}", npings);
                },
                OperationResult::Pop(_, recvbuf) => {
                    // Sanity received buffer.
                    assert!(
                        Test::bufcmp(&sendbuf, recvbuf),
                        "server expectbuf != recevbuf"
                    );
                    npings += 1;
                    npongs -= 1;
                },
                _ => panic!("unexpected result"),
            }
        }
    }
}