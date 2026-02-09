from scapy.all import *

MINSIZE = 60
DEV_ID = 0
PKTGEN_PIPE_PORT = 68
ETHERTYPE_PKTGEN = 0x7777
UDP_DSTPORT_PKTGEN = 7777
RPS_SIGNAL_SIGNATURE = 0xABCDABCD

def make_port(pipe, local_port):
    assert pipe >= 0 and pipe < 4
    assert local_port >= 0 and local_port < 72
    return pipe << 7 | local_port


class rps_signal(Packet):
   fields_desc = [ 
                    BitField("signature", 0, 32),
                    BitField("sum", 0, 32),
                    BitField("individual", 0, 32),
                ]

pktgen.enable( make_port(0,68)) # port 68 on logic pipeline 1

#pkt = simple_eth_packet(pktlen=pktlen, eth_dst=DST_MAC_ADDR, eth_type=ETHERTYPE_PKTGEN)
pkt = (Ether(src="08:c0:eb:b6:e8:05", dst="ff:ff:ff:ff:ff:ff", type=ETHERTYPE_PKTGEN)/ # 14
     IP(src="10.0.1.8", dst="10.0.1.9")/ # 20
     UDP(sport=22222, dport=UDP_DSTPORT_PKTGEN, chksum=0)/ # 8 
     rps_signal(signature=RPS_SIGNAL_SIGNATURE)) 
# pkt = pkt/("1" * (pktlen - len(pkt)))

if len(pkt) < MINSIZE:
    diff = MINSIZE - len(pkt)
    append_payload = b"\x00" * diff
    pkt = pkt/append_payload

pktgen.write_pkt_buffer( 0, len(pkt)-6, str(pkt)[6:], sess_hdl=sess_hdl, dev_tgt=dev_pipe(0))

# batch_count = 2
# pktlen = 60

app_cfg = pktgen.AppCfg_t()
app_cfg.trigger_type = pktgen.TriggerType_t.TIMER_PERIODIC
app_cfg.batch_count= 0
app_cfg.pkt_count= 0
app_cfg.pattern_key= 0
app_cfg.pattern_msk= 0
app_cfg.timer= 10000000 # nanoseconds
app_cfg.ibg= 1
app_cfg.ibg_jitter= 0
app_cfg.ipg= 0
app_cfg.ipg_jitter= 0
app_cfg.src_port= PKTGEN_PIPE_PORT
app_cfg.src_port_inc= 1
app_cfg.buffer_offset= 0
app_cfg.length= len(pkt)-6


## For Tofino2 setup, our logical pipe id is 1 (see 'ports' cmd on bf-sde), same to Tofino1 setup
pktgen.cfg_app( 1, app_cfg, sess_hdl=sess_hdl, dev_tgt=dev_pipe(0) )  # 1 is the app id


## Start the traffic generation
pktgen.app_enable( 1, sess_hdl=sess_hdl, dev_tgt=dev_pipe(0)) # 1 is the app id

conn_mgr.complete_operations(sess_hdl=sess_hdl)

## To check how pktgen app is working
# pktgen.show_counters(same=True)

## To stop the pktgen app
# pktgen.app_disable(1, sess_hdl=sess_hdl, dev_tgt=dev_pipe(0))