/* -*- P4_16 -*- */

#include "./capybara_header.h"



/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    ethernet_h                  ethernet;
    ipv4_h                      ipv4;
    tcp_h                       tcp;
    tcp_migration_header_h      tcp_migration_header;
}

struct my_ingress_metadata_t {
    PortId_t ingress_port;
    PortId_t egress_port;
    bit<16> l4_payload_checksum;

    // bit<48> mac;
    bit<1> is_collision;
    bit<32> ip;
    bit<16> port;

    bit<16> hash_digest1;
    bit<16> hash_digest2;
    
    bit<32> client_ip;
    bit<16> client_port;
    // pair pair_holder;
    // bit<32> holder_a;
    // bit<32> holder_b;

    bit<1> request_ip_matched;
    bit<1> request_port_matched;
    bit<1> reply_ip_matched;
    bit<1> reply_port_matched;
}

/***********************  P A R S E R  **************************/
parser IngressParser(
    packet_in pkt,
    out my_ingress_headers_t hdr,
    out my_ingress_metadata_t meta,
    out ingress_intrinsic_metadata_t ig_intr_md){

    Checksum() ipv4_checksum;
    Checksum() tcp_checksum;

    /* This is a mandatory state, required by Tofino Architecture */
    state start {
        pkt.extract(ig_intr_md);
        pkt.advance(PORT_METADATA_SIZE);
        transition meta_init;
    }

    state meta_init {
        meta.ingress_port = ig_intr_md.ingress_port;
        meta.egress_port = 0;
        meta.l4_payload_checksum  = 0;
        
        // meta.mac = 0;
        meta.is_collision = 0;
        meta.ip = 0;
        meta.port = 0;
        meta.hash_digest1 = 0;
        meta.hash_digest2 = 0;
        // meta.pair_holder = {0, 0};
        // meta.holder_a = 0;
        // meta.holder_b = 0;
        meta.request_ip_matched = 0;
        meta.request_port_matched = 0;
        meta.reply_ip_matched = 0;
        meta.reply_port_matched = 0;

        // meta.src_ip = 0;
        // meta.src_port = 0;
        // meta.dst_ip = 0;
        // meta.dst_port = 0;
        
        transition parse_ethernet;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
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
            default: accept;
        }
    }

    state parse_tcp {
        pkt.extract(hdr.tcp);

        // meta.src_port = hdr.tcp.src_port;
        // meta.dst_port = hdr.tcp.dst_port;

        /* Calculate Payload checksum */
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

        transition select(pkt.lookahead<bit<32>>()) {
            MIGRATION_SIGNATURE: parse_tcp_migration_header;
            default: accept;
        }
    }

    state parse_tcp_migration_header {
        pkt.extract(hdr.tcp_migration_header);
        transition accept;
    }

}

/***************** M A T C H - A C T I O N  *********************/
#include "./capybara_hash.p4"
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


    /* REGISTERS FOR REQUEST MIGRATION */
    #include "reg0.p4"
    // #include "reg1.p4"

    Register<bit<32>, _> (32w1) counter;
    RegisterAction<bit<32>, _, bit<32>>(counter) counter_update = {
        void apply(inout bit<32> val, out bit<32> rv) {
            rv = val;
            val = val + 1;
        }
    };

    Register< bit<16>, bit<8> >(1, 0) reg_check;  // value, key
    RegisterAction< bit<16>, bit<8>, bit<16> >(reg_check)
    check_val = {
        void apply(inout bit<16> register_data, out bit<16> return_data) {
            register_data = meta.hash_digest1;
            return_data = register_data;
        }
    };
    action exec_check_val(){
        check_val.execute(0);
    }

    #include "forwarding.p4"


    calc_hash(CRCPolynomial<bit<32>>(
            coeff=32w0x04C11DB7, reversed=true, msb=false, extended=false,
            init=32w0xFFFFFFFF, xor=32w0xFFFFFFFF)) hash;
    apply {
        bit<16> hash1;
        bit<16> hash2;
        bit<16> hash3;
        ig_tm_md.bypass_egress = 1w1;
        
        if(hdr.tcp_migration_header.isValid()){
            if(hdr.tcp_migration_header.flag[0:0] == 0b1){ // LOAD flag is on
                // ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;
                hash.apply(hdr.tcp_migration_header.client_ip, hdr.tcp_migration_header.client_port, meta.hash_digest1);        
        
                exec_write_reg0_request_client_ip(meta.hash_digest1);
                tb_write_reply_client_ip.apply();
                tb_write_request_client_port.apply();
                tb_write_reply_client_port.apply();
                tb_write_target_mac_hi32.apply();
                tb_write_origin_mac_hi32.apply();
                tb_write_target_mac_lo16.apply();
                tb_write_origin_mac_lo16.apply();
                tb_write_target_ip.apply();
                tb_write_origin_ip.apply();
                tb_write_target_port.apply();
                // else if(write_reg1_request_client_ip_port.execute(hash1) == 0){
                // }

                // exec_check_val();
            }
        }                   
        else if(hdr.tcp.isValid()){
            hash.apply(hdr.ipv4.dst_ip, hdr.tcp.dst_port, meta.hash_digest1);
            hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, meta.hash_digest2);
            // hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, meta)
            // hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, hash3);
            
            exec_check_reg0_request_client_ip(meta.hash_digest1);
            exec_check_reg0_request_client_port(meta.hash_digest1);
            exec_check_reg0_reply_client_ip(meta.hash_digest2);
            exec_check_reg0_reply_client_port(meta.hash_digest2);
            tb_request_mac_hi32.apply();
            tb_request_mac_lo16.apply();
            tb_request_ip.apply();
            tb_request_port.apply();
            // if(meta.request_ip_matched == 1){
                
            //     if(meta.request_port_matched == 1){
            //         exec_read_reg0_target_mac_hi32(hash2);
            //         exec_read_reg0_target_mac_lo16(hash2);
            //         exec_read_reg0_target_ip(hash2);
            //         exec_read_reg0_target_port(hash2);
            //     }
            // }
            // else{
            //     // exec_check_reg0_reply_client_ip(hash3);
            // }
        }

        l2_forwarding.apply();
        // else if(check_reg0_reply_client_ip.execute(hash2) == 1 && check_reg0_reply_client_port.execute(hash2) == 1){
            //     exec_read_reg0_target_mac_hi32(hash2);
            // }
    }
    

}  // End of SwitchIngressControl

/*********************  D E P A R S E R  ************************/

/* This struct is needed for proper digest receive API generation */
struct migration_digest_t {
    bit<32>  src_ip;
    bit<16>  src_port;
    bit<32>  dst_ip;
    bit<16>  dst_port;
    bit<32>  meta_ip;
    bit<16>  meta_port;

    bit<16>  hash_digest1;
    bit<16>  hash_digest2;
}

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    Digest <migration_digest_t>() migration_digest;

    Checksum()  ipv4_checksum;
    Checksum()  tcp_checksum;
    apply {
        // if (ig_dprsr_md.digest_type == TCP_MIGRATION_DIGEST) {
        //     migration_digest.pack({
        //             meta.src_ip,
        //             meta.src_port,
        //             meta.dst_ip,
        //             meta.dst_port,
        //             meta.ip,
        //             meta.port,
        //             meta.hash_digest1,
        //             meta.hash_digest2 });
        // }


        if (hdr.ipv4.isValid() && !hdr.tcp_migration_header.isValid()) {
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
