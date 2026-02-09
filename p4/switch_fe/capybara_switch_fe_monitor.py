from bfrtcli import *
from netaddr import EUI, IPAddress
import time

p4 = bfrt.capybara_switch_fe.pipe

# Configuration
NUM_PORTS = 4
BASE_PORT = 10000
TOTAL_SLOTS = 16  # Total number of WRR slots
BACKEND_IP = IPAddress('10.0.1.9')
BACKEND_MAC_HI32 = 0x08c0ebb6
BACKEND_MAC_LO16 = 0xc5ad

def calculate_weights(rps_values):
    """Calculate weights based on RPS (lower RPS = higher weight)"""
    if all(v == 0 for v in rps_values):
        return [1] * len(rps_values)

    # Use inverse proportion: weight = max_rps / rps
    max_rps = max(rps_values)
    if max_rps == 0:
        return [1] * len(rps_values)

    weights = []
    for rps in rps_values:
        if rps == 0:
            weights.append(max_rps)  # Give max weight to idle server
        else:
            weights.append(max_rps / rps)  # Inverse proportion
    return weights

def distribute_slots(weights, total_slots):
    """Distribute slots among backends based on weights"""
    total_weight = sum(weights)
    if total_weight == 0:
        return [total_slots // len(weights)] * len(weights)

    slots = []
    for w in weights:
        slots.append(max(1, int(total_slots * w / total_weight)))

    while sum(slots) < total_slots:
        max_weight_idx = weights.index(max(weights))
        slots[max_weight_idx] += 1
    while sum(slots) > total_slots:
        min_weight_idx = weights.index(min(weights))
        if slots[min_weight_idx] > 1:
            slots[min_weight_idx] -= 1
        else:
            for i in range(len(slots)):
                if slots[i] > 1:
                    slots[i] -= 1
                    break

    return slots

def update_backend_registers(p4, slots):
    """Update backend registers based on slot distribution"""
    idx = 0
    for port_offset in range(4):
        num_slots = slots[port_offset]
        for _ in range(num_slots):
            p4.Ingress.backend_port.mod(REGISTER_INDEX=idx, f1=10000 + port_offset)
            idx += 1
    bfrt.complete_operations()

try:
    update_count = 0
    print_count = 0
    last_print_time = time.time()
    current_slots = [4, 4, 4, 4]

    while True:
        update_count += 1

        # Read reg_active
        active_entry = p4.Egress.reg_active.get(REGISTER_INDEX=0, from_hw=True, print_ents=False, return_ents=True)
        active_val = active_entry.data[b'Egress.reg_active.f1'][0]

        # Read reg_sum_rps
        sum_vals = []
        for i in range(2):
            entry = p4.Egress.reg_sum_rps.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
            sum_vals.append(entry.data[b'Egress.reg_sum_rps.f1'][0])

        # Read reg_min_rps
        min_rps_entry = p4.Egress.reg_min_rps.get(REGISTER_INDEX=0, from_hw=True, print_ents=False, return_ents=True)
        min_rps_val = min_rps_entry.data[b'Egress.reg_min_rps.f1'][0]

        # Read reg_min_rps_server_port
        min_port_entry = p4.Egress.reg_min_rps_server_port.get(REGISTER_INDEX=0, from_hw=True, print_ents=False, return_ents=True)
        min_port_val = min_port_entry.data[b'Egress.reg_min_rps_server_port.f1'][0]

        # Read individual RPS
        rps_vals = []
        for i in range(BASE_PORT, BASE_PORT + NUM_PORTS):
            if active_val == 0:
                entry = p4.Egress.reg_individual_rps_0.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                rps_vals.append(entry.data[b'Egress.reg_individual_rps_0.f1'][0])
            else:
                entry = p4.Egress.reg_individual_rps_1.get(REGISTER_INDEX=i, from_hw=True, print_ents=False, return_ents=True)
                rps_vals.append(entry.data[b'Egress.reg_individual_rps_1.f1'][0])

        # Calculate weights and slots
        weights = calculate_weights(rps_vals)
        new_slots = distribute_slots(weights, TOTAL_SLOTS)

        # Update backend registers if slots changed
        if new_slots != current_slots:
            update_backend_registers(p4, new_slots)
            current_slots = new_slots

        # Print every 1 second
        now = time.time()
        if now - last_print_time >= 1.0:
            print_count += 1
            last_print_time = now

            print("\n" + "="*70)
            print("RPS Status (print: {}, updates: {})".format(print_count, update_count))
            print("="*70)

            print("\n[reg_active] idx[0] : {}".format(active_val))

            print("\n[reg_sum_rps]")
            for i in range(2):
                print("  idx[{}] : {}".format(i, sum_vals[i]))

            print("\n[reg_min_rps] : {}".format(min_rps_val))
            print("[reg_min_rps_server_port] : {}".format(min_port_val))

            print("\n[reg_individual_rps_{}] with WRR weights".format(active_val))
            print("  {:>8}  {:>10}  {:>10}  {:>10}".format("Port", "RPS", "Weight", "Slots"))
            print("  " + "-"*44)
            for i in range(NUM_PORTS):
                port = BASE_PORT + i
                print("  {:>8}  {:>10}  {:>10}  {:>10}".format(
                    port, rps_vals[i], weights[i], current_slots[i]))

            print("\n" + "="*70)

except KeyboardInterrupt:
    print("\nStopped.")
