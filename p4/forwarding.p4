action remove_pktgen_hdr(){
    hdr.ethernet.setValid();
    hdr.pktgen_timer_header.setInvalid();
    hdr.remaining_ethernet.setInvalid();

    hdr.ethernet.src_mac = hdr.remaining_ethernet.src_mac;
    hdr.ethernet.dst_mac = 0xffffffffffff;
    hdr.ethernet.ether_type = ETHERTYPE_IPV4;
}

action multicast(MulticastGroupId_t mcast_grp) {
    ig_tm_md.mcast_grp_a = mcast_grp;
}

action send(PortId_t port) {
    meta.egress_port = port;
    ig_tm_md.ucast_egress_port = port;
}

action drop() {
    ig_dprsr_md.drop_ctl = 1;
}

action broadcast() {
    ig_tm_md.mcast_grp_a       = 1;
    ig_tm_md.level2_exclusion_id = ig_intr_md.ingress_port;
}

table l2_forwarding {
    key = {
        hdr.ethernet.dst_mac : exact;
    }
    actions = {
        send;
        drop;
        broadcast;
        // l2_forward;
    }
    const entries = {
        0xb8cef62a2f95 : send(8);
        0xb8cef62a45fd : send(12);
        0xb8cef62a3f9d : send(16);
        0xb8cef62a30ed : send(20);
        0x1070fdc8944d : send(0);
        0x08c0ebb6cd5d : send(32);
        0x08c0ebb6e805 : send(36);
        0x08c0ebb6c5ad : send(24);
        0xffffffffffff : broadcast();
    }
    default_action = drop();
    size = 16;
}