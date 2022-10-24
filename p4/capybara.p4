/* -*- P4_16 -*- */

#include "./capybara_header.h"

#include <core.p4>
#include <tna.p4>


/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    ethernet_h                  ethernet;
    ipv4_h                      ipv4;
    tcp_h                       tcp;
    tcp_migration_header_h      tcp_migration_header;
}

struct my_ingress_metadata_t {
    bit<9> mac_move;
    bit<1> is_static;
    bit<1> smac_hit;
    PortId_t ingress_port;
    bit<32> origin_ip;
    bit<16> l4_payload_checksum;
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
        meta.mac_move = 0;
        meta.is_static = 0;
        meta.smac_hit = 0;
        meta.ingress_port = ig_intr_md.ingress_port;
        meta.origin_ip = 0;
        meta.l4_payload_checksum  = 0;
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
            TCP_MIGRATION_FLAG: parse_tcp_migration_header;
            default: accept;
        }
    }

    state parse_tcp_migration_header {
        pkt.extract(hdr.tcp_migration_header);
        meta.origin_ip = hdr.tcp_migration_header.origin_ip;
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
    action send(PortId_t port) {
        ig_tm_md.ucast_egress_port = port;
    }

    action drop() {
        ig_dprsr_md.drop_ctl = 1;
    }

    action smac_hit(PortId_t port, bit<1> is_static) {
        meta.mac_move  = ig_intr_md.ingress_port ^ port;
        meta.smac_hit  = 1;
        meta.is_static = is_static;
    }

    action smac_miss() { }

    action smac_drop() {
        drop(); exit;
    }

    @idletime_precision(3)
    table smac {
        key = {
            hdr.ethernet.src_mac : exact;
        }
        actions = {
            smac_hit; smac_miss; smac_drop;
        }
        size                 = MAC_TABLE_SIZE;
        const default_action = smac_miss();
        idle_timeout         = true;
    }

    action mac_learn_notify() {
        ig_dprsr_md.digest_type = L2_LEARN_DIGEST;
    }

    table smac_results {
        key = {
            meta.mac_move  : ternary;
            meta.is_static : ternary;
            meta.smac_hit  : ternary;
        }
        actions = {
            mac_learn_notify; NoAction; smac_drop;
        }
        const entries = {
            ( _, _, 0) : mac_learn_notify();
            ( 0, _, 1) : NoAction();
            ( _, 0, 1) : mac_learn_notify();
            ( _, 1, 1) : smac_drop();
        }
    }

    action dmac_unicast(PortId_t port) {
        send(port);
    }

    action dmac_miss() {
        ig_tm_md.mcast_grp_a = 1;
    }

    action dmac_drop() {
        drop();
        exit;
    }

    table dmac {
        key = {
            hdr.ethernet.dst_mac : exact;
        }
        actions = {
            dmac_unicast; dmac_miss; dmac_drop;
        }
        size           = MAC_TABLE_SIZE;
        default_action = dmac_miss();
    }

    Register< bit<32>, bit<8> >(1, 0) reg_ip;  // value, key
    RegisterAction< bit<32>, bit<8>, bit<32> >(reg_ip)
    reg_write_ip = {
        void apply(inout bit<32> register_data, out bit<32> ip_addr) {
            register_data = meta.origin_ip;
            ip_addr = register_data;
        }
    };
    action exec_write_ip(){
        reg_write_ip.execute(0);
    }

    Register<bit<32>, _> (32w1) counter;
    RegisterAction<bit<32>, _, bit<32>>(counter) counter_update = {
        void apply(inout bit<32> val, out bit<32> rv) {
            rv = val;
            val = val + 1;
        }
    };



    action migrate_request_hit(bit<48> migrate_mac, bit<32> migrate_ip, bit<16> migrate_port) {
        hdr.ethernet.dst_mac = migrate_mac;
        hdr.ipv4.dst_ip = migrate_ip;
        hdr.tcp.dst_port = migrate_port;
        ig_tm_md.ucast_egress_port = 24;
    }

    table migrate_request {
        key = {
            hdr.ethernet.dst_mac  : exact;
            hdr.ipv4.dst_ip : exact;
            hdr.tcp.dst_port  : exact;
        }
        actions = {
            migrate_request_hit; NoAction;
        }
        size           = 65536;
        default_action = NoAction();
    }

    action migrate_reply_hit(bit<48> migrate_mac, bit<32> migrate_ip, bit<16> migrate_port) {
        hdr.ethernet.src_mac = migrate_mac;
        hdr.ipv4.src_ip = migrate_ip;
        hdr.tcp.src_port = migrate_port;
    }

    table migrate_reply {
        key = {
            hdr.ethernet.src_mac  : exact;
            hdr.ipv4.src_ip : exact;
            hdr.tcp.src_port  : exact;
        }
        actions = {
            migrate_reply_hit; NoAction;
        }
        size           = 65536;
        default_action = NoAction();
    }

    apply {
        // hdr.ipv4.hdr_checksum = 0;
        // hdr.tcp.checksum = 0;
        ig_tm_md.bypass_egress = 1w1;

        smac.apply();
        smac_results.apply();

        switch (dmac.apply().action_run) {
            dmac_unicast: { /* Unicast source pruning */
                if (ig_intr_md.ingress_port == ig_tm_md.ucast_egress_port) {
                    drop();
                }
            }
        }

        if(hdr.tcp.isValid()){
            migrate_reply.apply();
            migrate_request.apply();
        }
        if(hdr.tcp_migration_header.isValid()){
            ig_dprsr_md.digest_type = TCP_MIGRATION_DIGEST;
            counter_update.execute(0);
            exec_write_ip();
        }

    }

}  // End of SwitchIngressControl

/*********************  D E P A R S E R  ************************/

/* This struct is needed for proper digest receive API generation */
struct l2_digest_t {
    bit<48> src_mac;
    bit<9>  ingress_port;
    bit<9>  mac_move;
    bit<1>  is_static;
    bit<1>  smac_hit;
}
struct migration_digest_t {
    bit<48>  origin_mac;
    bit<32>  origin_ip;
    bit<16>  origin_port;

    bit<48>  dst_mac;
    bit<32>  dst_ip;
    bit<16>  dst_port;
}

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    Digest <l2_digest_t>() l2_digest;
    Digest <migration_digest_t>() migration_digest;

    Checksum()  ipv4_checksum;
    Checksum()  tcp_checksum;
    apply {
        if (ig_dprsr_md.digest_type == L2_LEARN_DIGEST) {
            l2_digest.pack({
                    hdr.ethernet.src_mac,
                    meta.ingress_port,
                    meta.mac_move,
                    meta.is_static,
                    meta.smac_hit });
        }else if (ig_dprsr_md.digest_type == TCP_MIGRATION_DIGEST) {
            migration_digest.pack({
                    hdr.ethernet.src_mac,
                    hdr.ipv4.src_ip,
                    hdr.tcp_migration_header.origin_port,
                    
                    hdr.ethernet.dst_mac,
                    hdr.ipv4.dst_ip,
                    hdr.tcp_migration_header.dst_port });
        }


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
