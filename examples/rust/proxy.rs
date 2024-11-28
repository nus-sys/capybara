// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![feature(never_type)]
#![feature(extract_if)]
#![feature(hash_extract_if)]

//======================================================================================================================
// Imports
//======================================================================================================================

use ::anyhow::Result;
use ::demikernel::{
    demi_sgarray_t,
    runtime::types::{
        demi_opcode_t,
        demi_qresult_t,
    },
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use std::net::SocketAddrV4;
use ::std::{
    collections::HashMap,
    env,
    slice,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

//======================================================================================================================
// server()
//======================================================================================================================

struct TcpProxy {
    /// LibOS that handles incoming flow.
    incoming_libos: LibOS,
    /// LibOS that handles outgoing flow.
    outgoing_libos: LibOS,
    /// Number of clients that are currently connected.
    nclients: usize,
    /// Remote socket address.
    remote_addr: SocketAddrV4,
    /// Socket for accepting incoming connections.
    local_socket: QDesc,
    /// Queue descriptors of incoming connections.
    incoming_qds: HashMap<QDesc, bool>,
    /// Maps a queue descriptor of an incoming connection to its respective outgoing connection.
    incoming_qds_map: HashMap<QDesc, QDesc>,
    /// Incoming operations that are pending.
    incoming_qts: Vec<QToken>,
    /// Maps a pending incoming operation to its respective queue descriptor.
    incoming_qts_map: HashMap<QToken, QDesc>,
    /// Queue descriptors of outgoing connections.
    outgoing_qds: HashMap<QDesc, bool>,
    /// Maps a queue descriptor of an outgoing connection to its respective incoming connection.
    outgoing_qds_map: HashMap<QDesc, QDesc>,
    /// Outgoing operations that are pending.
    outgoing_qts: Vec<QToken>,
    /// Maps a pending outgoing operation to its respective queue descriptor.
    outgoing_qts_map: HashMap<QToken, QDesc>,
}

impl TcpProxy {
    /// Expected length for the array of pending incoming operations.
    /// It controls the pre-allocated size of the array.
    /// Change this value accordingly so as to avoid allocations on the datapath.
    const INCOMING_LENGTH: usize = 1024;
    /// Expected length for the array of pending outgoing operations.
    /// It controls the pre-allocated size of the array.
    /// Change this value accordingly so as to avoid allocations on the datapath.
    const OUTGOING_LENGTH: usize = 1024;

    /// Instantiates a TCP proxy that accepts incoming flows from `local_addr` and forwards them to `remote_addr`.
    pub fn new(local_addr: SocketAddrV4, remote_addr: SocketAddrV4) -> Result<Self> {
        // Retrieve LibOS name from environment variables.
        let libos_name: LibOSName = match LibOSName::from_env() {
            Ok(libos_name) => libos_name.into(),
            Err(e) => anyhow::bail!("{:?}", e),
        };

        // Instantiate LibOS for handling incoming flows.
        let mut incoming_libos: LibOS = match LibOS::new(LibOSName::Catnip) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos (error={:?})", e),
        };

        // Instantiate LibOS for handling outgoing flows.
        let outgoing_libos: LibOS = match LibOS::new(LibOSName::Catnap) {
            Ok(libos) => libos,
            Err(e) => anyhow::bail!("failed to initialize libos (error={:?})", e),
        };

        // Setup local socket.
        let local_socket: QDesc = Self::setup_local_socket(&mut incoming_libos, local_addr)?;

        Ok(Self {
            incoming_libos,
            outgoing_libos,
            nclients: 0,
            remote_addr,
            local_socket,
            incoming_qts: Vec::with_capacity(Self::INCOMING_LENGTH),
            incoming_qts_map: HashMap::default(),
            outgoing_qts: Vec::with_capacity(Self::OUTGOING_LENGTH),
            outgoing_qts_map: HashMap::default(),
            incoming_qds: HashMap::default(),
            incoming_qds_map: HashMap::default(),
            outgoing_qds: HashMap::default(),
            outgoing_qds_map: (HashMap::default()),
        })
    }

    /// Runs the target TCP proxy.
    pub fn run(&mut self) -> Result<!> {
        ctrlc::set_handler(move || {
            eprintln!("Received Ctrl-C signal.");
            std::process::exit(0);
        }).expect("Error setting Ctrl-C handler");

        // Time interval for dumping logs and statistics.
        // This was arbitrarily set, but keep in mind that too short intervals may negatively impact performance.
        let log_interval: Option<Duration> = Some(Duration::from_secs(1));
        // Time stamp when last log was dumped.
        let mut last_log: Instant = Instant::now();
        // Timeout for polling incoming operations.This was intentionally set to zero to force no waiting.
        let timeout_incoming: Option<Duration> = Some(Duration::from_secs(0));
        // Timeout for polling outgoing operations. This was intentionally set to zero to force no waiting.
        let timeout_outgoing: Option<Duration> = Some(Duration::from_secs(0));

        // Accept incoming connections.
        self.issue_accept()?;

        // Create qrs filled with garbage.
        let mut qrs: Vec<(QDesc, OperationResult)> = Vec::with_capacity(2000);
        qrs.resize_with(2000, || (0.into(), OperationResult::Connect));
        let mut indices: Vec<usize> = Vec::with_capacity(2000);
        indices.resize(2000, 0);
        let mut migration_counter: i32 = 0;
        let mut is_migration_started: bool = false;
        loop {
            // Dump statistics.
            if let Some(log_interval) = log_interval {
                if last_log.elapsed() > log_interval {
                    // println!("INFO: {:?} clients connected", self.nclients);
                    last_log = Instant::now();
                }
            }

            // eprintln!("Incoming wait_any2()");
            // loop {
            let result_count = self.incoming_libos.wait_any2(&self.incoming_qts, &mut qrs, &mut indices, timeout_incoming).expect("result");
            // if(result_count == 0) {
            //     break;
            // }
            let results = &qrs[..result_count];
            let completed_indices = &indices[..result_count];
            
            for (index, (qd, result)) in completed_indices.iter().zip(results.iter()).rev() {
                let (index, qd) = (*index, *qd);
                
                let qt: QToken = self.incoming_qts.remove(index);
                self.incoming_qts_map
                    .remove(&qt)
                    .expect("queue token should be registered");
                match result {
                    OperationResult::Accept(new_qd) => {
                        self.handle_incoming_accept(*new_qd)?;
                    },
                    OperationResult::Pop(_, recvbuf) => {
                        migration_counter += 1;
                        if  self.remote_addr.ip().octets()[3] != 9 {
                            is_migration_started = (migration_counter > 400000);
                        }
                        // eprintln!("migration_counter: {}", migration_counter);
                        // eprintln!("incoming_qds: {:?}", self.incoming_qds);
                        
                        self.handle_incoming_pop(qd, &recvbuf, is_migration_started);

                        
                    },
                    OperationResult::Push => {
                        self.handle_incoming_push(qd); 
                        #[cfg(feature = "tcp-migration")]
                        if  self.remote_addr.ip().octets()[3] != 9 {
                            if *self.incoming_qds.get(&qd).unwrap() == false {
                                self.incoming_libos.initiate_migration(qd).unwrap();
                            }
                        }
                    
                    },
                    _ => {
                        eprintln!("Unexpected result: {:?}", result);
                    },
                }

            }
            // }    
            // eprintln!("Outgoing wait_any2()");
            let result_count = self.outgoing_libos.wait_any2(&self.outgoing_qts, &mut qrs, &mut indices, timeout_outgoing).expect("result");

            let results = &qrs[..result_count];
            let completed_indices = &indices[..result_count];
            
            for (index, (qd, result)) in completed_indices.iter().zip(results.iter()).rev() {
                let (index, qd) = (*index, *qd);
                
                let qt: QToken = self.outgoing_qts.remove(index);
                self.outgoing_qts_map
                    .remove(&qt)
                    .expect("queue token should be registered");
                match result {
                    OperationResult::Connect => {
                        self.handle_outgoing_connect(qd);
                    },
                    OperationResult::Pop(_, recvbuf) => self.handle_outgoing_pop(qd, &recvbuf),
                    OperationResult::Push => self.handle_outgoing_push(qd),
                    _ => unreachable!(),
                }

            }
        }
    }

    /// Registers an incoming operation that is waiting for completion (pending).
    /// This function fails if the operation is already registered in the table of pending incoming operations.
    fn register_incoming_operation(&mut self, qd: QDesc, qt: QToken) -> Result<()> {
        if self.incoming_qts_map.insert(qt, qd).is_some() {
            anyhow::bail!("incoming operation is already registered (qt={:?})", qt);
        }
        self.incoming_qts.push(qt);
        Ok(())
    }

    /// Registers an outgoing operation that is waiting for completion (pending).
    /// This function fails if the operation is already registered in the table of pending outgoing operations.
    fn register_outgoing_operation(&mut self, qd: QDesc, qt: QToken) -> Result<()> {
        // eprintln!("Register outgoing qtoken: {:?}", qt);
        if self.outgoing_qts_map.insert(qt, qd).is_some() {
            anyhow::bail!("outgoing operation is already registered (qt={:?})", qt);
        }
        self.outgoing_qts.push(qt);
        Ok(())
    }

    /// Issues an `accept()`operation.
    /// This function fails if the underlying `accept()` operation fails.
    fn issue_accept(&mut self) -> Result<()> {
        let qt: QToken = self.incoming_libos.accept(self.local_socket)?;
        self.register_incoming_operation(self.local_socket, qt)?;
        Ok(())
    }

    /// Issues a `push()` operation in an incoming flow.
    /// This function fails if the underlying `push()` operation fails.
    fn issue_incoming_push(&mut self, qd: QDesc, data: &[u8]) -> Result<()> {
        let qt: QToken = self.incoming_libos.push2(qd, data)?;

        // It is safe to call except() here, because we just issued the `push()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_incoming_operation(qd, qt)
            .expect("incoming push() operration is already registered");

        Ok(())
    }

    /// Issues a `pop()` operation in an incoming flow.
    /// This function fails if the underlying `pop()` operation fails.
    fn issue_incoming_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.incoming_libos.pop(qd)?;

        // It is safe to call except() here, because we just issued the `pop()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_incoming_operation(qd, qt)
            .expect("incoming pop() operration is already registered");

        // Set the flag to indicate that this flow has an inflight `pop()` operation.
        // It is safe to call except() here, because `qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let incoming_inflight_pop: &mut bool = self
            .incoming_qds
            .get_mut(&qd)
            .expect("queue descriptor should be registered");
        *incoming_inflight_pop = true;

        Ok(())
    }

    /// Issues a `push()` operation in an outgoing flow.
    /// This function fails if the underlying `push()` operation fails.
    fn issue_outgoing_push(&mut self, qd: QDesc, data: &[u8]) -> Result<()> {
        // eprintln!("issue_outgoing_push qd: {:?}, data: {:?}", qd, data);
        let qt: QToken = self.outgoing_libos.push2(qd, data)?;

        // It is safe to call except() here, because we just issued the `push()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_outgoing_operation(qd, qt)
            .expect("outgoing push() operration is already registered");

        Ok(())
    }

    /// Issues a `pop()` operation in an outgoing flow.
    /// This function fails if the underlying `pop()` operation fails.
    fn issue_outgoing_pop(&mut self, qd: QDesc) -> Result<()> {
        let qt: QToken = self.outgoing_libos.pop(qd)?;

        // It is safe to call except() here, because we just issued the `pop()` operation,
        // queue tokens are unique, and thus the operation is ensured to not be registered.
        self.register_outgoing_operation(qd, qt)
            .expect("outgoing pop() operration is already registered");

        // Set the flag to indicate that this flow has an inflight `pop()` operation.
        // It is safe to call except() here, because `qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let outgoing_inflight_pop: &mut bool = self
            .outgoing_qds
            .get_mut(&qd)
            .expect("queue descriptor should be registered");
        *outgoing_inflight_pop = true;

        Ok(())
    }

    /// Handles the completion of an `accept()` operation.
    /// This function fails if we we fail to setup a connection with the remote address.
    fn handle_incoming_accept(&mut self, new_client_socket: QDesc) -> Result<()> {
        // Setup remote connection.
        let new_server_socket: QDesc = match self.outgoing_libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        // Connect to remote address.
        // eprintln!("ACCEPT => connecting to {} at {:?}", self.remote_addr, std::time::SystemTime::now());
        match self.outgoing_libos.connect(new_server_socket, self.remote_addr) {
            // Operation succeeded, register outgoing operation.
            Ok(qt) => self.register_outgoing_operation(new_server_socket, qt)?,
            // Operation failed, close socket.
            Err(e) => {
                if let Err(e) = self.outgoing_libos.close(new_server_socket) {
                    // Failed to close socket, log error.
                    println!("ERROR: close failed (error={:?})", e);
                    println!("WARN: leaking socket descriptor (sockqd={:?})", new_server_socket);
                }
                anyhow::bail!("failed to connect socket: {:?}", e)
            },
        };

        // Accept another connection.
        if let Err(e) = self.issue_accept() {
            // Failed to issue accept operation, log error.
            println!("ERROR: accept failed (error={:?})", e);
        };

        self.incoming_qds.insert(new_client_socket, false);
        self.incoming_qds_map.insert(new_server_socket, new_client_socket);
        self.outgoing_qds.insert(new_server_socket, false);
        self.outgoing_qds_map.insert(new_client_socket, new_server_socket);

        Ok(())
    }

    /// Handles the completion of a `connect()` operation.
    fn handle_outgoing_connect(&mut self, outgoing_qd: QDesc) {
        // eprintln!("handle_outgoing_connect qd {:?} at {:?}", outgoing_qd, std::time::SystemTime::now());

        // It is safe to call expect() here, because `outgoing_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let incoming_qd: QDesc = *self
            .incoming_qds_map
            .get(&outgoing_qd)
            .expect("queue descriptor should be registered");

        // Issue a `pop()` operation in the incoming flow.
        if let Err(e) = self.issue_incoming_pop(incoming_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }

        self.nclients += 1;
        // println!("INFO: {:?} clients connected", self.nclients);
    }

    /// Handles the completion of a `pop()` operation on an incoming flow.
    fn handle_incoming_pop(&mut self, incoming_qd: QDesc, data: &[u8], is_migration_started: bool) {
        // eprintln!("Received Request");

        // It is safe to call expect() here, because `outgoing_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let outgoing_qd: QDesc = *self
            .outgoing_qds_map
            .get(&incoming_qd)
            .expect("queue descriptor should be registered");

        // Check if client closed connection.
        if data.len() == 0 {
            // println!("INFO: client closed connection");
            self.close_client(incoming_qd, outgoing_qd);
            return;
        }
        
        // Issue `push()` operation.
        if let Err(e) = self.issue_outgoing_push(outgoing_qd, data) {
            // Failed to issue push operation, log error.
            println!("ERROR: push failed (error={:?})", e);
        }

        if is_migration_started {
            let incoming_inflight_pop: &mut bool = self
                .incoming_qds
                .get_mut(&incoming_qd)
                .expect("queue descriptor should be registered");
            *incoming_inflight_pop = false;
            return;
        }

        // Pop more data from incoming flow.
        if let Err(e) = self.issue_incoming_pop(incoming_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }
    }

    /// Handles the completion of a `pop()` operation on an outgoing flow.
    fn handle_outgoing_pop(&mut self, outgoing_qd: QDesc, data: &[u8]) {
        // eprintln!("handle_outgoing_pop");
        
        // It is safe to call expect() here, because `outgoing_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let incoming_qd: QDesc = *self
            .incoming_qds_map
            .get(&outgoing_qd)
            .expect("queue descriptor should be registered");

        // Check if server aborted connection.
        if data.len() == 0 {
            unimplemented!("server aborted connection");
        }

        // Issue `push()` operation.
        if let Err(e) = self.issue_incoming_push(incoming_qd, data) {
            // Failed to issue push operation, log error.
            println!("ERROR: push failed (error={:?})", e);
        }

        // Pop data from outgoing flow.
        if let Err(e) = self.issue_outgoing_pop(outgoing_qd) {
            // Failed to issue pop operation, log error.
            println!("ERROR: pop failed (error={:?})", e);
        }

    }

    /// Handles the completion of a `push()` operation on an incoming flow.
    /// This will issue a pop operation on the incoming connection, if none is inflight.
    fn handle_incoming_push(&mut self, incoming_qd: QDesc) {
        // eprintln!("Sent Reply");

        // It is safe to call except() here, because `incoming_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        // let has_incoming_inflight_pop: bool = self
        //     .incoming_qds
        //     .get_mut(&incoming_qd)
        //     .expect("queue descriptor should be registered")
        //     .to_owned();

        // // Issue a pop operation if none is inflight.
        // if !has_incoming_inflight_pop {
        //     unreachable!("should have an incoming pop, but it hasn't (qd={:?})", incoming_qd);
        // }
    }

    /// Handles the completion of a `push()` operation on an outgoing flow.
    /// This will issue a pop operation on the outgoing connection, if none is inflight.
    fn handle_outgoing_push(&mut self, outgoing_qd: QDesc) {
        // eprintln!("handle_outgoing_push");
        
        // It is safe to call except() here, because `outgoing_qd` is ensured to be in the table of queue descriptors.
        // All queue descriptors are registered when connection is established.
        let has_inflight_pop: bool = self
            .outgoing_qds
            .get_mut(&outgoing_qd)
            .expect("queue descriptor should be registered")
            .to_owned();

        // Issue a pop operation if none is inflight.
        if !has_inflight_pop {
            // println!("INFO: issuing outgoing pop (qd={:?})", outgoing_qd);
            if let Err(e) = self.issue_outgoing_pop(outgoing_qd) {
                // Failed to issue pop operation, log error.
                println!("ERROR: pop failed (error={:?})", e);
            }
        }
    }

    // Closes an incoming flow.
    fn close_client(&mut self, incoming_socket: QDesc, outgoing_socket: QDesc) {
        match self.incoming_libos.close(incoming_socket) {
            Ok(_) => {
                println!("handle cancellation of tokens (incoming_socket={:?})", incoming_socket);
                self.incoming_qds.remove(&incoming_socket).unwrap();
                self.outgoing_qds_map.remove(&incoming_socket).unwrap();
                let qts_drained: HashMap<QToken, QDesc> = self.incoming_qts_map.extract_if(|_k, v| v == &incoming_socket).collect();
                let _: Vec<_> = self.incoming_qts.extract_if(|x| qts_drained.contains_key(x)).collect();
            },
            Err(e) => println!("ERROR: failed to close socket (error={:?})", e),
        }

        match self.outgoing_libos.close(outgoing_socket) {
            Ok(_) => {
                println!("handle cancellation of tokens (outgoing_socket={:?})", outgoing_socket);
                self.outgoing_qds.remove(&outgoing_socket).unwrap();
                self.incoming_qds_map.remove(&outgoing_socket).unwrap();
                let qts_drained: HashMap<QToken, QDesc> = self.outgoing_qts_map.extract_if(|_k, v| v == &outgoing_socket).collect();
                let _: Vec<_> = self.outgoing_qts.extract_if(|x| qts_drained.contains_key(x)).collect();
            },
            Err(e) => println!("ERROR: failed to close socket (error={:?})", e),
        }
        self.nclients -= 1;
    }


    /// Copies `len` bytes from `src` to `dest`.
    fn copy(src: *mut libc::c_uchar, dest: *mut libc::c_uchar, len: usize) {
        let src: &mut [u8] = unsafe { slice::from_raw_parts_mut(src, len) };
        let dest: &mut [u8] = unsafe { slice::from_raw_parts_mut(dest, len) };
        dest.clone_from_slice(src);
    }

    /// Setups local socket.
    fn setup_local_socket(incoming_libos: &mut LibOS, local_addr: SocketAddrV4) -> Result<QDesc> {
        // Create local socket.
        let local_socket: QDesc = match incoming_libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => anyhow::bail!("failed to create socket: {:?}", e.cause),
        };

        // Bind socket to local address.
        if let Err(e) = incoming_libos.bind(local_socket, local_addr) {
            // Bind failed, close socket.
            if let Err(e) = incoming_libos.close(local_socket) {
                // Close failed, log error.
                println!("ERROR: close failed (error={:?})", e);
                println!("WARN: leaking socket descriptor (sockqd={:?})", local_socket);
            }
            anyhow::bail!("bind failed: {:?}", e.cause)
        };

        // Enable socket to accept incoming connections.
        if let Err(e) = incoming_libos.listen(local_socket, 16) {
            // Listen failed, close socket.
            if let Err(e) = incoming_libos.close(local_socket) {
                // Close failed, log error.
                println!("ERROR: close failed (error={:?})", e);
                println!("WARN: leaking socket descriptor (sockqd={:?})", local_socket);
            }
            anyhow::bail!("listen failed: {:?}", e.cause)
        }

        Ok(local_socket)
    }
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    // Check command line arguments.
    if args.len() < 3 {
        println!("Usage: {} local-address remote-address\n", &args[0]);
        return Ok(());
    }

    let local_addr: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
    let remote_addr: SocketAddrV4 = SocketAddrV4::from_str(&args[2])?;
    let mut proxy: TcpProxy = TcpProxy::new(local_addr, remote_addr)?;
    proxy.run()?;
}