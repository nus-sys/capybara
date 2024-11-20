/* -*- P4_16 -*- */

#include "../capybara_header.h"



/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    ethernet_h                  ethernet;
    ipv4_h                      ipv4;
    tcp_h                       tcp;
    udp_h                       udp;
    tcpmig_h                    tcpmig;
    heartbeat_h                 heartbeat;
}

struct my_ingress_metadata_t { // client ip and port in meta
    port_mirror_meta_t port_mirror_meta; // <== To Add


    index_t backend_idx;
    bit<48> owner_mac;
    value32b_t owner_ip;
    value16b_t owner_port;

    value32b_t client_ip;
    value16b_t client_port;

    PortId_t ingress_port;
    PortId_t egress_port;
    bit<16> l4_payload_checksum;
    
    bit<16> hash1;
    bit<16> hash2;
    // bit<16> hash3;
    
    bit<8> flag;
    bit<1> initial_distribution;
    bit<1> result00;
    bit<1> result01;
    bit<1> result02;
    bit<1> result03;
    bit<1> result10;
    bit<1> result11;
    bit<1> result12;
    bit<1> result13;

    // bit<48> dst_mac;

    bit<32> time;
}

/***********************  P A R S E R  **************************/
parser IngressParser(
    packet_in pkt,
    out my_ingress_headers_t hdr,
    out my_ingress_metadata_t meta,
    out ingress_intrinsic_metadata_t ig_intr_md){

    Checksum() ipv4_checksum;
    Checksum() tcp_checksum;
    // Checksum() udp_checksum;

    /* This is a mandatory state, required by Tofino Architecture */
    state start {
        pkt.extract(ig_intr_md);
        pkt.advance(PORT_METADATA_SIZE);
        transition meta_init;
    }

    state meta_init {
        meta.backend_idx = 0;
        meta.owner_mac = 0;
        meta.owner_ip = 0;
        meta.owner_port = 0;

        meta.client_ip = 0;
        meta.client_port = 0;

        meta.ingress_port = ig_intr_md.ingress_port;
        meta.egress_port = 0;
        meta.l4_payload_checksum  = 0;
        meta.flag = 0;
        meta.initial_distribution = 0;
        
        meta.result00 = 0;
        meta.result01 = 0;
        meta.result02 = 0;
        meta.result03 = 0;
        meta.result10 = 0;
        meta.result11 = 0;
        meta.result12 = 0;
        meta.result13 = 0;
        
        transition parse_ethernet;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
        // meta.dst_mac = hdr.ethernet.dst_mac;
        transition select(hdr.ethernet.ether_type){
            ETHERTYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);
        
        // meta.src_ip = hdr.ipv4.src_ip;
        // meta.dst_ip = hdr.ipv4.dst_ip;


        tcp_checksum.subtract({
            hdr.ipv4.src_ip,
            hdr.ipv4.dst_ip,
            8w0, hdr.ipv4.protocol
        });
        transition select(hdr.ipv4.protocol){
            IP_PROTOCOL_TCP: parse_tcp;
            IP_PROTOCOL_UDP: parse_udp;
            default: accept;
        }
    }

    state parse_tcp {
        pkt.extract(hdr.tcp);

        /* Calculate Payload _m */
        tcp_checksum.subtract({
            hdr.tcp.src_port,
            hdr.tcp.dst_port,
            hdr.tcp.seq_no,
            hdr.tcp.ack_no,
            hdr.tcp.data_offset, hdr.tcp.res, hdr.tcp.flags,
            hdr.tcp.window,
            hdr.tcp.checksum,
            hdr.tcp.urgent_ptr
        });

        meta.l4_payload_checksum = tcp_checksum.get();

        transition accept;
    }

    state parse_udp {
        pkt.extract(hdr.udp);

        /* Calculate Payload checksum */
        // udp_checksum.subtract({
        //     hdr.udp.src_port,
        //     hdr.udp.dst_port,
        //     // hdr.udp.len, Do not subtract the length!!!!
        //     hdr.udp.checksum
        // });

        // meta.l4_payload_checksum = udp_checksum.get();

        transition select(pkt.lookahead<bit<32>>()) {
            MIGRATION_SIGNATURE: parse_tcpmig;
            HEARTBEAT_SIGNATURE: parse_heartbeat;
            default: accept;
        }
    }

    state parse_tcpmig {
        pkt.extract(hdr.tcpmig);
        meta.client_ip = hdr.tcpmig.client_ip;
        meta.client_port = hdr.tcpmig.client_port;
        meta.flag = hdr.tcpmig.flag;
        transition accept;
    }

    state parse_heartbeat {
        pkt.extract(hdr.heartbeat);
        transition accept;
    }
}

/***************** M A T C H - A C T I O N  *********************/
control Ingress(
    /* User */
    inout my_ingress_headers_t                       hdr,
    inout my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_t               ig_intr_md,
    in    ingress_intrinsic_metadata_from_parser_t   ig_prsr_md,
    inout ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md,
    inout ingress_intrinsic_metadata_for_tm_t        ig_tm_md)
{

    action l2_forward(PortId_t port) {
        ig_tm_md.ucast_egress_port=port;
    }

    action broadcast() {
        ig_tm_md.mcast_grp_a = 1;
        ig_tm_md.level2_exclusion_id = ig_intr_md.ingress_port;
    }

    table l2_forwarding_decision {
        key = {
            hdr.ethernet.dst_mac : exact;
        }
        actions = {
            l2_forward;
            broadcast;
        }
    }

    action exec_forwarding_to_endhost_switch() {
        ig_tm_md.ucast_egress_port=36;
        // hdr.ethernet.dst_mac = FE_MAC;
        // hdr.ipv4.dst_ip = FE_IP;
        // hdr.tcp.dst_port = FE_PORT;
    }

    apply {
        ig_tm_md.bypass_egress = 1;
        if(ig_intr_md.ingress_port != 36){ // not from endhost switch
            exec_forwarding_to_endhost_switch(); // send to endhost switch
        }else{
            l2_forwarding_decision.apply();
        }
    }
}  // End of SwitchIngressControl

/*********************  D E P A R S E R  ************************/

/* This struct is needed for proper digest receive API generation */
struct migration_digest_t {
    bit<48>  src_mac;
    bit<32>  src_ip;
    bit<16>  src_port;

    bit<48>  dst_mac;
    bit<32>  dst_ip;
    bit<16>  dst_port;
    
    // bit<32>  dst_ip;
    // bit<16>  dst_port;
    // bit<32>  meta_ip;
    // bit<16>  meta_port;

    bit<16>  hash_digest1;
    // bit<16>  hash_digest2;
    // bit<16>  hash_digest;
}

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    PortMirroringCommonDeparser() port_mirroring_deparser; // <== To Add
    Digest <migration_digest_t>() migration_digest;

    Checksum()  ipv4_checksum;
    Checksum()  tcp_checksum;
    // Checksum()  udp_checksum;

    apply {
        port_mirroring_deparser.apply(meta.port_mirror_meta, ig_dprsr_md.mirror_type); // <== To Add

        if (ig_dprsr_md.digest_type == TCP_MIGRATION_DIGEST) {
            migration_digest.pack({
                    hdr.ethernet.src_mac,
                    hdr.ipv4.src_ip,
                    hdr.udp.src_port,
                    hdr.ethernet.dst_mac,
                    hdr.ipv4.dst_ip,
                    hdr.udp.dst_port,
                    meta.hash1
                });
        }


        if (hdr.tcp.isValid()) {
            hdr.ipv4.hdr_checksum = ipv4_checksum.update({
                hdr.ipv4.version,
                hdr.ipv4.ihl,
                hdr.ipv4.diffserv,
                hdr.ipv4.total_len,
                hdr.ipv4.identification,
                hdr.ipv4.flags,
                hdr.ipv4.frag_offset,
                hdr.ipv4.ttl,
                hdr.ipv4.protocol,
                hdr.ipv4.src_ip,
                hdr.ipv4.dst_ip
            });
            hdr.tcp.checksum = tcp_checksum.update({
                hdr.ipv4.src_ip,
                hdr.ipv4.dst_ip,
                8w0, hdr.ipv4.protocol,
                hdr.tcp.src_port,
                hdr.tcp.dst_port,
                hdr.tcp.seq_no,
                hdr.tcp.ack_no,
                hdr.tcp.data_offset, hdr.tcp.res, hdr.tcp.flags,
                hdr.tcp.window,
                hdr.tcp.urgent_ptr,
                /* Any headers past TCP */
                meta.l4_payload_checksum
            });
        }

        if (hdr.udp.isValid()) {
            hdr.ipv4.hdr_checksum = ipv4_checksum.update({
                hdr.ipv4.version,
                hdr.ipv4.ihl,
                hdr.ipv4.diffserv,
                hdr.ipv4.total_len,
                hdr.ipv4.identification,
                hdr.ipv4.flags,
                hdr.ipv4.frag_offset,
                hdr.ipv4.ttl,
                hdr.ipv4.protocol,
                hdr.ipv4.src_ip,
                hdr.ipv4.dst_ip
            });
            // hdr.udp.checksum = udp_checksum.update({
            //     hdr.ipv4.src_ip,
            //     hdr.ipv4.dst_ip,
            //     8w0, hdr.ipv4.protocol,
            //     hdr.udp.src_port,
            //     hdr.udp.dst_port,
            //     /* Any headers past TCP */
            //     meta.l4_payload_checksum
            // });
        }

        pkt.emit(hdr);
    }
}


/*************************************************************************
 ****************  E G R E S S   P R O C E S S I N G   *******************
 *************************************************************************/

/***********************  H E A D E R S  ************************/

struct my_egress_headers_t {
    ethernet_h          ethernet;
    ipv4_h              ipv4;
    tcp_h               tcp;
    udp_h               udp;
    tcpmig_h            tcpmig;
    rps_signal_h        rps_signal;
}

/********  G L O B A L   E G R E S S   M E T A D A T A  *********/

struct my_egress_metadata_t {
    port_mirror_meta_t  port_mirror_meta; // <== To Add
    bit<16>             udp_dst_port;
    bit<8>              active_reg_idx;
    bit<8>              snapshot_reg_idx;
    bit<16>             tcp_hdr_len;

    bit<1>              is_min_rps_updated;
    bit<16>             min_rps_server_port;
    bit<8>              flag;
}

/***********************  P A R S E R  **************************/

parser EgressParser(packet_in        pkt,
    /* User */
    out my_egress_headers_t          hdr,
    out my_egress_metadata_t         meta,
    /* Intrinsic */
    out egress_intrinsic_metadata_t  eg_intr_md)
{
    PortMirroringEgressParser() port_mirror_parser; // <== To Add
    /* This is a mandatory state, required by Tofino Architecture */
    state start {
        meta.udp_dst_port = 0;
        meta.active_reg_idx = 0;
        meta.snapshot_reg_idx = 0;
        meta.tcp_hdr_len = 0;
        meta.is_min_rps_updated = 0;
        meta.flag = 0;
        pkt.extract(eg_intr_md);
        port_mirror_parser.apply(pkt, meta.port_mirror_meta); // <== To Add 
        transition parse_ethernet;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
        transition select(hdr.ethernet.ether_type) {
            ETHERTYPE_IPV4:  parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);
        transition select(hdr.ipv4.protocol){
            IP_PROTOCOL_TCP: parse_tcp;
            IP_PROTOCOL_UDP: parse_udp;
            default: accept;
        }
    }

    state parse_tcp {
        pkt.extract(hdr.tcp);
        // meta.tcp_hdr_len[5:2] = hdr.tcp.data_offset;
        transition accept;
    }

    state parse_udp {
        pkt.extract(hdr.udp);
        meta.udp_dst_port = hdr.udp.dst_port;
        transition select(pkt.lookahead<bit<32>>()) {
            MIGRATION_SIGNATURE: parse_tcpmig;
            RPS_SIGNAL_SIGNATURE: parse_rps_signal;
            default: accept;
        }
    }

    state parse_tcpmig {
        pkt.extract(hdr.tcpmig);
        meta.flag = hdr.tcpmig.flag;
        transition accept;
    }

    state parse_rps_signal {
        pkt.extract(hdr.rps_signal);
        transition accept;
    }
}

/***************** M A T C H - A C T I O N  *********************/

control Egress(
    /* User */
    inout my_egress_headers_t                          hdr,
    inout my_egress_metadata_t                         meta,
    /* Intrinsic */
    in    egress_intrinsic_metadata_t                  eg_intr_md,
    in    egress_intrinsic_metadata_from_parser_t      eg_prsr_md,
    inout egress_intrinsic_metadata_for_deparser_t     eg_dprsr_md,
    inout egress_intrinsic_metadata_for_output_port_t  eg_oport_md)
{
    PortMirroringEgress() port_mirroring_egress; // <== To Add
    
    // bit<1> value register does not allow conditional statement because it uses a different (simpler) ALU
    // so I use bit<8> to workaround.
    Register<bit<8>, _> (32w1) reg_active; 
    RegisterAction<bit<8>, _, bit<8>>(reg_active) update_active_reg_idx = {
        void apply(inout bit<8> val, out bit<8> return_val) {
            if(val == 0){
                val = 1;
            }else {
                val = 0;
            }
            return_val = val;
        }
    };
    action exec_update_active_reg_idx(){
        meta.active_reg_idx = update_active_reg_idx.execute(0);
    }
    RegisterAction<bit<8>, _, bit<8>>(reg_active) read_active_reg_idx = {
        void apply(inout bit<8> val, out bit<8> return_val) {
            return_val = val;
        }
    };
    action exec_read_active_reg_idx(){
        meta.active_reg_idx = read_active_reg_idx.execute(0);
    }
    RegisterAction<bit<8>, _, bit<8>>(reg_active) read_snapshot_reg_idx = {
        void apply(inout bit<8> val, out bit<8> return_val) {
            if(val == 0){
                return_val = 1;
            }else {
                return_val = 0;
            }
        }
    };
    action exec_read_snapshot_reg_idx(){
        meta.snapshot_reg_idx = read_snapshot_reg_idx.execute(0);
    }

    Register<bit<16>, bit<8>> (32w256) reg_rid_to_svrport; 
    RegisterAction<bit<16>, _, bit<16>>(reg_rid_to_svrport) convert_rid_to_svrport = {
        void apply(inout bit<16> val, out bit<16> return_val) {
            return_val = val;
        }
    };
    action exec_convert_rid_to_svrport(){
        hdr.udp.dst_port = convert_rid_to_svrport.execute(eg_intr_md.egress_rid);
    }


    Register<bit<32>, _> (32w2) reg_sum_rps;
    RegisterAction<bit<32>, _, bit<32>>(reg_sum_rps) init_reg_sum_rps = {
        void apply(inout bit<32> val) {
            val = 0;
        }
    };
    action exec_init_reg_sum_rps(){
        init_reg_sum_rps.execute(meta.active_reg_idx);
    }
    RegisterAction<bit<32>, _, bit<32>>(reg_sum_rps) read_reg_sum_rps = {
        void apply(inout bit<32> val, out bit<32> return_val) {
            return_val = val;
        }
    };
    action exec_read_reg_sum_rps(){
        hdr.rps_signal.sum = read_reg_sum_rps.execute(meta.snapshot_reg_idx);
    }
    RegisterAction<bit<32>, _, bit<32>>(reg_sum_rps) add_reg_sum_rps = {
        void apply(inout bit<32> val) {
            val = val + 1;
        }
    };
    action exec_add_reg_sum_rps(){
        add_reg_sum_rps.execute(meta.active_reg_idx);
    }

    Register<bit<32>, bit<16>> (TWO_POWER_SIXTEEN) reg_individual_rps_0;
    RegisterAction<bit<32>, bit<16>, bit<32>>(reg_individual_rps_0) read_reg_individual_rps_0 = {
        void apply(inout bit<32> val, out bit<32> return_val) {
            return_val = val;
            val = 0;
        }
    };
    action exec_read_reg_individual_rps_0(){
        hdr.rps_signal.individual = read_reg_individual_rps_0.execute(hdr.udp.dst_port);
    }
    RegisterAction<bit<32>, bit<16>, bit<32>>(reg_individual_rps_0) add_reg_individual_rps_0 = {
        void apply(inout bit<32> val) {
            val = val + 1;
        }
    };
    action exec_add_reg_individual_rps_0(){
        add_reg_individual_rps_0.execute(hdr.tcp.dst_port);
    }

    Register<bit<32>, bit<16>> (TWO_POWER_SIXTEEN) reg_individual_rps_1;
    RegisterAction<bit<32>, bit<16>, bit<32>>(reg_individual_rps_1) read_reg_individual_rps_1 = {
        void apply(inout bit<32> val, out bit<32> return_val) {
            return_val = val;
            val = 0;
        }
    };
    action exec_read_reg_individual_rps_1(){
        hdr.rps_signal.individual = read_reg_individual_rps_1.execute(hdr.udp.dst_port);
    }
    RegisterAction<bit<32>, bit<16>, bit<32>>(reg_individual_rps_1) add_reg_individual_rps_1 = {
        void apply(inout bit<32> val) {
            val = val + 1;
        }
    };
    action exec_add_reg_individual_rps_1(){
        add_reg_individual_rps_1.execute(hdr.tcp.dst_port);
    }

    Register<bit<32>, _> (1) reg_min_rps;
    RegisterAction<bit<32>, _, bit<1>>(reg_min_rps) init_reg_min_rps = {
        void apply(inout bit<32> val) {
            // if(val == 0){
            val = TWO_POWER_SIXTEEN;
            // }
        }
    };
    action exec_init_reg_min_rps(){
        init_reg_min_rps.execute(0);
    }
    RegisterAction<bit<32>, _, bit<1>>(reg_min_rps) poll_reg_min_rps = {
        void apply(inout bit<32> val, out bit<1> return_val) {
            if(val > hdr.rps_signal.individual){
                val = hdr.rps_signal.individual;
                return_val = 1;
            }else{
                return_val = 0;
            }
        }
    };
    action exec_poll_reg_min_rps(){
        meta.is_min_rps_updated = poll_reg_min_rps.execute(0);
    }

    Register<bit<16>, bit<8>> (1) reg_min_rps_server_port;
    RegisterAction<bit<16>, bit<8>, bit<16>>(reg_min_rps_server_port) write_reg_min_rps_server_port = {
        void apply(inout bit<16> val) {
            if(meta.is_min_rps_updated == 1) {
                val = hdr.udp.dst_port;
            }
        }
    };
    action exec_write_reg_min_rps_server_port(){
        write_reg_min_rps_server_port.execute(0);
    }
    RegisterAction<bit<16>, bit<8>, bit<16>>(reg_min_rps_server_port) read_reg_min_rps_server_port = {
        void apply(inout bit<16> val, out bit<16> return_val) {
            return_val = val;
        }
    };
    action exec_read_reg_min_rps_server_port(){
        hdr.udp.dst_port = read_reg_min_rps_server_port.execute(0);
    }

    Register< bit<16>, _> (1) reg_round_robin_server_port; // value, key
    RegisterAction<bit<16>, _, bit<16>>(reg_round_robin_server_port) read_round_robin_server_port = {
        void apply(inout bit<16> val, out bit<16> rv) {
            rv = val+10000;
            if(val == NUM_BACKENDS-1){
                val = 0;
            }else{
                val = val + 1;    
            }
        }
    };
    action exec_read_round_robin_server_port(){
        hdr.udp.dst_port = read_round_robin_server_port.execute(0);
    }

    action drop() {
        eg_dprsr_md.drop_ctl = 1;
    }

    Register<bit<32>, _> (32w1) counter;
    RegisterAction<bit<32>, _, bit<32>>(counter) counter_update = {
        void apply(inout bit<32> val, out bit<32> rv) {
            rv = val;
            val = val + 1;
        }
    };
    // table tbl_individual_rps { 
    //     key = {
    //         meta.flag : exact;
    //     }
    //     actions = {
    //         exec_read_reg_min_rps_server_port;
    //         NoAction;
    //     }
    //     const entries = {
    //         0b00100000 : exec_read_reg_min_rps_server_port();
    //     }
    //     size = 16;
    // }

    apply {
        port_mirroring_egress.apply(hdr.ethernet.src_mac, meta.port_mirror_meta, eg_intr_md.egress_port,
                        eg_prsr_md.global_tstamp, eg_dprsr_md.mirror_type); // <== To Add
        
        if(eg_intr_md.egress_port == PIPE_0_RECIRC){
            exec_update_active_reg_idx();
            exec_init_reg_sum_rps();
            exec_init_reg_min_rps();
        }else if(hdr.rps_signal.isValid()){
            hdr.udp.dst_port = eg_intr_md.egress_rid;
            exec_read_snapshot_reg_idx();
            exec_read_reg_sum_rps();
            if(meta.snapshot_reg_idx == 0){
                exec_read_reg_individual_rps_0();
            }else{
                exec_read_reg_individual_rps_1();
            }

            exec_poll_reg_min_rps();
            // if(meta.is_min_rps_updated == 1){
            exec_write_reg_min_rps_server_port();
            // }
            // if(meta.flag == 0b00100000){
            //     exec_read_reg_min_rps_server_port();
            // }
            if(hdr.rps_signal.sum == 0){
                drop();
            }
            // tbl_individual_rps.apply();
        }else if(hdr.tcp.isValid() && hdr.tcp.flags[1:1] != 1 && hdr.ipv4.total_len != 40 && eg_intr_md.egress_port == 24){
            
            exec_read_active_reg_idx();
            exec_add_reg_sum_rps();
            if(meta.active_reg_idx == 0){
                exec_add_reg_individual_rps_0();
            }else{
                exec_add_reg_individual_rps_1();
            }
        }else if(hdr.tcpmig.isValid() && meta.flag == 0b00100000) { // PREPARE_MIG
            exec_read_reg_min_rps_server_port();
            // exec_read_round_robin_server_port();
            // counter_update.execute(0);
        }
        // tbl_individual_rps.apply();
    }
}

    /*********************  D E P A R S E R  ************************/

control EgressDeparser(packet_out pkt,
    /* User */
    inout my_egress_headers_t                       hdr,
    in    my_egress_metadata_t                      meta,
    /* Intrinsic */
    in    egress_intrinsic_metadata_for_deparser_t  eg_dprsr_md)
{
    PortMirroringCommonDeparser() port_mirroring_deparser; // <== To Add

    apply {
        port_mirroring_deparser.apply(meta.port_mirror_meta, eg_dprsr_md.mirror_type); // <== To Add

        pkt.emit(hdr);
    }
}

/************ F I N A L   P A C K A G E ******************************/
Pipeline(IngressParser(),
         Ingress(),
         IngressDeparser(),
         EgressParser(),
         Egress(),
         EgressDeparser()
         ) pipe;

Switch(pipe) main;
