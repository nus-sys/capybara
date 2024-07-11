# manufactured base on cluster hardware sheet 2022.12.04
from bfrtcli import *
from netaddr import EUI, IPAddress
class port_forward():
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
        port = bfrt.port.port
        port.clear()
        for entry in bfrt.pre.node.dump(return_ents=True) or []:
            entry.remove()
        for entry in bfrt.pre.mgid.dump(return_ents=True) or []:
            entry.remove()
        
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
        self.p4 = bfrt.port_forward.pipe
        # self.all_ports  = [port.key[b'$DEV_PORT']
        #                    for port in bfrt.port.port.get(regex=1,
        #                                                   return_ents=True,
        #                                                   print_ents=False)]
        self.l2_age_ttl = default_ttl
        
    def setup(self):
        self.clear_all()
        self.__init__()

    


def set_bcast(ports):
    # Broadcast
    bfrt.pre.node.entry(MULTICAST_NODE_ID=0, MULTICAST_RID=0,
        MULTICAST_LAG_ID=[], DEV_PORT=ports).push()
    bfrt.pre.mgid.entry(MGID=1, MULTICAST_NODE_ID=[0],
            MULTICAST_NODE_L1_XID_VALID=[False],
                MULTICAST_NODE_L1_XID=[0]).push()


p4 = bfrt.port_forward.pipe

### Setup L2 learning
sl2 = port_forward(default_ttl=10000)
sl2.setup()

routes = (
    (0x08c0ebb6cd5d, 32, '100G'), # node7
    (0x08c0ebb6e805, 36, '100G'), # node8
    (0x08c0ebb6c5ad, 24, '100G'), # node9
    #
)
port = bfrt.port.port
for _, dev_port, speed in routes:
    formatted_speed = 'BF_SPEED_{}'.format(speed)
    port.add(
        DEV_PORT=dev_port,
        PORT_ENABLE=True,
        SPEED=formatted_speed,
        FEC='BF_FEC_TYP_NONE',
        AUTO_NEGOTIATION='PM_AN_FORCE_DISABLE'
    )


p4.Ingress.l2_forwarding_decision.clear()
p4.Ingress.l2_forwarding_decision.add_with_broadcast(dst_addr=0xffffffffffff)
for addr, port, _ in routes:
    p4.Ingress.l2_forwarding_decision.add_with_l2_forward(dst_addr=addr, port=port)
    bfrt.pre.node.add(port, port, [], [port])
    bfrt.pre.prune.mod(port, [port])
bfrt.pre.mgid.add(
    1,
    [port for _, port, _ in routes],
    [False for _ in routes],
    [0 for _ in routes])
bfrt.complete_operations()