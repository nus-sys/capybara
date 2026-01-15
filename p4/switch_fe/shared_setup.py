def setup_tbl_rewrite_dst_mac(p4):
    """
    Common MAC rewrite table setup for all switch_fe programs.
    Maps destination IP to MAC address and egress port.
    """
    from netaddr import EUI, IPAddress
    tbl_rewrite_dst_mac = p4.Ingress.tbl_rewrite_dst_mac
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.5'),
        dstmac    = EUI('1c:34:da:5e:0e:d8'),
        port      = 16
    ).push()
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.6'),
        dstmac    = EUI('1c:34:da:5e:0e:d4'),
        port      = 20
    ).push()
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.7'),
        dstmac    = EUI('08:c0:eb:b6:cd:5d'),
        port      = 32
    ).push()
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.8'),
        dstmac    = EUI('08:c0:eb:b6:e8:05'),
        port      = 36
    ).push()
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.9'),
        dstmac    = EUI('08:c0:eb:b6:c5:ad'),
        port      = 24
    ).push()
    tbl_rewrite_dst_mac.entry_with_rewrite_dst_mac(
        dst_ip   = IPAddress('10.0.1.10'),
        dstmac    = EUI('08:c0:eb:b6:e7:e5'),
        port      = 28
    ).push()
