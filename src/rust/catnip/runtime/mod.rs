// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

pub mod memory;
mod network;

//==============================================================================
// Imports
//==============================================================================

use self::memory::{
    consts::DEFAULT_MAX_BODY_SIZE,
    MemoryManager,
};
use crate::runtime::{
    libdpdk::{
        rte_delay_us_block,
        rte_eal_init,
        rte_eth_conf,
        rte_eth_dev_configure,
        rte_eth_dev_count_avail,
        rte_eth_dev_get_mtu,
        rte_eth_dev_info_get,
        rte_eth_dev_is_valid_port,
        rte_eth_dev_set_mtu,
        rte_eth_dev_start,
        rte_eth_find_next_owned_by,
        rte_eth_link,
        rte_eth_link_get_nowait,
        rte_eth_macaddr_get,
        rte_eth_promiscuous_enable,
        rte_eth_rx_mq_mode_ETH_MQ_RX_RSS as ETH_MQ_RX_RSS,
        rte_eth_rx_queue_setup,
        rte_eth_rxconf,
        rte_eth_tx_mq_mode_ETH_MQ_TX_NONE as ETH_MQ_TX_NONE,
        rte_eth_tx_queue_setup,
        rte_eth_txconf,
        rte_ether_addr,
        DEV_RX_OFFLOAD_JUMBO_FRAME,
        DEV_RX_OFFLOAD_TCP_CKSUM,
        DEV_RX_OFFLOAD_UDP_CKSUM,
        DEV_TX_OFFLOAD_MULTI_SEGS,
        DEV_TX_OFFLOAD_TCP_CKSUM,
        DEV_TX_OFFLOAD_UDP_CKSUM,
        ETH_LINK_FULL_DUPLEX,
        ETH_LINK_UP,
        ETH_RSS_IP,
        RTE_ETHER_MAX_JUMBO_FRAME_LEN,
        RTE_ETHER_MAX_LEN,
        RTE_ETH_DEV_NO_OWNER,
        RTE_PKTMBUF_HEADROOM,
        rte_eal_process_type,
        rte_flow_error,
        rte_flow_attr,
        rte_flow_item,
        rte_flow_item_type_RTE_FLOW_ITEM_TYPE_ETH,
        rte_flow_item_type_RTE_FLOW_ITEM_TYPE_IPV4,
        rte_flow_item_type_RTE_FLOW_ITEM_TYPE_TCP,
        rte_flow_item_type_RTE_FLOW_ITEM_TYPE_UDP,
        rte_tcp_hdr,
        rte_udp_hdr,
        rte_ether_hdr,
        rte_flow_item_type_RTE_FLOW_ITEM_TYPE_END,
        rte_flow_action,
        rte_flow_validate,
        rte_flow_create,
        rte_flow_action_queue,
        rte_flow_action_type_RTE_FLOW_ACTION_TYPE_END,
        rte_flow_action_type_RTE_FLOW_ACTION_TYPE_QUEUE,
        rte_flow_action_type_RTE_FLOW_ACTION_TYPE_DROP,
        ETH_RSS_NONFRAG_IPV4_TCP,
        RTE_ETHER_TYPE_IPV4,
        rte_ipv4_hdr,
    },
    network::{
        config::{
            ArpConfig,
            TcpConfig,
            UdpConfig,
        },
        types::MacAddress,
    },
    Runtime,
};
use ::anyhow::{
    bail,
    format_err,
    Error,
};
use ::std::{
    collections::HashMap,
    ffi::CString,
    mem::MaybeUninit,
    net::Ipv4Addr,
    time::Duration,
    mem,
};

//==============================================================================
// Macros
//==============================================================================

macro_rules! expect_zero {
    ($name:ident ( $($arg: expr),* $(,)* )) => {{
        let ret = $name($($arg),*);
        if ret == 0 {
            Ok(0)
        } else {
            Err(format_err!("{} failed with {:?}", stringify!($name), ret))
        }
    }};
}

//==============================================================================
// Structures
//==============================================================================

/// DPDK Runtime
#[derive(Clone)]
pub struct DPDKRuntime {
    mm: MemoryManager,
    port_id: u16,
    queue_id: u16,
    pub link_addr: MacAddress,
    pub ipv4_addr: Ipv4Addr,
    pub arp_options: ArpConfig,
    pub tcp_options: TcpConfig,
    pub udp_options: UdpConfig,
}

//==============================================================================
// Associate Functions
//==============================================================================

/// Associate Functions for DPDK Runtime
impl DPDKRuntime {
    // Constants
    const FLOW_IPV4_PROTO_UDP: u8 = 0x11;
    const FLOW_IPV4_PROTO_TCP: u8 = 0x06;

    pub fn new(
        ipv4_addr: Ipv4Addr,
        eal_init_args: &[CString],
        arp_table: HashMap<Ipv4Addr, MacAddress>,
        disable_arp: bool,
        use_jumbo_frames: bool,
        mtu: u16,
        mss: usize,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> DPDKRuntime {
        let mut l_flag_value: Option<u16> = None;
        for (i, arg) in eal_init_args.iter().enumerate() {
            if arg.to_str().unwrap() == "-l" {
                if let Some(value) = eal_init_args.get(i + 1) {
                    if let Ok(u) = value.to_str().unwrap().parse::<u16>() {
                        l_flag_value = Some(u);
                    }
                }
            }
        }

        if let Some(value) = l_flag_value {
            // use the value here
            println!("The value for -l flag is: {}", value);
        }

        let core_id: u16 = match l_flag_value {
            Some(val) => val,
            None => panic!("-l flag is not set in config"),
        };
        println!("core_id: {}", core_id); 
        let (mm, port_id, link_addr) = Self::initialize_dpdk(
            eal_init_args,
            use_jumbo_frames,
            mtu,
            tcp_checksum_offload,
            udp_checksum_offload,
        )
        .unwrap();

        let arp_options = ArpConfig::new(
            Some(Duration::from_secs(15)),
            Some(Duration::from_secs(20)),
            Some(5),
            Some(arp_table),
            Some(disable_arp),
        );

        let tcp_options = TcpConfig::new(
            Some(mss),
            None,
            None,
            Some(0xffff),
            Some(0),
            None,
            Some(tcp_checksum_offload),
            Some(tcp_checksum_offload),
        );

        let udp_options = UdpConfig::new(Some(udp_checksum_offload), Some(udp_checksum_offload));

        Self {
            mm,
            port_id,
            queue_id: core_id-1, // We are using from core 1 (not core 0), so (queue_id == core_id - 1)
            link_addr,
            ipv4_addr,
            arp_options,
            tcp_options,
            udp_options,
        }
    }

    /// Initializes DPDK.
    fn initialize_dpdk(
        eal_init_args: &[CString],
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(MemoryManager, u16, MacAddress), Error> {
        std::env::set_var("MLX5_SHUT_UP_BF", "1");
        std::env::set_var("MLX5_SINGLE_THREADED", "1");
        std::env::set_var("MLX4_SINGLE_THREADED", "1");
        
        
        let eal_init_refs = eal_init_args.iter().map(|s| s.as_ptr() as *mut u8).collect::<Vec<_>>();
        let ret: libc::c_int = unsafe { rte_eal_init(eal_init_refs.len() as i32, eal_init_refs.as_ptr() as *mut _) };
        if ret < 0 {
            let rte_errno: libc::c_int = unsafe { dpdk_rs::rte_errno() };
            bail!("EAL initialization failed (rte_errno={:?})", rte_errno);
        }
        let nb_ports: u16 = unsafe { rte_eth_dev_count_avail() };
        if nb_ports == 0 {
            bail!("No ethernet ports available");
        }
        eprintln!("DPDK reports that {} ports (interfaces) are available.", nb_ports);
        
        let proc_type = unsafe {rte_eal_process_type()};
        let max_body_size: usize = if use_jumbo_frames {
            (RTE_ETHER_MAX_JUMBO_FRAME_LEN + RTE_PKTMBUF_HEADROOM) as usize
        } else {
            DEFAULT_MAX_BODY_SIZE
        };

        let memory_manager = MemoryManager::new(max_body_size)?;

        let owner: u64 = RTE_ETH_DEV_NO_OWNER as u64;
        let port_id: u16 = unsafe { rte_eth_find_next_owned_by(0, owner) as u16 };
        
        match proc_type {
            0 => {
                Self::initialize_dpdk_port(
                    port_id,
                    &memory_manager,
                    use_jumbo_frames,
                    mtu,
                    tcp_checksum_offload,
                    udp_checksum_offload,
                )?;
                let num_cores: u16 = match std::env::var("NUM_CORES") {
                    Ok(val) => val.parse::<u16>().unwrap(),
                    Err(_) => panic!("NUM_CORES environment variable is not set"),
                };
                println!("num_cores: {}", num_cores);
                Self::init_flow_rules();
                Self::generate_tcp_flow_rules(num_cores);
                Self::generate_udp_flow_rules(num_cores);
            },
            _ => ()
        }

        // TODO: Where is this function?
        // if unsafe { rte_lcore_count() } > 1 {
        //     eprintln!("WARNING: Too many lcores enabled. Only 1 used.");
        // }

        let local_link_addr: MacAddress = unsafe {
            let mut m: MaybeUninit<rte_ether_addr> = MaybeUninit::zeroed();
            // TODO: Why does bindgen say this function doesn't return an int?
            rte_eth_macaddr_get(port_id, m.as_mut_ptr());
            MacAddress::new(m.assume_init().addr_bytes)
        };
        if local_link_addr.is_nil() || !local_link_addr.is_unicast() {
            Err(format_err!("Invalid mac address"))?;
        }

        Ok((memory_manager, port_id, local_link_addr))
    }

    /// Initializes a DPDK port.
    fn initialize_dpdk_port(
        port_id: u16,
        memory_manager: &MemoryManager,
        use_jumbo_frames: bool,
        mtu: u16,
        tcp_checksum_offload: bool,
        udp_checksum_offload: bool,
    ) -> Result<(), Error> {
        let num_cores: u16 = match std::env::var("NUM_CORES") {
            Ok(val) => val.parse::<u16>().unwrap(),
            Err(_) => panic!("NUM_CORES environment variable is not set"),
        };
        let rx_rings = num_cores * 2;
        let tx_rings = num_cores * 2;
        let rx_ring_size = 2048;
        let tx_ring_size = 2048;
        let nb_rxd = rx_ring_size;
        let nb_txd = tx_ring_size;

        let rx_pthresh = 8;
        let rx_hthresh = 8;
        let rx_wthresh = 0;

        let tx_pthresh = 0;
        let tx_hthresh = 0;
        let tx_wthresh = 0;

        let dev_info = unsafe {
            let mut d = MaybeUninit::zeroed();
            rte_eth_dev_info_get(port_id, d.as_mut_ptr());
            d.assume_init()
        };

        println!("dev_info: {:?}", dev_info);
        unsafe {
            expect_zero!(rte_eth_dev_set_mtu(port_id, mtu))?;
            let mut dpdk_mtu = 0u16;
            expect_zero!(rte_eth_dev_get_mtu(port_id, &mut dpdk_mtu as *mut _))?;
            if dpdk_mtu != mtu {
                bail!("Failed to set MTU to {}, got back {}", mtu, dpdk_mtu);
            }
        }

        let mut port_conf: rte_eth_conf = unsafe { MaybeUninit::zeroed().assume_init() };
        port_conf.rxmode.max_rx_pkt_len = if use_jumbo_frames {
            RTE_ETHER_MAX_JUMBO_FRAME_LEN
        } else {
            RTE_ETHER_MAX_LEN
        };
        if tcp_checksum_offload {
            port_conf.rxmode.offloads |= DEV_RX_OFFLOAD_TCP_CKSUM as u64;
        }
        if udp_checksum_offload {
            port_conf.rxmode.offloads |= DEV_RX_OFFLOAD_UDP_CKSUM as u64;
        }
        if use_jumbo_frames {
            port_conf.rxmode.offloads |= DEV_RX_OFFLOAD_JUMBO_FRAME as u64;
        }
        port_conf.rxmode.mq_mode = ETH_MQ_RX_RSS;
        port_conf.rx_adv_conf.rss_conf.rss_hf = ETH_RSS_NONFRAG_IPV4_TCP as u64;

        port_conf.txmode.mq_mode = ETH_MQ_TX_NONE;
        if tcp_checksum_offload {
            port_conf.txmode.offloads |= DEV_TX_OFFLOAD_TCP_CKSUM as u64;
        }
        if udp_checksum_offload {
            port_conf.txmode.offloads |= DEV_TX_OFFLOAD_UDP_CKSUM as u64;
        }
        port_conf.txmode.offloads |= DEV_TX_OFFLOAD_MULTI_SEGS as u64;

        let mut rx_conf: rte_eth_rxconf = unsafe { MaybeUninit::zeroed().assume_init() };
        rx_conf.rx_thresh.pthresh = rx_pthresh;
        rx_conf.rx_thresh.hthresh = rx_hthresh;
        rx_conf.rx_thresh.wthresh = rx_wthresh;
        rx_conf.rx_free_thresh = 32;

        let mut tx_conf: rte_eth_txconf = unsafe { MaybeUninit::zeroed().assume_init() };
        tx_conf.tx_thresh.pthresh = tx_pthresh;
        tx_conf.tx_thresh.hthresh = tx_hthresh;
        tx_conf.tx_thresh.wthresh = tx_wthresh;
        tx_conf.tx_free_thresh = 32;

        unsafe {
            expect_zero!(rte_eth_dev_configure(
                port_id,
                rx_rings,
                tx_rings,
                &port_conf as *const _,
            ))?;
        }

        let socket_id = 0;

        unsafe {
            for i in 0..rx_rings {
                expect_zero!(rte_eth_rx_queue_setup(
                    port_id,
                    i,
                    nb_rxd,
                    socket_id,
                    &rx_conf as *const _,
                    memory_manager.body_pool(),
                ))?;
            }
            for i in 0..tx_rings {
                expect_zero!(rte_eth_tx_queue_setup(
                    port_id,
                    i,
                    nb_txd,
                    socket_id,
                    &tx_conf as *const _
                ))?;
            }
            expect_zero!(rte_eth_dev_start(port_id))?;
            rte_eth_promiscuous_enable(port_id);
        }

        if unsafe { rte_eth_dev_is_valid_port(port_id) } == 0 {
            bail!("Invalid port");
        }

        let sleep_duration = Duration::from_millis(100);
        let mut retry_count = 90;

        loop {
            unsafe {
                let mut link: MaybeUninit<rte_eth_link> = MaybeUninit::zeroed();
                rte_eth_link_get_nowait(port_id, link.as_mut_ptr());
                let link = link.assume_init();
                if link.link_status() as u32 == ETH_LINK_UP {
                    let duplex = if link.link_duplex() as u32 == ETH_LINK_FULL_DUPLEX {
                        "full"
                    } else {
                        "half"
                    };
                    eprintln!(
                        "Port {} Link Up - speed {} Mbps - {} duplex",
                        port_id, link.link_speed, duplex
                    );
                    break;
                }
                rte_delay_us_block(sleep_duration.as_micros() as u32);
            }
            if retry_count == 0 {
                bail!("Link never came up");
            }
            retry_count -= 1;
        }

        Ok(())
    }
    fn init_flow_rules() {
        unsafe {
            
            let mut err: rte_flow_error = mem::zeroed();
            let mut attr: rte_flow_attr = mem::zeroed();
            attr.priority = 1; // FLOW_DEFAULT_PRIORITY: 1
            // attr.set_egress(0);
            attr.set_ingress(1);
            
            let mut pattern: Vec<rte_flow_item> = vec![mem::zeroed(); 2];
            let mut eth_spec: rte_ether_hdr = mem::zeroed();
            let mut eth_mask: rte_ether_hdr = mem::zeroed();
            pattern[0].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_ETH;
            pattern[0].spec = &mut eth_spec as *mut _ as *mut std::os::raw::c_void;
            pattern[0].mask = &mut eth_mask as *mut _ as *mut std::os::raw::c_void;
            pattern[1].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_END;
            
            let mut action: Vec<rte_flow_action> = vec![mem::zeroed(); 2];
            action[0].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_DROP;
            action[1].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_END;

            

            let error = rte_flow_validate(0, &attr, pattern.as_ptr(), action.as_ptr(), &mut err);
            let flow = rte_flow_create(0, &attr, pattern.as_ptr(), action.as_ptr(), &mut err);
            if error != 0 {
                panic!("Default flow rule is not valid, code {:?}", error);
            }
            if flow.is_null() {
                panic!("rte_flow_create failed");
            }
            println!("Default flow (dropping) created: {:?}", flow);
        }
    }

    
    fn generate_tcp_flow_rules(nr_queues: u16) {
        unsafe{
        for i in 0..nr_queues {
            let port: u16 = i + 10000;
            let mut err: rte_flow_error = mem::zeroed();
            let mut attr: rte_flow_attr = mem::zeroed();
            attr.priority = 0; // FLOW_TRANSPORT_PRIORITY: 0
            attr.set_egress(0);
            attr.set_ingress(1);
        
            let mut eth_spec: rte_ether_hdr = mem::zeroed();
            let mut eth_mask: rte_ether_hdr = mem::zeroed();
            eth_spec.ether_type = u16::to_be(RTE_ETHER_TYPE_IPV4 as u16);
            eth_mask.ether_type = u16::MAX;
        
            let mut ip_spec_tcp: rte_ipv4_hdr = mem::zeroed();
            let mut ip_spec_tcp: rte_ipv4_hdr = mem::zeroed();
            let mut ip_mask: rte_ipv4_hdr = mem::zeroed();
            ip_spec_tcp.next_proto_id = u8::to_be(Self::FLOW_IPV4_PROTO_TCP as u8);
            ip_spec_tcp.next_proto_id = u8::to_be(Self::FLOW_IPV4_PROTO_TCP as u8);
            ip_mask.next_proto_id = u8::MAX;
        
        
            let mut tcp_pattern: Vec<rte_flow_item> = vec![mem::zeroed(); 4];
            tcp_pattern[0].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_ETH;
            tcp_pattern[0].spec = &mut eth_spec as *mut _ as *mut std::os::raw::c_void;
            tcp_pattern[0].mask = &mut eth_mask as *mut _ as *mut std::os::raw::c_void;
        
            tcp_pattern[1].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_IPV4;
            tcp_pattern[1].spec = &mut ip_spec_tcp as *mut _ as *mut std::os::raw::c_void;
            tcp_pattern[1].mask = &mut ip_mask as *mut _ as *mut std::os::raw::c_void;
        
            tcp_pattern[2].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_TCP;
            let mut flow_tcp: rte_tcp_hdr = mem::zeroed();
            let mut flow_tcp_mask: rte_tcp_hdr = mem::zeroed();
            flow_tcp.dst_port = u16::to_be(port);
            flow_tcp_mask.dst_port = u16::MAX;
            tcp_pattern[2].spec = &mut flow_tcp as *mut _ as *mut std::os::raw::c_void;
            tcp_pattern[2].mask = &mut flow_tcp_mask as *mut _ as *mut std::os::raw::c_void;
        
            tcp_pattern[3].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_END;    


            let mut tcp_action: Vec<rte_flow_action> = vec![mem::zeroed(); 2];
            tcp_action[0].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_QUEUE;
            let mut queue_action: rte_flow_action_queue = mem::zeroed();
            queue_action.index = 2*i;
            tcp_action[0].conf = &mut queue_action as *mut _ as *mut std::os::raw::c_void;
            tcp_action[1].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_END;
            
            let tcp_error = rte_flow_validate(0, &attr, tcp_pattern.as_ptr(), tcp_action.as_ptr(), &mut err);
            let tcp_flow = rte_flow_create(0, &attr, tcp_pattern.as_ptr(), tcp_action.as_ptr(), &mut err);
            if tcp_error != 0 {
                warn!("Flow rule is not valid, code {:?}", tcp_error);
            }
            if tcp_flow.is_null() {
                warn!("rte_flow_create failed");
            }
            println!("TCP Flow (port: {:?} => queue: {:?}) created: {:?}", port, queue_action.index, tcp_flow);
        }
        }
    }

    fn generate_udp_flow_rules(nr_queues: u16) {
        unsafe{
        for i in 0..nr_queues {
            let port: u16 = i + 10000;
            let mut err: rte_flow_error = mem::zeroed();
            let mut attr: rte_flow_attr = mem::zeroed();
            attr.priority = 0; // FLOW_TRANSPORT_PRIORITY: 0
            attr.set_egress(0);
            attr.set_ingress(1);
        
            let mut eth_spec: rte_ether_hdr = mem::zeroed();
            let mut eth_mask: rte_ether_hdr = mem::zeroed();
            eth_spec.ether_type = u16::to_be(RTE_ETHER_TYPE_IPV4 as u16);
            eth_mask.ether_type = u16::MAX;
        
            let mut ip_spec_tcp: rte_ipv4_hdr = mem::zeroed();
            let mut ip_spec_udp: rte_ipv4_hdr = mem::zeroed();
            let mut ip_mask: rte_ipv4_hdr = mem::zeroed();
            ip_spec_tcp.next_proto_id = u8::to_be(Self::FLOW_IPV4_PROTO_TCP as u8);
            ip_spec_udp.next_proto_id = u8::to_be(Self::FLOW_IPV4_PROTO_UDP as u8);
            ip_mask.next_proto_id = u8::MAX;
        
        
            let mut udp_pattern: Vec<rte_flow_item> = vec![mem::zeroed(); 4];
            udp_pattern[0].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_ETH;
            udp_pattern[0].spec = &mut eth_spec as *mut _ as *mut std::os::raw::c_void;
            udp_pattern[0].mask = &mut eth_mask as *mut _ as *mut std::os::raw::c_void;
        
            udp_pattern[1].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_IPV4;
            udp_pattern[1].spec = &mut ip_spec_udp as *mut _ as *mut std::os::raw::c_void;
            udp_pattern[1].mask = &mut ip_mask as *mut _ as *mut std::os::raw::c_void;
        
            udp_pattern[2].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_UDP;
            let mut flow_udp: rte_udp_hdr = mem::zeroed();
            let mut flow_udp_mask: rte_udp_hdr = mem::zeroed();
            flow_udp.dst_port = u16::to_be(port);
            flow_udp_mask.dst_port = u16::MAX;
            udp_pattern[2].spec = &mut flow_udp as *mut _ as *mut std::os::raw::c_void;
            udp_pattern[2].mask = &mut flow_udp_mask as *mut _ as *mut std::os::raw::c_void;
        
            udp_pattern[3].type_ = rte_flow_item_type_RTE_FLOW_ITEM_TYPE_END;    


            let mut udp_action: Vec<rte_flow_action> = vec![mem::zeroed(); 2];
            udp_action[0].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_QUEUE;
            let mut queue_action: rte_flow_action_queue = mem::zeroed();
            queue_action.index = 2*i;
            udp_action[0].conf = &mut queue_action as *mut _ as *mut std::os::raw::c_void;
            udp_action[1].type_ = rte_flow_action_type_RTE_FLOW_ACTION_TYPE_END;
            
            let udp_error = rte_flow_validate(0, &attr, udp_pattern.as_ptr(), udp_action.as_ptr(), &mut err);
            let udp_flow = rte_flow_create(0, &attr, udp_pattern.as_ptr(), udp_action.as_ptr(), &mut err);
            if udp_error != 0 {
                warn!("Flow rule is not valid, code {:?}", udp_error);
            }
            if udp_flow.is_null() {
                warn!("rte_flow_create failed");
            }
            println!("UDP Flow (port: {:?} => queue: {:?}) created: {:?}", port, queue_action.index, udp_flow);
        }
        }
    }
    
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl Runtime for DPDKRuntime {}
