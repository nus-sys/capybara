//======================================================================================================================
// Constants
//======================================================================================================================
use std::{time::{Duration, Instant}};
use ::std::{
    net::{
        Ipv4Addr,
    },
};
use crate::{
    runtime::{
        network::{
            types::MacAddress,
        },
    }, 
};

//const BASE_RX_TX_THRESHOLD_RATIO: f64 = 2.0;
pub const BASE_RECV_QUEUE_LENGTH_THRESHOLD: f64 = 10.0;

// TEMP
pub const DEST_UDP_PORT: u16 = 10000;

// TEMP
pub const SELF_UDP_PORT: u16 = 10000; // it will be set properly when the socket is binded

pub const FRONTEND_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xe8, 0x05]);
pub const FRONTEND_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 8);
pub const FRONTEND_PORT: u16 = 10000;

pub const BACKEND_MAC: MacAddress = MacAddress::new([0x08, 0xc0, 0xeb, 0xb6, 0xc5, 0xad]);
pub const BACKEND_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 1, 9);

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_micros(1000);