/* -*- P4_16 -*- */

#include "../capybara_header.h"



/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    ethernet_h                  ethernet;
    ipv4_h                      ipv4;
    tcp_h                       tcp;
    udp_h                       udp;
    prism_add_req_h             prism_add_req;
}

struct my_ingress_metadata_t {
    PortId_t ingress_port;
    PortId_t egress_port;
    bit<16> l4_payload_checksum;
    
    bit<16> hash1;
    bit<16> hash2;
    bit<16> hash3;
    
    bit<1> load;
    bit<1> result00;
    bit<1> result01;
    bit<1> result02;
    bit<1> result03;
    bit<1> result10;
    bit<1> result11;
    bit<1> result12;
    bit<1> result13;

    bit<48> dst_mac;
    bit<32> dst_ip;
    bit<16> dst_port;
          
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
        meta.ingress_port = ig_intr_md.ingress_port;
        meta.egress_port = 0;
        meta.l4_payload_checksum  = 0;
        meta.load = 0;
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
        meta.dst_mac = hdr.ethernet.dst_mac;
        transition select(hdr.ethernet.ether_type){
            ETHERTYPE_IPV4: parse_ipv4;
            default: accept;
        }
    }

    state parse_ipv4 {
        pkt.extract(hdr.ipv4);
        
        // meta.src_ip = hdr.ipv4.src_ip;
        meta.dst_ip = hdr.ipv4.dst_ip;


        tcp_checksum.subtract({
            hdr.ipv4.src_ip,
            hdr.ipv4.dst_ip,
            8w0, hdr.ipv4.protocol
        });
        // udp_checksum.subtract({
        //     hdr.ipv4.src_ip,
        //     hdr.ipv4.dst_ip,
        //     8w0, hdr.ipv4.protocol
        // });
        
        transition select(hdr.ipv4.protocol){
            IP_PROTOCOL_TCP: parse_tcp;
            IP_PROTOCOL_UDP: parse_udp;
            default: accept;
        }
    }

    state parse_tcp {
        pkt.extract(hdr.tcp);

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

        transition accept;
        // transition select(pkt.lookahead<bit<32>>()) {
        //     MIGRATION_SIGNATURE: parse_tcp_migration_header;
        //     default: accept;
        // }
    }
    state parse_udp {
        pkt.extract(hdr.udp);
        meta.dst_port = hdr.udp.dst_port;
        /* Calculate Payload checksum */
        // udp_checksum.subtract({
        //     hdr.udp.src_port,
        //     hdr.udp.dst_port,
        //     // hdr.udp.len, Do not subtract the length!!!!
        //     hdr.udp.checksum
        // });
        // meta.l4_payload_checksum = udp_checksum.get();

        transition select(pkt.lookahead<bit<8>>()) {
            0: parse_prism_add_req;
            default: accept;
        }
    }
    state parse_prism_add_req {
        pkt.extract(hdr.prism_add_req);
        transition accept;
    }
}

/***************** M A T C H - A C T I O N  *********************/
// #include "../capybara_hash.p4"
// #include "migration_helper.p4"
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
    // calc_hash(CRCPolynomial<bit<32>>(
    //         coeff=32w0x04C11DB7, reversed=true, msb=false, extended=false,
    //         init=32w0xFFFFFFFF, xor=32w0xFFFFFFFF)) hash;
    

    // MigrationRequestIdentifier32b() request_client_ip_0;
    // MigrationRequestIdentifier16b() request_client_port_0;
    // MigrationReplyIdentifier32b() reply_client_ip_0;
    // MigrationReplyIdentifier16b() reply_client_port_0;

    // MigrationRequestIdentifier32b() request_client_ip_1;
    // MigrationRequestIdentifier16b() request_client_port_1;
    // MigrationReplyIdentifier32b() reply_client_ip_1;
    // MigrationReplyIdentifier16b() reply_client_port_1;


    // MigrationRequest32b0() target_mac_hi32_0;
    // MigrationRequest16b0() target_mac_lo16_0;
    // MigrationRequest32b0() target_ip_0;
    // MigrationRequest16b0() target_port_0;

    // MigrationReply32b0() origin_mac_hi32_0;
    // MigrationReply16b0() origin_mac_lo16_0;
    // MigrationReply32b0() origin_ip_0;
    // MigrationReply16b0() origin_port_0;

    // MigrationRequest32b1() target_mac_hi32_1;
    // MigrationRequest16b1() target_mac_lo16_1;
    // MigrationRequest32b1() target_ip_1;
    // MigrationRequest16b1() target_port_1;

    // MigrationReply32b1() origin_mac_hi32_1;
    // MigrationReply16b1() origin_mac_lo16_1;
    // MigrationReply32b1() origin_ip_1;
    // MigrationReply16b1() origin_port_1;




    
    Register<bit<32>, _> (32w1) counter;
    RegisterAction<bit<32>, _, bit<32>>(counter) counter_update = {
        void apply(inout bit<32> val, out bit<32> rv) {
            rv = val;
            val = val + 1;
        }
    };

    Register< bit<32>, _ >(1) reg_check_1;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_check_1)
    check_val_1 = {
        void apply(inout bit<32> register_data, out bit<32> return_data) {
            register_data = hdr.prism_add_req.peer_addr;
            return_data = register_data;
        }
    };
    action exec_check_val_1(){
        check_val_1.execute(0);
    }

    Register< bit<16>, _ >(1) reg_check_2;  // value, key
    RegisterAction< bit<16>, bit<8>, bit<16> >(reg_check_2)
    check_val_2 = {
        void apply(inout bit<16> register_data, out bit<16> return_data) {
            register_data = hdr.prism_add_req.peer_port;
            return_data = register_data;
        }
    };
    action exec_check_val_2(){
        check_val_2.execute(0);
    }

    Register< bit<32>, _ >(1) reg_check_3;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_check_3)
    check_val_3 = {
        void apply(inout bit<32> register_data, out bit<32> return_data) {
            register_data = hdr.prism_add_req.virtual_addr;
            return_data = register_data;
        }
    };
    action exec_check_val_3(){
        check_val_3.execute(0);
    }

    Register< bit<16>, _ >(1) reg_check_4;  // value, key
    RegisterAction< bit<16>, bit<8>, bit<16> >(reg_check_4)
    check_val_4 = {
        void apply(inout bit<16> register_data, out bit<16> return_data) {
            register_data = hdr.prism_add_req.virtual_port;
            return_data = register_data;
        }
    };
    action exec_check_val_4(){
        check_val_4.execute(0);
    }

    Register< bit<32>, _ >(1) reg_check_5;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_check_5)
    check_val_5 = {
        void apply(inout bit<32> register_data, out bit<32> return_data) {
            register_data = hdr.prism_add_req.owner_addr;
            return_data = register_data;
        }
    };
    action exec_check_val_5(){
        check_val_5.execute(0);
    }

    Register< bit<16>, _ >(1) reg_check_6;  // value, key
    RegisterAction< bit<16>, bit<8>, bit<16> >(reg_check_6)
    check_val_6 = {
        void apply(inout bit<16> register_data, out bit<16> return_data) {
            register_data = hdr.prism_add_req.owner_port;
            return_data = register_data;
        }
    };
    action exec_check_val_6(){
        check_val_6.execute(0);
    }

    Register< bit<32>, _ >(1) reg_check_7;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_check_7)
    check_val_7 = {
        void apply(inout bit<32> register_data, out bit<32> return_data) {
            register_data = hdr.prism_add_req.owner_mac[31:0];
            return_data = register_data;
        }
    };
    action exec_check_val_7(){
        check_val_7.execute(0);
    }

    action prism_reply() {
        // ig_tm_md.ucast_egress_port = ig_intr_md.ingress_port;
        // hdr.udp.checksum = 0;
        hdr.ethernet.dst_mac = hdr.ethernet.src_mac;
        hdr.ethernet.src_mac = meta.dst_mac;
        hdr.ipv4.dst_ip = hdr.ipv4.src_ip;
        hdr.ipv4.src_ip = meta.dst_ip;
        hdr.udp.dst_port = hdr.udp.src_port;
        hdr.udp.src_port = meta.dst_port;
    }


    apply {
        
        ig_tm_md.bypass_egress = 1w1;
        if(hdr.prism_add_req.isValid()){
            counter_update.execute(0);
            exec_check_val_1();
            exec_check_val_2();
            exec_check_val_3();
            exec_check_val_4();
            exec_check_val_5();
            exec_check_val_6();
            exec_check_val_7();

            prism_reply();
        }
        // if(hdr.tcp.isValid() || hdr.tcpmig.isValid()){
        //     ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;
            
        //     bit<48> target_mac;
        //     bit<48> origin_mac;
        //     bit<16> hash1;
        //     bit<16> hash2;
        //     // bit<16> hash3;

        //     bit<1> holder_1b_00;
        //     bit<1> holder_1b_01;
        //     bit<1> holder_1b_02;
        //     bit<1> holder_1b_03;

        //     bit<1> holder_1b_10;
        //     bit<1> holder_1b_11;
        //     bit<1> holder_1b_12;
        //     bit<1> holder_1b_13;

            
        //     if(meta.load == 1){
        //         counter_update.execute(0);
        //         // ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;
        //         // meta.hash1 = hash1;
        //         target_mac = hdr.ethernet.src_mac;
        //         origin_mac = hdr.ethernet.dst_mac;
        //         hash.apply(hdr.tcpmig.client_ip, hdr.tcpmig.client_port, hash1);
        //         hash2 = hash1;    
        //     }else{
        //         hash.apply(hdr.ipv4.src_ip, hdr.tcp.src_port, hash1);
        //         hash.apply(hdr.ipv4.dst_ip, hdr.tcp.dst_port, hash2);
        //     }
        //     // hash.apply(hdr, meta.load, hash1, hash2);

        //     request_client_ip_0.apply(hash1, hdr, meta, holder_1b_00);
        //     request_client_port_0.apply(hash1, hdr, meta, holder_1b_01);
        //     reply_client_ip_0.apply(hash2, hdr, meta, holder_1b_02);
        //     reply_client_port_0.apply(hash2, hdr, meta, holder_1b_03);

        //     meta.result00 = holder_1b_00;

        //     request_client_ip_1.apply(hash1, hdr, meta, holder_1b_10);
        //     request_client_port_1.apply(hash1, hdr, meta, holder_1b_11);
        //     reply_client_ip_1.apply(hash2, hdr, meta, holder_1b_12);
        //     reply_client_port_1.apply(hash2, hdr, meta, holder_1b_13);

        //     //TODO: return value no need for identifier, so remove it
        //     meta.result01 = holder_1b_01;
        //     meta.result02 = holder_1b_02;
        //     meta.result03 = holder_1b_03;
        //     meta.result10 = holder_1b_10;
        //     meta.result11 = holder_1b_11;
        //     meta.result12 = holder_1b_12;
        //     meta.result13 = holder_1b_13;
        //     // exec_check_val();

        //     target_mac_hi32_0.apply(hash1, target_mac[47:16], meta, hdr.ethernet.dst_mac[47:16]);
        //     target_mac_lo16_0.apply(hash1, target_mac[15:0], meta, hdr.ethernet.dst_mac[15:0]);
        //     target_ip_0.apply(hash1, hdr.ipv4.src_ip, meta, hdr.ipv4.dst_ip);
        //     target_port_0.apply(hash1, hdr.tcpmig.origin_port, meta, hdr.tcp.dst_port);


        //     origin_mac_hi32_0.apply(hash2, origin_mac[47:16], meta, hdr.ethernet.src_mac[47:16]);
        //     origin_mac_lo16_0.apply(hash2, origin_mac[15:0], meta, hdr.ethernet.src_mac[15:0]);
        //     origin_ip_0.apply(hash2, hdr.tcpmig.origin_ip, meta, hdr.ipv4.src_ip);
        //     origin_port_0.apply(hash2, hdr.tcpmig.origin_port, meta, hdr.tcp.src_port);


        //     target_mac_hi32_1.apply(hash1, target_mac[47:16], meta, hdr.ethernet.dst_mac[47:16]);
        //     target_mac_lo16_1.apply(hash1, target_mac[15:0], meta, hdr.ethernet.dst_mac[15:0]);
        //     target_ip_1.apply(hash1, hdr.ipv4.src_ip, meta, hdr.ipv4.dst_ip);
        //     target_port_1.apply(hash1, hdr.tcpmig.origin_port, meta, hdr.tcp.dst_port);


        //     origin_mac_hi32_1.apply(hash2, origin_mac[47:16], meta, hdr.ethernet.src_mac[47:16]);
        //     origin_mac_lo16_1.apply(hash2, origin_mac[15:0], meta, hdr.ethernet.src_mac[15:0]);
        //     origin_ip_1.apply(hash2, hdr.tcpmig.origin_ip, meta, hdr.ipv4.src_ip);
        //     origin_port_1.apply(hash2, hdr.tcpmig.origin_port, meta, hdr.tcp.src_port);

        // }

        l2_forwarding.apply();
        
    }
    

}  // End of SwitchIngressControl

/*********************  D E P A R S E R  ************************/

/* This struct is needed for proper digest receive API generation */
struct migration_digest_t {
    bit<48>  src_mac;
    bit<48>  dst_mac;
    bit<32>  src_ip;
    bit<32>  dst_ip;
    
    // bit<32>  dst_ip;
    // bit<16>  dst_port;
    // bit<32>  meta_ip;
    // bit<16>  meta_port;

    // bit<16>  hash_digest1;
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
    Digest <migration_digest_t>() migration_digest;

    Checksum()  ipv4_checksum;
    Checksum()  tcp_checksum;
    // Checksum()  udp_checksum;
    apply {
        if (ig_dprsr_md.digest_type == TCP_MIGRATION_DIGEST) {
            migration_digest.pack({
                    hdr.ethernet.src_mac,
                    hdr.ethernet.dst_mac,
                    hdr.ipv4.src_ip,
                    hdr.ipv4.dst_ip});
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

        // if (hdr.udp.isValid()) {
        //     hdr.ipv4.hdr_checksum = ipv4_checksum.update({
        //         hdr.ipv4.version,
        //         hdr.ipv4.ihl,
        //         hdr.ipv4.diffserv,
        //         hdr.ipv4.total_len,
        //         hdr.ipv4.identification,
        //         hdr.ipv4.flags,
        //         hdr.ipv4.frag_offset,
        //         hdr.ipv4.ttl,
        //         hdr.ipv4.protocol,
        //         hdr.ipv4.src_ip,
        //         hdr.ipv4.dst_ip
        //     });
        //     hdr.udp.checksum = udp_checksum.update({
        //         hdr.ipv4.src_ip,
        //         hdr.ipv4.dst_ip,
        //         8w0, hdr.ipv4.protocol,
        //         hdr.udp.src_port,
        //         hdr.udp.dst_port,
        //         /* Any headers past TCP */
        //         meta.l4_payload_checksum
        //     });
        // }

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
