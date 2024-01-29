#ifndef _HEADERS_
#define _HEADERS_

typedef bit<48> mac_addr_t;
typedef bit<32> ipv4_addr_t;


header ethernet_h {
	mac_addr_t dst_addr;
	mac_addr_t src_addr;
	bit<16> ether_type;
}

header arp_h {
    bit<16> htype;
    bit<16> ptype;
    bit<8> hlen;
    bit<8> plen;
    bit<16> oper;
    mac_addr_t sender_hw_addr;
    ipv4_addr_t sender_ip_addr;
    mac_addr_t target_hw_addr;
    ipv4_addr_t target_ip_addr;
}

header ipv4_h {
	bit<4> version;
    bit<4> ihl;
    bit<8> diffserv;
    bit<16> total_len;
    bit<16> identification;
    bit<3> flags;
    bit<13> frag_offset;
    bit<8> ttl;
    bit<8> protocol;
    bit<16> hdr_checksum;
    ipv4_addr_t src_addr;
    ipv4_addr_t dst_addr;
}

header tcp_h {
    bit<16> src_port;
    bit<16> dst_port;
    bit<32> seq_no;
    bit<32> ack_no;
    bit<4> data_offset;
    bit<4> res;
    bit<8> flags;
    bit<16> window;
    bit<16> checksum;
    bit<16> urgent_ptr;
}

header udp_h {
    bit<16> src_port;
    bit<16> dst_port;
    bit<16> hdr_length;
    bit<16> checksum;
}


struct header_t {
    bridged_meta_h bridged_meta; // <== To Add
	ethernet_h ethernet;
	ipv4_h ipv4;
    arp_h arp;
	tcp_h tcp;
	udp_h udp;
}

struct metadata_t {
    port_mirror_meta_t port_mirror_meta; // <== To Add
}

#endif
