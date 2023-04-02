// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

// A (Very) Simple HTTP Server.

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::{
        fail::Fail,
        types::demi_opcode_t,
    },
    LibOS,
    LibOSName,
    QDesc,
    QToken,
    runtime::logging,
};
use ::std::{
    env,
    collections::HashMap,
    fs,
    io::Write,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    slice,
    str::FromStr,
    panic,
};

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

// The Connection type tracks connection state.
struct Connection {
    queue_descriptor: QDesc,
    receive_queue: Vec<demi_sgarray_t>,
}

impl Connection {
    pub fn new(qd: QDesc) -> Self {
        Connection {
            queue_descriptor: qd,
            receive_queue: Vec::new(),
        }
    }

    pub fn process_data(&mut self, libos: &mut LibOS, rsga: demi_sgarray_t) -> Result<Option<QToken>, Fail> {
        // We only handle single-segment scatter-gather arrays for now.
        assert_eq!(rsga.sga_numsegs, 1);

        // Print the incoming data.
        let slice: &mut [u8] = unsafe {
            slice::from_raw_parts_mut(
                rsga.sga_segs[0].sgaseg_buf as *mut u8,
                rsga.sga_segs[0].sgaseg_len as usize,
            )
        };
        println!("Received: {}", String::from_utf8_lossy(slice));

        // Add incoming scatter-gather segment to our receive queue.
        self.receive_queue.push(rsga);

        // Craft a response and send it.
        let ssga: demi_sgarray_t = libos.sgaalloc(1500)?;
        let mut slice: &mut [u8] = unsafe {
            slice::from_raw_parts_mut(
                ssga.sga_segs[0].sgaseg_buf as *mut u8,
                ssga.sga_segs[0].sgaseg_len as usize,
            )
        };

        // Read the entire file into a string.
        let contents: String = fs::read_to_string("hello.html").expect("file should exist and be readable.");

        write!(
            slice,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            contents.len(),
            contents
        )?;

        // Write response.
        let qt = libos.push(self.queue_descriptor, &ssga)?;

        Ok(Some(qt))
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
    println!("Listening on local address: {:?}", local);

    let mut qts = vec![get_connection(&mut libos, sockqd)];

    let mut migratable_qds: HashMap<QDesc, QToken> = HashMap::new();

    // Create hash table of accepted connections.
    let mut connections: HashMap<QDesc, Connection> = HashMap::new();

    // Loop over queue tokens we're waiting on.
    loop {//HERE
        let (index, qr) = match libos.wait_any(&qts) {
            Ok((i, qr)) => (i, qr),
            Err(e) => panic!("Wait failed: {:?}", e),
        };

        // Since this QToken completed, remove it from the list of waiters.
        qts.swap_remove(index);

        // Find out what operation completed:
        match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_ACCEPT => {
                // A new connection arrived.
                let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                println!("Connection established!  Queue Descriptor = {:?}", qd);

                // Create new connection state and store it.
                let connection: Connection = Connection::new(qd);
                connections.insert(qd, connection);

                if let Some(qt) = get_request(&mut libos, qd) {
                    qts.push(qt);
                }

                qts.push(get_connection(&mut libos, sockqd));
            },
            demi_opcode_t::DEMI_OPC_POP => {
                // A pop completed.
                let qd: QDesc = qr.qr_qd.into();
                let recv_sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                migratable_qds.remove(&qd);
                // Find Connection for this queue descriptor.
                let connection: &mut Connection = connections.get_mut(&qd).expect("HashMap should hold connection!");

                // Process the incoming request.
                let oqt: Option<QToken> = match connection.process_data(&mut libos, recv_sga) {
                    Ok(oqt) => oqt,
                    Err(e) => panic!("process data failed: {:?}", e.cause),
                };

                if oqt.is_some() {
                    qts.push(oqt.expect("oqt should be some!"));
                }

                // // Post another pop.
                // match libos.pop(qd) {
                //     Ok(qt) => waiters.push(qt),
                //     Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                // }
            },
            demi_opcode_t::DEMI_OPC_PUSH => {
                // A push completed.
                let qd: QDesc = qr.qr_qd.into();
                if let Some(qt) = get_request(&mut libos, qd) {
                    qts.push(qt);

                    // This QDesc can be migrated. (waiting for new request)
                    migratable_qds.insert(qd, qt);
                }
                // ToDo: Can free the sga now.
            },
            demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
            _ => {
                println!("RESULT: {:?}", qr.qr_opcode);
                unreachable!();
            },
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
    loop {}
    #[cfg(feature = "profiler")]
    profiler::write(&mut std::io::stdout(), None).expect("failed to write to stdout");

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
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        let sockaddr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
        return server(sockaddr);
    }

    usage(&args[0]);

    Ok(())
}

/* 
fn main() -> ! {
    // Pull LibOS from environment variable "LIBOS" if present.  Otherwise, default to Catnap.
    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(_) => LibOSName::Catnap,
    };

    // Initialize the LibOS.
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    // Create listening socket (a QDesc in Demikernel).
    let local_addr: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 1, 8), 10000);
    let listening_qd: QDesc = match create_listening_socket(&mut libos, local_addr) {
        Ok(qd) => qd,
        Err(e) => panic!("create_listening_socket failed: {:?}", e.cause),
    };

    println!("Listening on local address: {:?}", local_addr);

    // Create list of queue tokens (QToken) representing operations we're going to wait on.
    let mut waiters: Vec<QToken> = Vec::new();

    // Post an accept for the first connection.
    let accept_qt: QToken = match libos.accept(listening_qd) {
        Ok(qt) => qt,
        Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
    };
    // Add this accept to the list of operations we're waiting to complete.
    waiters.push(accept_qt);

    // Create hash table of accepted connections.
    let mut connections: HashMap<QDesc, Connection> = HashMap::new();

    // Loop over queue tokens we're waiting on.
    loop {
        let (index, qr) = match libos.wait_any(&waiters) {
            Ok((i, qr)) => (i, qr),
            Err(e) => panic!("Wait failed: {:?}", e),
        };

        // Since this QToken completed, remove it from the list of waiters.
        waiters.swap_remove(index);

        // Find out what operation completed:
        match qr.qr_opcode {
            demi_opcode_t::DEMI_OPC_ACCEPT => {
                // A new connection arrived.
                let qd: QDesc = unsafe { qr.qr_value.ares.qd.into() };
                println!("Connection established!  Queue Descriptor = {:?}", qd);

                // Create new connection state and store it.
                let connection: Connection = Connection::new(qd);
                connections.insert(qd, connection);

                // Post a pop from the new connection.
                match libos.pop(qd) {
                    Ok(qt) => waiters.push(qt),
                    Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                };

                // Post an accept for the next connection.
                match libos.accept(listening_qd) {
                    Ok(qt) => waiters.push(qt),
                    Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
                }
            },
            demi_opcode_t::DEMI_OPC_POP => {
                // A pop completed.
                let qd: QDesc = qr.qr_qd.into();
                let recv_sga: demi_sgarray_t = unsafe { qr.qr_value.sga };

                // Find Connection for this queue descriptor.
                let connection: &mut Connection = connections.get_mut(&qd).expect("HashMap should hold connection!");

                // Process the incoming request.
                let oqt: Option<QToken> = match connection.process_data(&mut libos, recv_sga) {
                    Ok(oqt) => oqt,
                    Err(e) => panic!("process data failed: {:?}", e.cause),
                };

                if oqt.is_some() {
                    waiters.push(oqt.expect("oqt should be some!"));
                }

                // Post another pop.
                match libos.pop(qd) {
                    Ok(qt) => waiters.push(qt),
                    Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                }
            },
            demi_opcode_t::DEMI_OPC_PUSH => {
                // A push completed.
                let _qd: QDesc = qr.qr_qd.into();

                // ToDo: Can free the sga now.
            },
            demi_opcode_t::DEMI_OPC_FAILED => panic!("operation failed"),
            _ => panic!("unexpected opcode"),
        };
    }
}*/