/* -*- P4_16 -*- */

#include "./capybara_header.h"

#include <core.p4>
#include <tna.p4>


/******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_headers_t {
    ethernet_h  ethernet;
    vlan_tag_h   vlan_tag;
}

struct my_ingress_metadata_t {
    bit<9> mac_move;
    bit<1> is_static;
    bit<1> smac_hit;
    PortId_t ingress_port;
}

/***********************  P A R S E R  **************************/
parser IngressParser(
    packet_in pkt,
    out my_ingress_headers_t hdr,
    out my_ingress_metadata_t meta,
    out ingress_intrinsic_metadata_t ig_intr_md){

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
        transition parse_ethernet;
    }

    state parse_ethernet {
        pkt.extract(hdr.ethernet);
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
            hdr.ethernet.src_addr : exact;
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
            hdr.ethernet.dst_addr : exact;
        }
        actions = {
            dmac_unicast; dmac_miss; dmac_drop;
        }
        size           = MAC_TABLE_SIZE;
        default_action = dmac_miss();
    }

    apply {
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

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    Digest <l2_digest_t>() l2_digest;

    apply {
        if (ig_dprsr_md.digest_type == L2_LEARN_DIGEST) {
            l2_digest.pack({
                    hdr.ethernet.src_addr,
                    meta.ingress_port,
                    meta.mac_move,
                    meta.is_static,
                    meta.smac_hit });
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
    vlan_tag_h   vlan_tag;
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
            ETHERTYPE_TPID:  parse_vlan_tag;
            default: accept;
        }
    }

    state parse_vlan_tag {
        pkt.extract(hdr.vlan_tag);
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
