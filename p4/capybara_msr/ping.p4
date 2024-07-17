/* -*- P4_16 -*- */
// #include <core.p4>
// #if __TARGET_TOFINO__ == 3
// #include <t3na.p4>
// #elif __TARGET_TOFINO__ == 2
// #include <t2na.p4>
// #else
// #include <tna.p4>
// #endif
#include "../capybara_header.h"

/*************************************************************************
 ************* C O N S T A N T S    A N D   T Y P E S  *******************
**************************************************************************/

#define BIP_p40_p42 0xc613c923 // 198.19.201.35
#define DIP_p40 0xc613c828 // 198.19.200.40
#define DIP_p42 0xc613c82a // 198.19.200.42
#define VIP 0xc613c922 // 198.19.201.34

/*************************************************************************
 ***********************  H E A D E R S  *********************************
 *************************************************************************/

/*  Define all the headers the program will recognize             */
/*  The actual sets of headers processed by each gress can differ */

/* Standard ethernet header */
// header ethernet_h {
//     bit<48>   dst_mac;
//     bit<48>   src_mac;
//     bit<16>   ether_type;
// }


// header ipv4_h {
//     bit<4>   version;
//     bit<4>   ihl;
//     bit<8>   diffserv;
//     bit<16>  total_len;
//     bit<16>  identification;
//     bit<3>   flags;
//     bit<13>  frag_offset;
//     bit<8>   ttl;
//     bit<8>   protocol;
//     bit<16>  hdr_checksum;
//     bit<32>  src_ip;
//     bit<32>  dst_ip;
// } // 20

header ipv4_options_h {
    varbit<320> data;
}


/*************************************************************************
 **************  I N G R E S S   P R O C E S S I N G   *******************
 *************************************************************************/

    /***********************  H E A D E R S  ************************/

struct my_ingress_headers_t {
    ethernet_h           ethernet;
    ipv4_h               ipv4;
    ipv4_options_h     ipv4_options;
    tcp_h                       tcp;
    udp_h                       udp;
}

    /******  G L O B A L   I N G R E S S   M E T A D A T A  *********/

struct my_ingress_metadata_t {
    bit<48> src_mac;
    bit<48> dst_mac;
    bit<16> l4_payload_checksum;
}

    /***********************  P A R S E R  **************************/
parser IngressParser(packet_in        pkt,
    /* User */
    out my_ingress_headers_t          hdr,
    out my_ingress_metadata_t         meta,
    /* Intrinsic */
    out ingress_intrinsic_metadata_t  ig_intr_md)
{
    Checksum() tcp_checksum;

    /* This is a mandatory state, required by Tofino Architecture */
     state start {
        meta.l4_payload_checksum  = 0;
        
        pkt.extract(ig_intr_md);
        pkt.advance(PORT_METADATA_SIZE);
	    pkt.extract(hdr.ethernet);
        
        meta.src_mac = hdr.ethernet.src_mac;
        meta.dst_mac = hdr.ethernet.dst_mac;

        transition select(hdr.ethernet.ether_type){
            0x0800: parse_ipv4;
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

        transition select(hdr.ipv4.ihl) {
            0x5 : parse_ipv4_no_options;
            0x6 &&& 0xE : parse_ipv4_options;
            0x8 &&& 0x8 : parse_ipv4_options;
            /* 
             * Packets with other values of IHL are illegal and will be
             * dropped by the parser
             */
        }
    }

    state parse_ipv4_options {
        pkt.extract(
            hdr.ipv4_options,
            ((bit<32>)hdr.ipv4.ihl - 5) * 32);
        
        transition parse_ipv4_no_options;
    }

    state parse_ipv4_no_options {
        // parser_md.l4_lookup = pkt.lookahead<l4_lookup_t>();
        
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
        transition select(pkt.lookahead<bit<32>>()) {
            // MIGRATION_SIGNATURE: parse_tcpmig;
            default: accept;
        }
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

    // action l2_forward(PortId_t port) {
    //     ig_tm_md.ucast_egress_port=port;
    // }

    // action broadcast() {
    //     ig_tm_md.mcast_grp_a = 1;
    //     ig_tm_md.level2_exclusion_id = ig_intr_md.ingress_port;
    // }

    // table l2_forwarding_decision {
    //     key = {
    //         hdr.ethernet.dst_addr : exact;
    //     }
    //     actions = {
    //         l2_forward;
    //         broadcast;
    //     }
    // }
    action exec_return_to_7050a() {
        hdr.ethernet.src_mac = meta.dst_mac;
        hdr.ethernet.dst_mac = meta.src_mac;
        ig_tm_md.ucast_egress_port = ig_intr_md.ingress_port;
    }

    apply {
        if(hdr.udp.isValid()){
            hdr.udp.checksum = 0;
        }

        exec_return_to_7050a();
        if(hdr.ipv4.dst_ip == VIP){
            hdr.ipv4.src_ip = BIP_p40_p42;
            hdr.ipv4.dst_ip = DIP_p42;
        }
        else if (hdr.ipv4.dst_ip == BIP_p40_p42){
            hdr.ipv4.src_ip = VIP;
            hdr.ipv4.dst_ip = DIP_p40;
        }
    }
}

    /*********************  D E P A R S E R  ************************/

control IngressDeparser(packet_out pkt,
    /* User */
    inout my_ingress_headers_t                       hdr,
    in    my_ingress_metadata_t                      meta,
    /* Intrinsic */
    in    ingress_intrinsic_metadata_for_deparser_t  ig_dprsr_md)
{
    Checksum() ipv4_checksum; 
    Checksum()  tcp_checksum;

    apply {
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
            hdr.ipv4.dst_ip,
            hdr.ipv4_options.data
        });
        if (hdr.tcp.isValid()) {
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
Pipeline(
    IngressParser(),
    Ingress(),
    IngressDeparser(),
    EgressParser(),
    Egress(),
    EgressDeparser()
) pipe;

Switch(pipe) main;
