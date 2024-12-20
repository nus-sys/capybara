/* -*- P4_16 -*- */

#include "../capybara_header.h"



/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    bridged_meta_h              bridged_meta; // <== To Add
    
    pktgen_timer_header_t       pktgen_timer_header;
    remaining_ethernet_h        remaining_ethernet;

    ethernet_h                  ethernet;
    ipv4_h                      ipv4;
    tcp_h                       tcp;
    udp_h                       udp;
    tcpmig_h                    tcpmig;
    heartbeat_h                 heartbeat;
}

struct my_ingress_metadata_t { // client ip and port in meta
    // port_mirror_meta_t port_mirror_meta; // <== To Add


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
        
        
        transition select(pkt.lookahead<bit<112>>()[15:0]) {
            ETHERTYPE_PKTGEN: parse_pktgen;
            default: parse_ethernet;
        }
    }

    state parse_pktgen {
        pkt.extract(hdr.pktgen_timer_header);
        transition parse_remaining_ethernet;
    }

    state parse_remaining_ethernet {
        pkt.extract(hdr.remaining_ethernet);
        transition select(hdr.remaining_ethernet.ether_type){
            ETHERTYPE_PKTGEN: parse_ipv4;
            default: accept;
        }
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
#include "../capybara_hash.p4"
#include "migration_helper.p4"
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

    #include "../forwarding.p4"

    // PortMirroringIngress() port_mirroring_ingress; // <== To Add


    calc_hash(CRCPolynomial<bit<32>>(
            coeff=32w0x04C11DB7, reversed=true, msb=false, extended=false,
            init=32w0xFFFFFFFF, xor=32w0xFFFFFFFF)) hash;
    

    Register< index_t, index_t> (TWO_POWER_SIXTEEN) reg_be_idx; // value, key
    RegisterAction<index_t, index_t, index_t>(reg_be_idx) get_be_idx = { // value, key, out
        void apply(inout index_t val, out index_t rv) {
            rv = val;
            // if(val == NUM_BACKENDS-1){
            //     val = 0;
            // }else{
            //     val = val + 1;    
            // }
        }
    };

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) backend_mac_hi32;
    RegisterAction< value32b_t, index_t, value32b_t >(backend_mac_hi32) read_backend_mac_hi32 = {
        void apply(inout value32b_t register_value, out value32b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_backend_mac_hi32() {
        meta.owner_mac[47:16] = read_backend_mac_hi32.execute(meta.backend_idx);
    }

    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) backend_mac_lo16;
    RegisterAction< value16b_t, index_t, value16b_t >(backend_mac_lo16) read_backend_mac_lo16 = {
        void apply(inout value16b_t register_value, out value16b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_backend_mac_lo16() {
        meta.owner_mac[15:0] = read_backend_mac_lo16.execute(meta.backend_idx);
    }

    Register< value32b_t, index_t >(TWO_POWER_SIXTEEN) backend_ip;
    RegisterAction< value32b_t, index_t, value32b_t >(backend_ip) read_backend_ip = {
        void apply(inout value32b_t register_value, out value32b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_backend_ip() {
        meta.owner_ip = read_backend_ip.execute(meta.backend_idx);
    }


    Register< value16b_t, index_t >(TWO_POWER_SIXTEEN) backend_port;
    RegisterAction< value16b_t, index_t, value16b_t >(backend_port) read_backend_port = {
        void apply(inout value16b_t register_value, out value16b_t rv) {
            rv = register_value;
        }
    };
    action exec_read_backend_port() {
        meta.owner_port = read_backend_port.execute(meta.backend_idx);
    }


    Register< bit<32>, bit<8> >(256) reg_check;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_check)
    check_val = {
        void apply(inout bit<32> register_data, out bit<32> return_data) {
            register_data = meta.time;
        }
    };
    action exec_check_val(){
        check_val.execute(hdr.tcpmig.flag);
    }

    MigrationRequestIdentifier32b() request_client_ip_0;
    MigrationRequestIdentifier16b() request_client_port_0;
    MigrationReplyIdentifier32b() reply_client_ip_0;
    MigrationReplyIdentifier16b() reply_client_port_0;

    // MigrationRequest32b0() owner_mac_hi32_0;
    // MigrationRequest16b0() owner_mac_lo16_0;
    MigrationRequest32b0() owner_ip_0;
    MigrationRequest16b0() owner_port_0;


    Blocker0() blocker0;

    action exec_reply_rewrite() {
        hdr.ipv4.src_ip = FE_IP;
        hdr.tcp.src_port = FE_PORT;
    }
    table tbl_reply_rewrite {
        key = {
            meta.initial_distribution    : ternary;
            meta.flag               : ternary;
            meta.result02           : ternary;
            meta.result03           : ternary;
        }
        actions = {
            exec_reply_rewrite;
            NoAction;
        }
        const entries = {
            (0, 0, 1, 1) : exec_reply_rewrite();
        }
        const default_action = NoAction();
        size = 1;
    }
    
    action exec_mac_rewrite(bit<48> mac_addr) {
        hdr.ethernet.dst_mac = mac_addr;
    }
    table tbl_mac_lookup {
        key = {
            hdr.ipv4.dst_ip : exact;
        }
        actions = {
            exec_mac_rewrite;
            NoAction;
        }
        const entries = {
            0x0a000107 : exec_mac_rewrite(0x08c0ebb6cd5d);
            0x0a000108 : exec_mac_rewrite(0x08c0ebb6e805);
            0x0a000109 : exec_mac_rewrite(0x08c0ebb6c5ad);
        }
        const default_action = NoAction();
        size = 4;
    }

    Register<index_t, _> (1) counter;
    RegisterAction<index_t, _, index_t>(counter) counter_update = {
        void apply(inout index_t val, out index_t rv) {
            rv = val;
            val = val + 1;
        }
    };

    apply {
         
        if(hdr.pktgen_timer_header.isValid()){
            remove_pktgen_hdr();
            hdr.udp.checksum = 0;
            send(PIPE_0_RECIRC);
        }else if(ig_intr_md.ingress_port == PIPE_0_RECIRC){
            multicast(2);
        }
        else if(hdr.ipv4.isValid()){
            bit<16> hash1;
            bit<16> hash2;
            bit<1> holder_1b_00;
            bit<1> holder_1b_01;
            bit<1> holder_1b_02;
            bit<1> holder_1b_03;

            if(hdr.tcp.isValid()){
                hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, hash1);
                hash.apply(hdr.ipv4.dst_ip, hdr.tcp.dst_port, hash2);
                if(hdr.tcp.flags == 0b00000010 && hdr.ipv4.dst_ip == FE_IP){ // SYN to FE
                    // ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;

                    meta.client_ip = hdr.ipv4.src_ip;
                    meta.client_port = hdr.tcp.src_port;

                    // meta.backend_idx = get_be_idx.execute(meta.client_port); // counter => address
                    meta.backend_idx = counter_update.execute(0);
                    
                    exec_read_backend_mac_hi32();
                    exec_read_backend_mac_lo16();
                    exec_read_backend_ip();
                    exec_read_backend_port();
                    
                    hash2 = hash1;
                    meta.initial_distribution = 1; // initial migration from FE (switch) to a BE

                    hdr.ethernet.dst_mac = meta.owner_mac;
                    hdr.ipv4.dst_ip = meta.owner_ip;
                    hdr.tcp.dst_port = meta.owner_port;
                }
            }
            else if(hdr.tcpmig.isValid()){
                hash.apply(meta.client_ip, meta.client_port, hash1);
                hash2 = hash1;

                // ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;
                hdr.udp.checksum = 0;
                // hdr.ethernet.dst_mac = BE_MAC;
                // hdr.ipv4.dst_ip = BE_IP;
            }

            if(meta.flag[0:0] == 1){ // chown
                // meta.owner_mac = hdr.ethernet.src_mac;
                meta.owner_ip = hdr.ipv4.src_ip;
                meta.owner_port = hdr.udp.src_port;
            }

            // When the owner is changed? 1) SYN; 2) migration;
            request_client_ip_0.apply(hash1, hdr, meta, holder_1b_00);
            request_client_port_0.apply(hash1, hdr, meta, holder_1b_01);
            reply_client_ip_0.apply(hash2, hdr, meta, holder_1b_02);
            reply_client_port_0.apply(hash2, hdr, meta, holder_1b_03);
            
            meta.result00 = holder_1b_00;
            meta.result01 = holder_1b_01;
            meta.result02 = holder_1b_02;
            meta.result03 = holder_1b_03;

            owner_ip_0.apply(hash1, meta.owner_ip, meta, hdr.ipv4.dst_ip);
            owner_port_0.apply(hash1, meta.owner_port, meta, hdr.tcp.dst_port);
            
            tbl_reply_rewrite.apply();
            tbl_mac_lookup.apply();

            // blocker0.apply(hash1, meta, ig_dprsr_md.drop_ctl[0:0]);
            l2_forwarding.apply();
        }else{
            l2_forwarding.apply();
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
    // PortMirroringCommonDeparser() port_mirroring_deparser; // <== To Add
    Digest <migration_digest_t>() migration_digest;

    Checksum()  ipv4_checksum;
    Checksum()  tcp_checksum;
    // Checksum()  udp_checksum;

    apply {
        // port_mirroring_deparser.apply(meta.port_mirror_meta, ig_dprsr_md.mirror_type); // <== To Add

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
    ethernet_h   ethernet;
}

/********  G L O B A L   E G R E S S   M E T A D A T A  *********/

struct my_egress_metadata_t {
}

/***********************  P A R S E R  **************************/

parser EgressParser(packet_in        pkt,
    /* User */
    out my_egress_headers_t          hdr,
    out my_egress_metadata_t         meta,
    /* Intrinsic */
    out egress_intrinsic_metadata_t  eg_intr_md)
{
    /* This is a mandatory state, required by Tofino Architecture */
    state start {
        pkt.extract(eg_intr_md);
        transition parse_ethernet;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
        transition select(hdr.ethernet.ether_type) {
            default: accept;
        }
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
    apply {
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
    apply {
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
