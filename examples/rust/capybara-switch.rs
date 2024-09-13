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
use num_traits::ToBytes;

use std::{cell::RefCell, collections::{HashMap, HashSet, hash_map::Entry}, rc::Rc, time::Instant};
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
};
use ctrlc;

#[cfg(feature = "tcp-migration")]
use demikernel::{
    inetstack::protocols::tcpmig::ApplicationState,
    demikernel::bindings::demi_print_queue_length_log,
};

use ::demikernel::capy_time_log;

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;

use colored::Colorize;

#[macro_use]
extern crate lazy_static;
//=====================================================================================

macro_rules! server_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "capy-log")]
        if let Ok(val) = std::env::var("CAPY_LOG") {
            if val == "all" {
                eprintln!("{}", format!($($arg)*).green());
            }
        }
    };
}

fn capybara_switch(tcp_address: SocketAddrV4, udp_address: SocketAddrV4) -> Result<()> {
    ctrlc::set_handler(move || {
        eprintln!("Received Ctrl-C signal.");
        std::process::exit(0);
    }).expect("Error setting Ctrl-C handler");
    
    
    let libos_name: LibOSName = LibOSName::from_env().unwrap().into();
    let mut libos: LibOS = LibOS::new(libos_name).expect("intialized libos");
    
    libos.capybara_switch();

    eprintln!("capybara-switch stopping");

    LibOS::capylog_dump(&mut std::io::stderr().lock());

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
    println!("Usage: {} address\n", program_name);
}

//======================================================================================================================
// main()
//======================================================================================================================

pub fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 3 {
        let tcp_address = SocketAddrV4::from_str(&args[1]).expect("Invalid TCP address");
        let udp_address = SocketAddrV4::from_str(&args[2]).expect("Invalid UDP address");

        return capybara_switch(tcp_address, udp_address);
    }

    usage(&args[0]);

    Ok(())
}
