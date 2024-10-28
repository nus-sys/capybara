// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#![cfg_attr(feature = "strict", deny(warnings))]
// #![deny(clippy::all)]

//==============================================================================
// Imports
//==============================================================================

use ::anyhow::Result;
use ::clap::{
    Arg,
    ArgMatches,
    Command,
};
use ::demikernel::{
    LibOS,
    LibOSName,
    OperationResult,
    QDesc,
    QToken,
};
use ::std::{
    net::SocketAddrV4,
    str::FromStr,
    time::{
        Duration,
        Instant,
    },
};

//==============================================================================
// Program Arguments
//==============================================================================

/// Program Arguments
#[derive(Debug)]
pub struct ProgramArguments {
    /// Local socket IPv4 address.
    local: SocketAddrV4,
}

/// Associate functions for Program Arguments
impl ProgramArguments {
    /// Default local socket IPv4 address.
    const DEFAULT_LOCAL: &'static str = "127.0.0.1:12345";

    /// Parses the program arguments from the command line interface.
    pub fn new(app_name: &str, app_author: &str, app_about: &str) -> Result<Self> {
        let matches: ArgMatches = Command::new(app_name)
            .author(app_author)
            .about(app_about)
            .arg(
                Arg::new("local")
                    .long("local")
                    .takes_value(true)
                    .required(false)
                    .value_name("ADDRESS:PORT")
                    .help("Sets local address"),
            )
            .get_matches();

        // Default arguments.
        let mut args: ProgramArguments = ProgramArguments {
            local: SocketAddrV4::from_str(Self::DEFAULT_LOCAL)?,
        };

        // Local address.
        if let Some(addr) = matches.value_of("local") {
            args.set_local_addr(addr)?;
        }

        Ok(args)
    }

    /// Returns the local endpoint address parameter stored in the target program arguments.
    pub fn get_local(&self) -> SocketAddrV4 {
        self.local
    }

    /// Sets the local address and port number parameters in the target program arguments.
    fn set_local_addr(&mut self, addr: &str) -> Result<()> {
        self.local = SocketAddrV4::from_str(addr)?;
        Ok(())
    }
}

//==============================================================================
// Application
//==============================================================================

/// Application
struct Application {
    /// Underlying libOS.
    libos: LibOS,
    // Local socket descriptor.
    sockqd: QDesc,
}

/// Associated Functions for the Application
impl Application {
    /// Logging interval (in seconds).
    const LOG_INTERVAL: u64 = 5;

    /// Instantiates the application.
    pub fn new(mut libos: LibOS, args: &ProgramArguments) -> Self {
        // Extract arguments.
        let local: SocketAddrV4 = args.get_local();

        // Create TCP socket.
        let sockqd: QDesc = match libos.socket(libc::AF_INET, libc::SOCK_STREAM, 0) {
            Ok(qd) => qd,
            Err(e) => panic!("failed to create socket: {:?}", e.cause),
        };

        // Bind to local address.
        match libos.bind(sockqd, local) {
            Ok(()) => (),
            Err(e) => panic!("failed to bind socket: {:?}", e.cause),
        };

        // Mark socket as a passive one.
        match libos.listen(sockqd, 16) {
            Ok(()) => (),
            Err(e) => panic!("failed to listen socket: {:?}", e.cause),
        }

        println!("Local Address: {:?}", local);

        Self { libos, sockqd }
    }

    /// Runs the target echo server.
    pub fn run(&mut self) -> ! {
        let start: Instant = Instant::now();
        let mut nbytes: usize = 0;
        let mut last_log: Instant = Instant::now();

        // Accept first connection.
        let qt: QToken = match self.libos.accept(self.sockqd) {
            Ok(qt) => qt,
            Err(e) => panic!("failed to accept connection on socket: {:?}", e.cause),
        };
        let (qd, mut qt): (QDesc, QToken) = match self.libos.wait2(qt) {
            Ok((_, OperationResult::Accept(qd))) => {
                println!("connection accepted!");
                // Pop first packet.
                let qt: QToken = match self.libos.pop(qd) {
                    Ok(qt) => qt,
                    Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
                };

                (qd, qt)
            },
            Err(e) => panic!("operation failed: {:?}", e.cause),
            _ => panic!("unexpected result"),
        };

        loop {
            // Dump statistics.
            if last_log.elapsed() > Duration::from_secs(Self::LOG_INTERVAL) {
                let elapsed: Duration = Instant::now() - start;
                println!("{:?} B / {:?} us", nbytes, elapsed.as_micros());
                last_log = Instant::now();
            }

            // Drain packets.
            match self.libos.wait2(qt) {
                Ok((_, OperationResult::Pop(_, buf))) => {
                    nbytes += buf.len();
                },
                Err(e) => panic!("operation failed: {:?}", e.cause),
                _ => panic!("unexpected result"),
            }
            qt = match self.libos.pop(qd) {
                Ok(qt) => qt,
                Err(e) => panic!("failed to pop data from socket: {:?}", e.cause),
            };
        }
    }
}

//==============================================================================

fn main() -> Result<()> {
    let args: ProgramArguments = ProgramArguments::new(
        "tcp-dump",
        "Pedro Henrique Penna <ppenna@microsoft.com>",
        "Dumps incoming packets on a TCP port.",
    )?;

    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };

    Application::new(libos, &args).run();
}
