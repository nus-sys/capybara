
PROG_NAME="capybara_switch_fe"
INGRESS_CONTROL_NAME="Ingress"
EGRESS_CONTROL_NAME="Egress"
port_mirroring_cfgs = [
    #{"src_devport": 32, "mirror_devport": 36, "ig_mirror": False, "eg_mirror": True},
    #{"src_devport": 24, "mirror_devport": 36, "ig_mirror": False, "eg_mirror": True},
    # {"src_devport": 32, "mirror_devport": 8, "ig_mirror": True, "eg_mirror": False},
    {"src_devport": 24, "mirror_devport": 8, "ig_mirror": True, "eg_mirror": True},
] 


INITIAL_SESSION_ID = 1
exec("tbl_ingress_mirror = bfrt." + PROG_NAME + ".pipe." + INGRESS_CONTROL_NAME + ".port_mirroring_ingress.ingress_mirror")
exec("tbl_egress_mirror = bfrt." + PROG_NAME + ".pipe." + EGRESS_CONTROL_NAME +".port_mirroring_egress.egress_mirror")

sess_id = INITIAL_SESSION_ID

for cfg in port_mirroring_cfgs:
    src_devport = cfg["src_devport"]
    mirror_devport = cfg["mirror_devport"]

    ig_mirror = cfg["ig_mirror"]
    eg_mirror = cfg["eg_mirror"]

    if not ig_mirror and not eg_mirror:
        print("Skipping src devport {} since no mirroring configured".format(src_devport))
        continue

    if ig_mirror:
        # ingress mirroring
        tbl_ingress_mirror.add_with_do_ingress_mirror(ingress_port=src_devport, ig_mirror_sess_id=sess_id)

    if eg_mirror:
        # egress mirroring
        tbl_egress_mirror.add_with_do_egress_mirror(egress_port=src_devport, eg_mirror_sess_id=sess_id)


    bfrt.mirror.cfg.add_with_normal(
        sid=sess_id,
        direction='BOTH', # 'INGRESS'
        session_enable=True,
        ucast_egress_port=mirror_devport,
        ucast_egress_port_valid=True,
        max_pkt_len=16384)
    
    sess_id += 1
