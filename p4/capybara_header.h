#include <core.p4>
#include <tna.p4>

#define ETHERTYPE_TPID    0x8100
#define ETHERTYPE_IPV4  0x0800
#define ETHERTYPE_ARP   0x0806
#define ETHERTYPE_PKTGEN 16w0x7777
#define IP_PROTOCOL_UDP 17
#define IP_PROTOCOL_TCP 6

#define IPV4_HOST_SIZE 1024

#define MIGRATION_SIGNATURE 0xCAFEDEAD

const int MAC_TABLE_SIZE        = 65536;
const bit<3> L2_LEARN_DIGEST = 1;
const bit<3> TCP_MIGRATION_DIGEST = 2;

/*************************************************************************
 ***********************  H E A D E R S  *********************************
 *************************************************************************/

/*  Define all the headers the program will recognize             */
/*  The actual sets of headers processed by each gress can differ */

/* Standard ethernet header */
header ethernet_h {
    bit<48>   dst_mac;
    bit<48>   src_mac;
    bit<16>   ether_type;
}

header remaining_ethernet_h {
    bit<48> src_mac;
    bit<16> ether_type;
}

header vlan_tag_h {
    bit<3>   pcp;
    bit<1>   cfi;
    bit<12>  vid;
    bit<16>  ether_type;
}

header arp_h {
    bit<16> htype;
    bit<16> ptype;
    bit<8> hlen;
    bit<8> plen;
    bit<16> oper;
    bit<48> sender_hw_addr;
    bit<32> sender_ip_addr;
    bit<48> target_hw_addr;
    bit<32> target_ip_addr;
}

header ipv4_h {
    bit<4>   version;
    bit<4>   ihl;
    bit<8>   diffserv;
    bit<16>  total_len;
    bit<16>  identification;
    bit<3>   flags;
    bit<13>  frag_offset;
    bit<8>   ttl;
    bit<8>   protocol;
    bit<16>  hdr_checksum;
    bit<32>  src_ip;
    bit<32>  dst_ip;
} // 20

header tcp_h {
    bit<16>  src_port;
    bit<16>  dst_port;
    bit<32>  seq_no;
    bit<32>  ack_no;
    bit<4>   data_offset;
    bit<4>   res;
    bit<8>   flags;
    bit<16>  window;
    bit<16>  checksum;
    bit<16>  urgent_ptr;
} // 20

header udp_h {
    bit<16>  src_port;
    bit<16>  dst_port;
    bit<16>  len;
    bit<16>  checksum;
}


header tcp_migration_header_h {
    bit<32>  signature;

    bit<32>  origin_ip;
    bit<16>  origin_port;
    
    bit<32>  dst_ip;
    bit<16>  dst_port;
    
    bit<32>  client_ip;
    bit<16>  client_port;
    
    bit<8>  flag;
    bit<8> checksum;
}
