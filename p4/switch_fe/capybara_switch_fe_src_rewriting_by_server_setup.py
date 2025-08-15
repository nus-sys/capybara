from bfrtcli import *
from netaddr import EUI, IPAddress


class capybara_switch_fe_src_rewriting_by_server():
    #
    # Helper Functions to deal with ports
    #
    def devport(self, pipe, port):
        return ((pipe & 3) << 7) | (port & 0x7F)
    def pipeport(self,dp):
        return ((dp & 0x180) >> 7, (dp & 0x7F))
    def mcport(self, pipe, port):
        return pipe * 72 + port
    def devport_to_mcport(self, dp):
        return self.mcport(*self.pipeport(dp))

    # This is a useful bfrt_python function that should potentially allow one
    # to quickly clear all the logical tables (including the fixed ones) in
    #  their data plane program.
    #
    # This function  can clear all P4 tables and later other fixed objects
    # (once proper BfRt support is added). As of SDE-9.2.0 the support is mixed.
    # As a result the function contains some workarounds.
    def clear_all(self, verbose=True, batching=True, clear_ports=False):
    
        table_list = bfrt.info(return_info=True, print_info=False)

        # Remove port tables from the list
        port_types = ['PORT_CFG',      'PORT_FRONT_PANEL_IDX_INFO',
                      'PORT_HDL_INFO', 'PORT_STR_INFO']
    
        if not clear_ports:
            for table in list(table_list):
                if table['type'] in port_types:
                    table_list.remove(table)
                    
                    # The order is important. We do want to clear from the top,
                    # i.e. delete objects that use other objects. For example,
                    # table entries use selector groups and selector groups
                    # use action profile members.
                    #
                    # Same is true for the fixed tables. However, the list of
                    # table types grows, so we will first clean the tables we
                    # know and then clear the rest
        for table_types in (['MATCH_DIRECT', 'MATCH_INDIRECT_SELECTOR'],
                            ['SELECTOR'],
                            ['ACTION_PROFILE'],
                            ['PRE_MGID'],
                            ['PRE_ECMP'],
                            ['PRE_NODE'],
                            []):         # This is catch-all
            for table in list(table_list):
                if table['type'] in table_types or len(table_types) == 0:
                    try:
                        if verbose:
                            print("Clearing table {:<40} ... ".
                                  format(table['full_name']),
                                  end='', flush=True)
                        table['node'].clear(batch=batching)
                        table_list.remove(table)
                        if verbose:
                            print('Done')
                        use_entry_list = False
                    except:
                        use_entry_list = True

                    # Some tables do not support clear(). Thus we'll try
                    # to get a list of entries and clear them one-by-one
                    if use_entry_list:
                        try:
                            if batching:
                                bfrt.batch_begin()
                                
                            # This line can result in an exception,
                            # since # not all tables support get()
                            entry_list = table['node'].get(regex=True,
                                                           return_ents=True,
                                                           print_ents=False)

                            # Not every table supports delete() method.
                            # For those tables we'll try to push in an
                            # entry with everything being zeroed out
                            has_delete = hasattr(table['node'], 'delete')
                            
                            if entry_list != -1:
                                if has_delete:
                                    for entry in entry_list:
                                        entry.remove()
                                else:
                                    clear_entry = table['node'].entry()
                                    for entry in entry_list:
                                        entry.data = clear_entry.data
                                        # We can still have an exception
                                        # here, since not all tables
                                        # support add()/mod()
                                        entry.push()
                                if verbose:
                                    print('Done')
                            else:
                                print('Empty')
                            table_list.remove(table)
                        
                        except BfRtTableError as e:
                            print('Empty')
                            table_list.remove(table)
                        
                        except Exception as e:
                            # We can have in a number of ways: no get(),
                            # no add() etc. Another reason is that the
                            # table is read-only.
                            if verbose:
                                print("Failed")
                        finally:
                            if batching:
                                bfrt.batch_end()
        bfrt.complete_operations()

    def __init__(self, default_ttl=60000):
        self.p4 = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe
        self.all_ports  = [port.key[b'$DEV_PORT']
                           for port in bfrt.port.port.get(regex=1,
                                                          return_ents=True,
                                                          print_ents=False)]
        self.l2_age_ttl = default_ttl

    def setup(self):
        self.clear_all()
        self.__init__()

        # Enable learning on SMAC
        # print("Initializing learning on SMAC ... ", end='', flush=True)
        # try:
        #     self.p4.IngressDeparser.l2_digest.callback_deregister()
        # except:
        #     pass
        # self.p4.IngressDeparser.l2_digest.callback_register(self.learning_cb)
        # print("Done")

        # Enable migration learning
        # print("Initializing learning on TCP migration ... ", end='', flush=True)
        # try:
        #     self.p4.IngressDeparser.migration_digest.callback_deregister()
        # except:
        #     pass
        # self.p4.IngressDeparser.migration_digest.callback_register(self.learning_migration)
        # print("Done")
        

        # Enable aging on SMAC
        # print("Inializing Aging on SMAC ... ", end='', flush=True)
        # self.p4.Ingress.smac.idle_table_set_notify(enable=False,
        #                                            callback=None)

        # self.p4.Ingress.smac.idle_table_set_notify(enable=True,
        #                                            callback=self.aging_cb,
        #                                            interval = 10000,
        #                                            min_ttl  = 10000,
        #                                            max_ttl  = 60000)
        # print("Done")

    @staticmethod
    def aging_cb(dev_id, pipe_id, direction, parser_id, entry):
        smac = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.smac
        dmac = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.dmac

        mac_addr = entry.key[b'hdr.ethernet.src_mac']

        print("Aging out: MAC: {}".format(mac(mac_addr)))

        entry.remove() # from smac
        try:
            dmac.delete(dst_mac=mac_addr)
        except:
            print("WARNING: Could not find the matching DMAC entry")

    @staticmethod
    def learning_cb(dev_id, pipe_id, direction, parser_id, session, msg):
        smac = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.smac
        dmac = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.dmac

        for digest in msg:
            port     = digest["ingress_port"]
            mac_move = digest["mac_move"]
            mac_addr  = digest["src_mac"]

            old_port = port ^ mac_move # Because mac_move = ingress_port ^ port

            print("MAC: {},  Port={}".format(
                mac(mac_addr), port), end="", flush=True)

            if mac_move != 0:
                print("(Move from port={})".format(old_port))
            else:
                print("(New)")

            # Since we do not have access to self, we have to use
            # the hardcoded value for the TTL :(
            smac.entry_with_smac_hit(src_mac=mac_addr,
                                     port=port,
                                     is_static=False,
                                     ENTRY_TTL=60000).push()
            dmac.entry_with_dmac_unicast(dst_mac=mac_addr,
                                         port=port).push()
        return 0

    @staticmethod
    def learning_migration(dev_id, pipe_id, direction, parser_id, session, msg):
        # migrate_request = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.migrate_request
        # migrate_reply = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe.Ingress.migrate_reply
        
        for digest in msg:
            src_mac     = digest["src_mac"]
            src_ip      = digest["src_ip"]
            src_port      = digest["src_port"]
            
            dst_mac     = digest["dst_mac"]
            dst_ip      = digest["dst_ip"]
            dst_port      = digest["dst_port"]

            hash_digest1         =   digest["hash_digest1"];
            # hash_digest         =   digest["hash_digest"];

            # print("Hash: {}\n".format(hash_digest), end="", flush=True)
            # print("src: {}:{}, dst: {}:{}, meta: {},{}\nHash1: {} and Hash2: {}\n".format(
            #     ip(src_ip), src_port, ip(dst_ip), dst_port, ip(meta_ip), meta_port, hash_digest1, hash_digest2), end="", flush=True)
            print("Hash: {}\n".format(hash_digest1), end="", flush=True)
            print("\n{}:{}:{} => {}:{}:{} \n".format(
                mac(src_mac), ip(src_ip), src_port, mac(dst_mac), ip(dst_ip), dst_port), end="", flush=True)


            # Since we do not have access to self, we have to use
            # the hardcoded value for the TTL :(
            # migrate_request.entry_with_migrate_request_hit(dst_mac=origin_mac, dst_ip=origin_ip, dst_port=origin_port,
            #                                                 migrate_mac=dst_mac, migrate_ip=dst_ip, migrate_port=dst_port, migrate_egress_port=egress_port).push()
            # migrate_reply.entry_with_migrate_reply_hit(src_mac=dst_mac, src_ip=dst_ip, src_port=dst_port,
            #                                                 migrate_mac=origin_mac, migrate_ip=origin_ip, migrate_port=origin_port).push()
        return 0

    def l2_add_smac_drop(self, vid, mac_addr):
        mac_addr = mac(mac_addr)
        self.p4.Ingress.smac.entry_with_smac_drop(
            src_mac=mac_addr).push()

def set_bcast(ports):
    # Broadcast
    bfrt.pre.node.entry(MULTICAST_NODE_ID=0, MULTICAST_RID=0,
        MULTICAST_LAG_ID=[], DEV_PORT=ports).push()
    bfrt.pre.mgid.entry(MGID=1, MULTICAST_NODE_ID=[0],
            MULTICAST_NODE_L1_XID_VALID=[False],
                MULTICAST_NODE_L1_XID=[0]).push()


p4 = bfrt.capybara_switch_fe_src_rewriting_by_server.pipe
num_backends = 12

### Setup L2 learning
sl2 = capybara_switch_fe_src_rewriting_by_server(default_ttl=10000)
sl2.setup()
set_bcast([24, 32, 36, 28, 16, 20])


for i in range(int(num_backends/3)):
    bfrt.pre.node.entry(MULTICAST_NODE_ID=10000 + i, MULTICAST_RID=10000 + i,
        MULTICAST_LAG_ID=[], DEV_PORT=[24, 28, 36]).push()

mcast_node_ids = [i for i in range(10000, 10000 + int(num_backends/3))]
xid_valid_list = [False] * int(num_backends/3)
xid_list = [0] * int(num_backends/3)
# print(mcast_node_ids)
# print(xid_valid_list)
# print(xid_list)
bfrt.pre.mgid.entry(MGID=2, MULTICAST_NODE_ID=mcast_node_ids,
        MULTICAST_NODE_L1_XID_VALID=xid_valid_list,
            MULTICAST_NODE_L1_XID=xid_list).push()


# p4.Ingress.min_workload_ip.reg.mod(REGISTER_INDEX=0, f1=IPAddress('10.0.1.9'))
# p4.Ingress.min_workload_port.reg.mod(REGISTER_INDEX=0, f1=10001)

# p4.Ingress.reg_be_idx.mod(REGISTER_INDEX=0, f1 = 0)

for i in range(65536):
    # p4.Ingress.reg_be_idx.mod(REGISTER_INDEX=i, f1 = 0)
    p4.Ingress.reg_be_idx.mod(REGISTER_INDEX=i, f1 = i % num_backends)
    # p4.Ingress.reg_be_idx.mod(REGISTER_INDEX=i, f1 = (i/2) % 4) // for redis-benchmark distribution
  
# for i in range(4):
#     p4.Ingress.backend_ip.mod(REGISTER_INDEX=i*3, f1=IPAddress('10.0.1.8'))
#     p4.Ingress.backend_port.mod(REGISTER_INDEX=i*3, f1=10000 + i)
#     p4.Ingress.backend_ip.mod(REGISTER_INDEX=i*3 + 1, f1=IPAddress('10.0.1.9'))
#     p4.Ingress.backend_port.mod(REGISTER_INDEX=i*3 + 1, f1=10000 + i)
#     p4.Ingress.backend_ip.mod(REGISTER_INDEX=i*3 + 2, f1=IPAddress('10.0.1.10'))
#     p4.Ingress.backend_port.mod(REGISTER_INDEX=i*3 + 2, f1=10000 + i)

# for i in range(num_backends):
#     p4.Ingress.backend_ip.mod(REGISTER_INDEX=i, f1=IPAddress('10.0.1.8'))
#     p4.Ingress.backend_port.mod(REGISTER_INDEX=i, f1=10000)
for i in range(num_backends):
    p4.Ingress.backend_ip.mod(REGISTER_INDEX=i, f1=IPAddress('10.0.1.9'))
    p4.Ingress.backend_port.mod(REGISTER_INDEX=i, f1=10000)
    
# dst_mac_rewrite = p4.Ingress.dst_mac_rewrite
# dst_mac_rewrite.entry_with_exec_dst_mac_rewrite(dst_ip = IPAddress('10.0.1.1'), mac_addr = EUI('ff:ff:ff:ff:ff:ff')).push()

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
# p4.Ingress.backend_ip.mod(REGISTER_INDEX=0, f1=IPAddress('10.0.1.8'))
# p4.Ingress.backend_port.mod(REGISTER_INDEX=0, f1=10000)

# p4.Ingress.backend_ip.mod(REGISTER_INDEX=1, f1=IPAddress('10.0.1.9'))
# p4.Ingress.backend_port.mod(REGISTER_INDEX=1, f1=10000)

# p4.Ingress.backend_ip.mod(REGISTER_INDEX=2, f1=IPAddress('10.0.1.8'))
# p4.Ingress.backend_port.mod(REGISTER_INDEX=2, f1=10001)

# p4.Ingress.backend_ip.mod(REGISTER_INDEX=3, f1=IPAddress('10.0.1.9'))
# p4.Ingress.backend_port.mod(REGISTER_INDEX=3, f1=10001)



# p4.Egress.reg_min_rps_server_port.mod(REGISTER_INDEX=0, f1=0)
# p4.Egress.reg_round_robin_server_port.mod(REGISTER_INDEX=0, f1=0) # f1=1 for redis load-balancing case (migrations from be0 to others)

# port_mirror_setup_file="/home/singtel/inho/Capybara/capybara/p4/includes/setup_port_mirror.py" # <== To Modify and Add
# exec(open(port_mirror_setup_file).read()) # <== To Add


bfrt.complete_operations()
