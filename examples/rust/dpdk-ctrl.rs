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
    runtime::logging,
};
use log::debug;
use std::collections::HashMap;
use ::std::{
    env,
    net::SocketAddrV4,
    panic,
    str::FromStr,
    thread, time::Duration,
};

#[cfg(feature = "profiler")]
use ::demikernel::perftools::profiler;
use clap::{App, Arg};
//======================================================================================================================
// server()
//======================================================================================================================


fn setup_dpdk() -> Result<()> {
    let libos_env_var = match env::var("LIBOS") {
        Ok(val) => val,
        Err(_) => panic!("LIBOS environment variable is not set"),
    };
    if libos_env_var != "catnip" {
        panic!("LibOS type {:?} is not supported", libos_env_var);
    }


    let libos_name: LibOSName = match LibOSName::from_env() {
        Ok(libos_name) => libos_name.into(),
        Err(e) => panic!("{:?}", e),
    };
    let mut libos: LibOS = match LibOS::new(libos_name) {
        Ok(libos) => libos,
        Err(e) => panic!("failed to initialize libos: {:?}", e.cause),
    };
    // let libos_name: LibOSName = match LibOSName::from_env() {
    //     Ok(LibOSName::Catnip) => LibOSName::Catnip,
    //     Ok(libos_name) => panic!("Unsupported libos: {:?}", env::var("LIBOS")),
    //     Err(e) => panic!("{:?}", e),
    // };
    loop {}

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
    logging::initialize();
    // let args: Vec<String> = env::args().collect();

    // let matches = App::new("My Program")
    //     .arg(
    //         Arg::with_name("num_cores")
    //             .short('n')
    //             .long("num_cores")
    //             .value_name("NUM")
    //             .help("Sets the number of cores")
    //             .takes_value(true)
    //             .required(true),
    //     )
    //     .get_matches();

    // let num_cores = matches
    //     .value_of("num_cores")
    //     .unwrap()
    //     .parse::<i32>()
    //     .unwrap();

    // println!("num_cores: {}", num_cores);
    
    return setup_dpdk();
    // if args.len() >= 2 {
    //     let n_cores: SocketAddrV4 = SocketAddrV4::from_str(&args[1])?;
    //     return server(sockaddr);
    // }

    // usage(&args[0]);

    Ok(())
}
