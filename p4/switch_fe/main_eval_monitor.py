from bfrtcli import *
from netaddr import EUI, IPAddress
import select

p4 = bfrt.main_eval.pipe

try:
    count = 0
    prev_values = None
    while True:
        count += 1

        # Read all values
        active_entry = p4.Egress.reg_active.get(REGISTER_INDEX=0, from_hw=True, print_ents=False, return_ents=True)
        active_val = active_entry.data[b'Egress.reg_active.f1'][0]

        sum_vals = []
        for i in range(2):
            entry = p4.Egress.reg_sum_rps.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
            sum_vals.append(entry.data[b'Egress.reg_sum_rps.f1'][0])

        individual_vals = []
        for i in range(10000, 10004):
            if active_val == 0:
                node8_entry = p4.Egress.reg_individual_rps_node8_0.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                node9_entry = p4.Egress.reg_individual_rps_node9_0.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                node10_entry = p4.Egress.reg_individual_rps_node10_0.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                individual_vals.append((
                    node8_entry.data[b'Egress.reg_individual_rps_node8_0.f1'][0],
                    node9_entry.data[b'Egress.reg_individual_rps_node9_0.f1'][0],
                    node10_entry.data[b'Egress.reg_individual_rps_node10_0.f1'][0]
                ))
            else:
                node8_entry = p4.Egress.reg_individual_rps_node8_1.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                node9_entry = p4.Egress.reg_individual_rps_node9_1.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                node10_entry = p4.Egress.reg_individual_rps_node10_1.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                individual_vals.append((
                    node8_entry.data[b'Egress.reg_individual_rps_node8_1.f1'][0],
                    node9_entry.data[b'Egress.reg_individual_rps_node9_1.f1'][0],
                    node10_entry.data[b'Egress.reg_individual_rps_node10_1.f1'][0]
                ))

        curr_values = (sum_vals, individual_vals)

        # Only print if values changed
        if True:
            print("\n" + "="*60)
            print("RPS Status (count: {})".format(count))
            print("="*60)

            print("\n[reg_active] idx[0] : {}".format(active_val))

            print("\n[reg_sum_rps]")
            for i in range(2):
                print("  idx[{}] : {}".format(i, sum_vals[i]))

            print("\n[reg_individual_rps] (index 10000-10003)")
            print("  {:>8}  {:>10}  {:>10}  {:>10}".format("Index", "Node8", "Node9", "Node10"))
            print("  " + "-"*44)
            for idx, i in enumerate(range(10000, 10004)):
                print("  {:>8}  {:>10}  {:>10}  {:>10}".format(i, individual_vals[idx][0], individual_vals[idx][1], individual_vals[idx][2]))

            print("\n" + "="*60)
            prev_values = curr_values

        select.select([], [], [], 1)
except KeyboardInterrupt:
    print("\nStopped.")
