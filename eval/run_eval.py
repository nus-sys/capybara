import argparse
import os
import time
import math
import operator
import pyrem.host
# import pyrem.task
# from pyrem.host import RemoteHost
# import pyrem
import sys
import glob

import numpy as np
import matplotlib.pyplot as plt
import matplotlib.ticker as tick
from math import factorial, exp
from datetime import datetime
import signal
import atexit
from os.path import exists
import toml

import subprocess
from cycler import cycler

import datetime
import pty


from test_config import *

final_result = ''


def kill_procs():
    cmd = [f'sudo pkill -INT -e iokerneld ; \
            sudo pkill -INT -f synthetic ; \
            sudo pkill -INT -e dpdk-ctrl.elf ; \
            sudo pkill -INT -e phttp-bench ; \
            sudo pkill -INT -f tcpdump ; \
            sudo pkill -INT -f http-server ; \
            sudo pkill -INT -f curl ; \
            sudo pkill -INT -f {SERVER_APP}']
    # print(cmd)
    if TCPDUMP:
        cmd[0] += ' ; sudo pkill -INT -f -e tcpdump'
    # cmd = [f'sudo pkill -INT -f Capybara && sleep 2 && sudo pkill -f Capybara && sudo pkill -f caladan']
    
    # print(cmd)
    for node in ALL_NODES:
        # kill_tasks = []
        # print(node)
        host = pyrem.host.RemoteHost(node)
        task = host.run(cmd, quiet=False)
        # print(task)
        # kill_tasks.append(task)
        pyrem.task.Parallel([task], aggregate=True).start(wait=True)
    
    # print(kill_tasks)
    print('KILLED CAPYBARA PROCESSES')


def run_server(mig_delay, max_reactive_migs, max_proactive_migs, mig_per_n):
    global experiment_id
    print('SETUP SWITCH')
    cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_setup.py"'] 
    if EVAL_MAINTENANCE == True:
        if 'tcp-migration' in FEATURES:
            cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
            /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/capybara_msr/capybara_nsl_setup.py"'] 
        else:
            cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
            /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/port_forward/port_forward.py"'] 
    
    if SERVER_APP == 'prism' or SERVER_APP == 'proxy-server':
        cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/prism/prism_setup.py"']
    if SERVER_APP == "capybara-switch":
        cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/endhost_switch/endhost_switch.py"']
        
        
    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    # print(result + '\n\n')

    if SERVER_APP == 'proxy-server':
        print('RUNNING FRONTEND')
        host = pyrem.host.RemoteHost(FRONTEND_NODE) 
        cmd = [f'cd {CAPYBARA_PATH} && make be-dpdk-ctrl-node8'] 
        task = host.run(cmd, quiet=True)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
        time.sleep(3)
        print('Frontend dpdk-ctrl is running')

        cmd = [f'cd {CAPYBARA_PATH} && \
            {f"taskset --cpu-list {j+1}" if LIBOS == "catnap" else ""} \
            sudo -E \
            CAPY_LOG={CAPY_LOG} \
            LIBOS={LIBOS} \
            RECV_QUEUE_LEN_THRESHOLD={RECV_QUEUE_LEN_THRESHOLD} \
            MIG_DELAY={int(mig_delay/10) * 76} \
            {f"MAX_REACTIVE_MIGS={max_reactive_migs}" if max_reactive_migs != "" else ""} \
            {f"MAX_PROACTIVE_MIGS={max_proactive_migs}" if max_proactive_migs != "" else ""} \
            MIG_PER_N={int(mig_per_n)} \
            MIN_THRESHOLD={MIN_THRESHOLD} \
            RPS_THRESHOLD={RPS_THRESHOLD} \
            THRESHOLD_EPSILON={THRESHOLD_EPSILON} \
            MTU=1500 \
            MSS=1500 \
            NUM_CORES=4 \
            RUST_BACKTRACE=full \
            CORE_ID=1 \
            CONFIG_PATH={CAPYBARA_CONFIG_PATH}/node8_config.yaml \
            LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
            PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
            numactl -m0 \
            {CAPYBARA_PATH}/bin/examples/rust/proxy-server-fe.elf 10.0.1.8:10000 \
            > {DATA_PATH}/{experiment_id}.fe 2>&1']
        task = host.run(cmd, quiet=False)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)    
        time.sleep(2)
        print(f'Frontend is running')
        
        print('RUNNING BACKENDS')
        server_tasks = []
        tasks = []
        host = pyrem.host.RemoteHost(BACKEND_NODE) 
        cmd = [f'cd {CAPYBARA_PATH} && make be-dpdk-ctrl-node9'] 
        task = host.run(cmd, quiet=True)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
        time.sleep(3)
        print('Backend dpdk-ctrl is running')

        for j in range(NUM_BACKENDS):
            cmd = [f'cd {CAPYBARA_PATH} && \
                {f"taskset --cpu-list {j+1}" if LIBOS == "catnap" else ""} \
                sudo -E \
                CAPY_LOG={CAPY_LOG} \
                LIBOS={LIBOS} \
                RECV_QUEUE_LEN_THRESHOLD={RECV_QUEUE_LEN_THRESHOLD} \
                MIG_DELAY={int(mig_delay/10) * 76} \
                {f"MAX_REACTIVE_MIGS={max_reactive_migs}" if max_reactive_migs != "" else ""} \
                {f"MAX_PROACTIVE_MIGS={max_proactive_migs}" if max_proactive_migs != "" else ""} \
                MIG_PER_N={int(mig_per_n)} \
                MIN_THRESHOLD={MIN_THRESHOLD} \
                RPS_THRESHOLD={RPS_THRESHOLD} \
                THRESHOLD_EPSILON={THRESHOLD_EPSILON} \
                MTU=1500 \
                MSS=1500 \
                NUM_CORES=4 \
                RUST_BACKTRACE=full \
                CORE_ID={j+1} \
                CONFIG_PATH={CAPYBARA_CONFIG_PATH}/node9_config.yaml \
                LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
                PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
                numactl -m0 \
                {CAPYBARA_PATH}/bin/examples/rust/proxy-server-be.elf 10.0.1.9:1000{j} 10.0.1.8:10000 \
                > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
            task = host.run(cmd, quiet=False)
            server_tasks.append(task)
        pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
        time.sleep(2)
        print(f'{NUM_BACKENDS} Backend{"s are" if NUM_BACKENDS != 1 else " is"} running')
        return
    
    elif SERVER_APP == 'capybara-switch':
        print('RUNNING CAPYBARA ENDHOST SWITCH')
        host = pyrem.host.RemoteHost(FRONTEND_NODE) 
        

        cmd = [f'cd {CAPYBARA_PATH} && \
            sudo -E \
            CAPY_LOG={CAPY_LOG} \
            LIBOS={LIBOS} \
            NUM_CORES=1 \
            MTU=1500 \
            MSS=1500 \
            NUM_BACKENDS={NUM_BACKENDS} \
            CONFIG_PATH={CAPYBARA_CONFIG_PATH}/node8_config.yaml \
            LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
            PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
            taskset --cpu-list 1 numactl -m0 \
            {CAPYBARA_PATH}/bin/examples/rust/capybara-switch.elf 10.0.1.8:10000 10.0.1.8:10001 \
            > {DATA_PATH}/{experiment_id}.capybara_switch 2>&1']
        
        task = host.run(cmd, quiet=False)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)    
        time.sleep(2)
        print(f'CAPYBARA ENDHOST SWITCH is running')
    
    print('RUNNING BACKENDS')
    host = pyrem.host.RemoteHost(BACKEND_NODE)

    if SERVER_APP != 'prism' and LIBOS == 'catnip': 
        cmd = [f'cd {CAPYBARA_PATH} && make be-dpdk-ctrl'] 
        task = host.run(cmd, quiet=True)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)    
        if EVAL_MAINTENANCE == True:
            cmd = [f'cd {CAPYBARA_PATH} && make fe-dpdk-ctrl'] 
            node8 = pyrem.host.RemoteHost(FRONTEND_NODE)
            task = node8.run(cmd, quiet=True)
            pyrem.task.Parallel([task], aggregate=True).start(wait=False)    
            print('fe-dpdk-ctrl is running')
        print('be-dpdk-ctrl is running')
        time.sleep(3)
        
        if EVAL_MAINTENANCE == True:
            run_cmd = f'make redis-server-node8'
            cmd = [f'cd {CAPYBARA_PATH} && \
                sudo -E \
                CAPY_LOG={CAPY_LOG} \
                LIBOS={LIBOS} \
                RUST_BACKTRACE=full \
                MIG_AFTER=900000 \
                numactl -m0 \
                {run_cmd} \
                > {DATA_PATH}/{experiment_id}.origin 2>&1']
            node8 = pyrem.host.RemoteHost(FRONTEND_NODE)
            task = node8.run(cmd, quiet=True)
            pyrem.task.Parallel([task], aggregate=True).start(wait=False)    
            print('origin server is running')
            
    elif SERVER_APP == 'prism': # run FE
        fe_host = pyrem.host.RemoteHost(FRONTEND_NODE)
        BE_ADDRs = f'{NODE9_IP}:10000'
        for j in range(NUM_BACKENDS-1): 
            BE_ADDRs = BE_ADDRs + f',{NODE9_IP}:1000{j+1}'
        cmd = [f'taskset --cpu-list {1} \
                sudo numactl -m0 phttp-bench-proxy \
                --addr {NODE8_IP} --port 10000 --mac {NODE8_MAC} \
                --ho-addr {NODE8_IP} --ho-port 10001 \
                --sw-addr 10.0.1.7 --sw-port 18080 \
                --backends {BE_ADDRs} \
                --backlog 8192 --ho-backlog 64 \
                --nworkers 1 \
                > {DATA_PATH}/{experiment_id}.fe 2>&1']
        task = fe_host.run(cmd, quiet=True)
        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
        time.sleep(1)
        print('Prism FE is running')
        

    server_tasks = []
    for j in range(NUM_BACKENDS):
        if SERVER_APP == 'http-server' or SERVER_APP == 'capybara-switch' or SERVER_APP == 'https':
            run_cmd = f'{CAPYBARA_PATH}/bin/examples/rust/http-server.elf 10.0.1.9:1000{j}'
            if SERVER_APP == 'https':
                run_cmd = f'{CAPYBARA_PATH}/bin/examples/rust/https.elf 10.0.1.9:1000{j}'
            
            if EVAL_MIG_DELAY == True:
                run_cmd = run_cmd + ' migrate'
            
            cmd = [f'cd {CAPYBARA_PATH} && \
                {f"taskset --cpu-list {j+1}" if LIBOS == "catnap" else ""} \
                sudo -E \
                CAPY_LOG={CAPY_LOG} \
                LIBOS={LIBOS} \
                RECV_QUEUE_LEN_THRESHOLD={RECV_QUEUE_LEN_THRESHOLD} \
                MIG_DELAY={int(mig_delay/10) * 76} \
                {f"MAX_REACTIVE_MIGS={max_reactive_migs}" if max_reactive_migs != "" else ""} \
                {f"MAX_PROACTIVE_MIGS={max_proactive_migs}" if max_proactive_migs != "" else ""} \
                MIG_PER_N={int(mig_per_n)} \
                CONFIGURED_STATE_SIZE={CONFIGURED_STATE_SIZE} \
                MIN_THRESHOLD={MIN_THRESHOLD} \
                RPS_THRESHOLD={RPS_THRESHOLD} \
                THRESHOLD_EPSILON={THRESHOLD_EPSILON} \
                MTU=1500 \
                MSS=1500 \
                NUM_CORES=4 \
                RUST_BACKTRACE=full \
                CORE_ID={j+1} \
                CONFIG_PATH={CAPYBARA_CONFIG_PATH}/node9_config.yaml \
                LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
                PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
                numactl -m0 \
                {run_cmd} \
                > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        elif SERVER_APP == 'redis-server':
            run_cmd = f'make redis-server-node9-1000{j}'
            cmd = [f'cd {CAPYBARA_PATH} && \
                {f"taskset --cpu-list {j+1}" if LIBOS == "catnap" else ""} \
                sudo -E \
                CAPY_LOG={CAPY_LOG} \
                LIBOS={LIBOS} \
                MIN_THRESHOLD={MIN_THRESHOLD} \
                RPS_THRESHOLD={RPS_THRESHOLD} \
                THRESHOLD_EPSILON={THRESHOLD_EPSILON} \
                {f"MAX_REACTIVE_MIGS={max_reactive_migs}" if max_reactive_migs != "" else ""} \
                {f"MAX_PROACTIVE_MIGS={max_proactive_migs}" if max_proactive_migs != "" else ""} \
                RUST_BACKTRACE=full \
                {f"REDIS_CONFIG=../config/node9_1000{j}.conf" if TLS == 1 else f"REDIS_CONFIG=../config/node9_1000{j}_tcp.conf"} \
                {f"REDIS_SERVER_PATH=capybara-redis-tlse" if TLS == 1 else "REDIS_SERVER_PATH=capybara-redis"} \
                {f"MIG_AFTER=10000000" if EVAL_MAINTENANCE == True else ""} \
                numactl -m0 \
                {run_cmd} \
                > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
            
        elif SERVER_APP == 'prism':
            cmd = [f'taskset --cpu-list {j+1} \
                sudo numactl -m0 phttp-bench-backend \
                --addr {NODE9_IP} --port 80 --mac {NODE9_MAC} \
                --ho-addr {NODE9_IP} --ho-port 1000{j} \
                --proxy-addr {NODE8_IP} --proxy-port 10001 \
                --sw-addr 10.0.1.7 --sw-port 18080 \
                --backlog 8192 --ho-backlog 64 \
                --nworkers 1 \
                > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        else:
            print(f'Invalid server app: {SERVER_APP}')
            exit(1)
        # print(cmd)
        task = host.run(cmd, quiet=False)
        server_tasks.append(task)
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'{NUM_BACKENDS} backends are running')

def run_tcpdump(experiment_id):
    print(f'RUNNING TCPDUMP to {TCPDUMP_NODE}:{PCAP_PATH}/{experiment_id}.pcap')
    
    host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    cmd = [f'sudo tcpdump --time-stamp-precision=nano -i ens1f1 -w {PCAP_PATH}/{experiment_id}.pcap']
    task = host.run(cmd, quiet=False)
    pyrem.task.Parallel([task], aggregate=True).start(wait=False)
    
def parse_tcpdump(experiment_id):
    print(f'PARSING {TCPDUMP_NODE}:{PCAP_PATH}/{experiment_id}.pcap') 
    
    host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    
    cmd = [f"""
            {CAPYBARA_PATH}/eval/pcap-parser.sh {PCAP_PATH}/{experiment_id}.pcap &&
            cat {PCAP_PATH}/{experiment_id}.csv | awk '{{if($16 == "GET"){{print $2}}}}' > {PCAP_PATH}/{experiment_id}.request_times &&
            cat {PCAP_PATH}/{experiment_id}.csv | awk '{{if($16 == "OK"){{print $2}}}}' > {PCAP_PATH}/{experiment_id}.response_times &&
            paste {PCAP_PATH}/{experiment_id}.request_times {PCAP_PATH}/{experiment_id}.response_times | awk '{{print $2-$1}}'  > {PCAP_PATH}/{experiment_id}.pcap_latency
    """]
    
    task = host.run(cmd, quiet=False)
    pyrem.task.Parallel([task], aggregate=True).start(wait=True)
  

def parse_mig_delay(experiment_id):
    print(f'PARSING {experiment_id} migration delay') 
    
    # host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    
    
    clusters = {}
    for i in [0, 1]:
        file_path = f'{DATA_PATH}/{experiment_id}.be{i}'
        with open(file_path, "r") as file:
            # Iterate through each line in the file
            start_printing = False
            for line in file:
                # print(line)
                # Check if the line contains the target string
                if "[CAPYLOG] dumping time log data" in line:
                    # Set the flag to start printing lines
                    start_printing = True
                    continue  # Skip this line

                # Check if we should print the line
                if start_printing:
                    columns = line.strip().split(",")
                    if len(columns) != 3:
                        continue
                    # time_str = columns[0]
                    # time_components = time_str.split(':')
                    # hours, minutes = map(int, time_components[:2])
                    # seconds = float(time_components[2])  # Convert sub-second part to a float
                    # nanosecond_timestamp = int((hours * 3600 + minutes * 60 + seconds) * 1_000_000_000)
                    # columns[0] = str(nanosecond_timestamp)

                    last_column = columns[-1]

                    if last_column in clusters:
                        clusters[last_column].append(columns[0] + ',' + columns[1] + ',' + columns[2])
                    else:
                        clusters[last_column] = [columns[0] + ',' + columns[1] + ',' + columns[2]]
    

    steps = ['',
            'INIT_MIG',  #1
            'SEND_PREPARE_MIG', #2 
            'RECV_PREPARE_MIG',  #3
            'SEND_PREPARE_MIG_ACK', #4 
            'RECV_PREPARE_MIG_ACK', #5
            # 'SERIALIZE_STATE', #6
            'SEND_STATE', #6 (including fragmentation)
            'RECV_STATE', #7 (once receiving all fragments)
            'CONN_ACCEPTED', #8
            ]
            # 'SEND_STATE_ACK', #9
            # 'RECV_STATE_ACK']
    prev_step = 0
    prev_ns = 0
    final_result = ''
    for last_column, lines in clusters.items():
        result = ''
        # print(f"Cluster for last column '{last_column}':")
        # for line in lines:
        #     print(line)
        
        # print("Sorted")
        sorted_list = sorted(lines, key=lambda x: int(x.split(",")[0]))
        init_ns = 0
        
        black_out_times = list()
        black_out_start = 0
        for item in sorted_list:
            # print(item)
            columns = item.split(',')
            ns = int(columns[0])
            step = columns[1]

            if step == 'RECV_PREPARE_MIG_ACK':
                black_out_start = ns
            
            if step == 'CONN_ACCEPTED':
                black_out_times.append(ns - black_out_start)
        
        black_out_idx = 0
        for item in sorted_list:
            # print(item)
            columns = item.split(',')
            ns = int(columns[0])
            step = columns[1]
      
            step_idx = steps.index(step)
            if step_idx != prev_step+1:
                print("[PANIC] migration step is wrong!: prev {}, current {}", prev_step, step_idx, "\n", item)
                exit()
            prev_step = step_idx

            if step_idx == 1:
                init_ns = ns
            if step_idx >= 2:
                latency = ns - prev_ns
                result = result + str(latency) + ','
            if step_idx == 8:
                result = result + str(ns - init_ns) + ',' + str(black_out_times[black_out_idx]) + "\n"
                black_out_idx += 1
                prev_step = 0
            prev_ns = ns
        # print(result)
        final_result = final_result + '\n'.join(result.split('\n')[200:])
        # final_result = final_result + '\n'.join(result.split('\n')[:])
    
    print(len(final_result.split('\n')))
    
    # print('\n'.join(final_result.split('\n')[-10001:]))

    with open(f'{DATA_PATH}/{experiment_id}.mig_delay', 'w') as file:
        # Write the content to the file
        file.write('\n'.join(final_result.split('\n')[-2001:]))
        # file.write('\n'.join(final_result.split('\n')[:]))

    
    print(f'CALCULATING {experiment_id} mig_delay CDF') 
    
    cmd = f"cd {CAPYBARA_PATH}/eval && bash mig_delay_cdf_avg_minmax_stddev.sh {experiment_id}"
    print("Executing command:", cmd)  # For debugging

    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    if result != '':
        print("ERROR: " + result + '\n\n')
    else:
        print("DONE")

  

def parse_mig_cpu_ovhd(experiment_id):
    print(f'PARSING {experiment_id} prism mig delay') 
    
    cmd = f"cd {CAPYBARA_PATH}/eval\
            && sh parse_mig_cpu_ovhd.sh {experiment_id}"
    print("Executing command:", cmd)  # For debugging

    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    if result != '':
        print("ERROR: " + result + '\n\n')
    else:
        print("DONE")
        
        
def parse_poll_interval(experiment_id):
    print(f'PARSING {experiment_id} poll_interval') 
    
    # host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    
    
    clusters = {}
    prev_ns = 0
    final_result = ''
    for i in [0, 1]:
        file_path = f'{DATA_PATH}/{experiment_id}.be{i}'
        with open(file_path, "r") as file:
            # Iterate through each line in the file
            start_printing = False
            for line in file:
                # Check if the line contains the target string
                if "[CAPYLOG] dumping time log data" in line:
                    # Set the flag to start printing lines
                    start_printing = True
                    continue  # Skip this line
                
                if "poll_dpdk_interval" not in line:
                    continue

                # Check if we should print the line
                if start_printing:
                    columns = line.strip().split(",")
                    if len(columns) != 4:
                        continue

                    time_str = columns[2]
                    time_components = time_str.split(':')
                    hours, minutes = map(int, time_components[:2])
                    seconds = float(time_components[2])  # Convert sub-second part to a float
                    nanosecond_timestamp_1 = int((hours * 3600 + minutes * 60 + seconds) * 1_000_000_000)
                    
                    time_str = columns[3]
                    time_components = time_str.split(':')
                    hours, minutes = map(int, time_components[:2])
                    seconds = float(time_components[2])  # Convert sub-second part to a float
                    nanosecond_timestamp_2 = int((hours * 3600 + minutes * 60 + seconds) * 1_000_000_000)

                    poll_interval = nanosecond_timestamp_1 - nanosecond_timestamp_2
                    final_result = final_result + str(poll_interval) + "\n"
    print(len(final_result.split('\n')))
    with open(f'{DATA_PATH}/{experiment_id}.poll_interval', 'w') as file:
        # Write the content to the file
        file.write(final_result)



def parse_latency_trace(experiment_id):
    print(f'PARSING {experiment_id} latency_trace') 
    
    cmd = f"cd {CAPYBARA_PATH}/eval\
            && sh parse_request_sched.sh {experiment_id}\
            && sh ms_avg_99p_lat.sh {experiment_id}\
            && sh ms_total_even_odd_numreq.sh {experiment_id}\
            && sh latency_cdf.sh {experiment_id}"
    print("Executing command:", cmd)  # For debugging

    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    if result != '':
        print("ERROR: " + result + '\n\n')
    else:
        print("DONE")



def parse_wrk_result(experiment_id):
    import re
    result_str = ''
    # Read the text file
    with open(f'{DATA_PATH}/{experiment_id}.client', "r") as file:
        text = file.read()

    # Use regular expressions to extract relevant values
    avg_latency_match = re.search(r"(\d+\.\d+)(us|ms|s)\s", text)
    threads_match = re.search(r"(\d+)\s+threads", text)
    connections_match = re.search(r"(\d+)\s+connections", text)
    requests_sec_match = re.search(r"Requests/sec:\s+(\d+\.\d+)", text)
    # print(avg_latency_match)
    # Check if all necessary values were found
    if not (avg_latency_match and threads_match and connections_match and requests_sec_match):
        print("Failed to parse the text file")
    else:
        threads = threads_match.group(1)
        connections = connections_match.group(1)
        requests_per_sec = requests_sec_match.group(1)
         
        avg_lat = float(avg_latency_match.group(1)) * {'us': 1, 'ms': 1000, 's': 1000000}.get(avg_latency_match.group(2), 1)
        
        result_str = f'{experiment_id}, {NUM_BACKENDS}, {int(connections)}, {threads}, {requests_per_sec.split(".")[0]}, {int(avg_lat)}'

        # Define a regular expression pattern to match the percentages and values
        pattern = r'(\d+%)\s+(\d+\.\d+(?:us|ms|s))'
        # Find all matches in the text using the regular expression pattern
        matches = re.findall(pattern, text)

        # Create a dictionary to store the parsed data
        latency_data = {}

        # Iterate through the matches and populate the dictionary
        for match in matches:
            percentile, value = match
            latency_data[percentile] = value
            pattern = r"(\d+\.\d+)(us|ms|s)"
            matches = re.search(pattern, value)
            if matches:
                percentiile_lat = float(matches.group(1)) * {'us': 1, 'ms': 1000, 's': 1000000}.get(matches.group(2), 1)
            else:
                print("PANIC: cannot find percentile latency")
                exit(1)

            result_str = result_str + f', {int(percentiile_lat)}'
        # Print the parsed data
        # for percentile, value in latency_data.items():
        #     print(f"{percentile}: {value}")

        # # Create a CSV-style string
        # csv_data = f"{threads},{connections},{requests_per_sec}"

        # # Print the CSV-style string
        # print(csv_data)
    # print(result_str)
    return result_str
        

def parse_server_reply(experiment_id):
    print(f'PARSING {experiment_id} server reply') 
    cmd = f"cd {CAPYBARA_PATH}/eval && sh parse_server_reply.sh {experiment_id}"
    print("Executing command:", cmd)  # For debugging


    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    if result != '':
        print("ERROR: " + result + '\n\n')
    else:
        print("DONE")
    return
    
    
def parse_rps_signal(experiment_id):
    print(f'PARSING {experiment_id} RPS SIGNAL') 
    
    for i in range(NUM_BACKENDS):
        file_path = f'{DATA_PATH}/{experiment_id}.be{i}'
        with open(file_path, "r") as file:
            final_result = ''
            for line in file:
                columns = line.strip().split(",")
                if len(columns) != 4:
                    continue
                if columns[1] != 'RPS_SIGNAL':
                    continue

                time_str = columns[0]
                sum_rps = columns[2]
                individual_rps = columns[3]
                
                final_result = final_result + time_str + ',' + sum_rps + ',' + individual_rps + "\n"
        with open(f'{DATA_PATH}/{experiment_id}.be{i}_rps_signal', 'w') as file:
            # Write the content to the file
            file.write(final_result)
    
    cmd = f"cd {CAPYBARA_PATH}/eval && sh workload_gap_cdf.sh {experiment_id}"
    print("Executing command:", cmd)  # For debugging


    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    if result != '':
        print("ERROR: " + result + '\n\n')
    else:
        print("DONE")
    
def run_eval():
    global experiment_id
    global final_result
    if CLIENT_APP != 'wrk' and len(NUM_THREADS) > 1:
        print("Error: NUM_THREADS is used only for wrk generator")
        kill_procs()
        exit(1)
    if CLIENT_APP == 'caladan' and LOADSHIFTS.count('|') != 0 and LOADSHIFTS.count('|') != NUM_BACKENDS - 1:
        print(f"Error: LOADSHIFT configuration is wrong (check '|')")
        kill_procs()
        exit(1)
    for repeat in range(0, REPEAT_NUM):
        for mig_delay in MIG_DELAYS:
            for max_reactive_migs in MAX_REACTIVE_MIGS: 
                for max_proactive_migs in MAX_PROACTIVE_MIGS:
                    for mig_per_n in MIG_PER_N:
                        for pps in CLIENT_PPS:
                            for conn in NUM_CONNECTIONS:
                                for num_thread in NUM_THREADS:
                                    kill_procs()
                                    experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                                    
                                    with open(f'{CAPYBARA_PATH}/eval/test_config.py', 'r') as file:
                                        print(f'================ RUNNING TEST =================')
                                        print(f'\n\nEXPTID: {experiment_id}')
                                        with open(f'{DATA_PATH}/{experiment_id}.test_config', 'w') as output_file:
                                            output_file.write(file.read())
                                    if TCPDUMP == True:
                                        run_tcpdump(experiment_id)
                                    
                                    run_server(mig_delay, max_reactive_migs, max_proactive_migs, mig_per_n)

                                    
                                    host = pyrem.host.RemoteHost(CLIENT_NODE)
                                    cmd = [f'sudo arp -f {CAPYBARA_HOME}/arp_table']
                                    task = host.run(cmd, quiet=True)
                                    pyrem.task.Parallel([task], aggregate=True).start(wait=True)
                                    
                                    if EVAL_MIG_DELAY == True:
                                        cmd = [f'cd {CAPYBARA_PATH}/eval/control && python3 run_tcp_migrate_client.py']
                                        if SERVER_APP == 'https':
                                            cmd = [f'cd {CAPYBARA_PATH}/eval/control && python3 run_tls_migrate_client.py']
                                        
                                        task = host.run(cmd, quiet=False)
                                        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                                        time.sleep(5)
                                        kill_procs()
                                        time.sleep(3)
                                        parse_mig_delay(experiment_id)
                                        exit()
                                    
                                    if EVAL_MAINTENANCE == True:
                                        client = pyrem.host.RemoteHost(CLIENT_NODE)
                                        tls_cmd = f'--tls --cert /usr/local/tls/svr.crt --key /usr/local/tls/svr.key --cacert /usr/local/tls/CA.pem'
                                        cmd = [f'sudo numactl -m0 \
                                            {CAPYBARA_HOME}/capybara-redis/src/redis-benchmark \
                                            {f"{tls_cmd}" if TLS == 1 else ""} \
                                            -h 10.0.1.8 -p 10000 \
                                            -t get -n 3000000 -c {conn} --threads {conn} \
                                            --backup-host 10.0.1.9 --backup-port 10000 \
                                            > {DATA_PATH}/{experiment_id}.client']
                                        
                                        task = client.run(cmd, quiet=False)
                                        
                                        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                                        time.sleep(5)
                                    
                                        node8 = pyrem.host.RemoteHost(FRONTEND_NODE)
                                        cmd = [f'sudo pkill -INT -e dpdk-ctrl.elf ; \
                                                sudo pkill -INT -f {SERVER_APP}']
                                        task = node8.run(cmd, quiet=False)
                                        pyrem.task.Parallel([task], aggregate=True).start(wait=True)
                                        time.sleep(10)
                                        
                                        kill_procs()
                                        exit()
                                    
                                    
                                    if CLIENT_APP == 'caladan':
                                        cmd = [f'cd {CALADAN_PATH} && sudo ./iokerneld ias nicpci 0000:31:00.1']
                                        task = host.run(cmd, quiet=True)
                                        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                                        time.sleep(3)
                                        print('iokerneld is running')
                                    
                                    if SERVER_APP == 'http-server' or SERVER_APP == 'capybara-switch' or SERVER_APP == 'prism' or SERVER_APP == 'proxy-server':
                                        if CLIENT_APP == 'wrk':
                                            cmd = [f'sudo numactl -m0 {HOME}/wrk-tools/wrk/wrk \
                                                -t{num_thread} \
                                                -c{int(conn)*int(num_thread)} \
                                                -d{RUNTIME}s \
                                                --latency \
                                                http://10.0.1.8:10000/get \
                                                > {DATA_PATH}/{experiment_id}.client']
                                        else:    
                                            cmd = [f'sudo numactl -m0 {CALADAN_PATH}/apps/synthetic/target/release/synthetic \
                                                10.0.1.8:10000 \
                                                --config {CALADAN_PATH}/client.config \
                                                --mode runtime-client \
                                                --protocol=http \
                                                --transport=tcp \
                                                --samples=1 \
                                                --pps={pps} \
                                                --threads={conn} \
                                                --runtime={RUNTIME} \
                                                --discard_pct=10 \
                                                --output=trace \
                                                --rampup=0 \
                                                {f"--loadshift={LOADSHIFTS}" if LOADSHIFTS != "" else ""} \
                                                {f"--zipf={ZIPF_ALPHA}" if ZIPF_ALPHA != "" else ""} \
                                                {f"--onoff={ONOFF}" if ONOFF == "1" else ""} \
                                                --exptid={DATA_PATH}/{experiment_id} \
                                                > {DATA_PATH}/{experiment_id}.client']
                                    elif SERVER_APP == 'redis-server':
                                        if CLIENT_APP == 'caladan':
                                            cmd = [f'sudo numactl -m0 {CALADAN_PATH}/apps/synthetic/target/release/synthetic \
                                                    10.0.1.8:10000 \
                                                    --config {CALADAN_PATH}/client.config \
                                                    --mode runtime-client \
                                                    --protocol=resp \
                                                    --redis-string=1000000 \
                                                    --transport=tcp \
                                                    --samples=1 \
                                                    --pps={pps} \
                                                    --threads={conn} \
                                                    --runtime={RUNTIME} \
                                                    --discard_pct=10 \
                                                    --output=trace \
                                                    --rampup=0 \
                                                    {f"--loadshift={LOADSHIFTS}" if LOADSHIFTS != "" else ""} \
                                                    {f"--zipf={ZIPF_ALPHA}" if ZIPF_ALPHA != "" else ""} \
                                                    {f"--onoff={ONOFF}" if ONOFF == "1" else ""} \
                                                    --exptid={DATA_PATH}/{experiment_id} \
                                                    > {DATA_PATH}/{experiment_id}.client']
                                        else: # redis-benchmark
                                            tls_cmd = f'--tls --cert /usr/local/tls/svr.crt --key /usr/local/tls/svr.key --cacert /usr/local/tls/CA.pem'
                                            cmd = [f'sudo numactl -m0 \
                                                {CAPYBARA_HOME}/capybara-redis/src/redis-benchmark \
                                                {f"{tls_cmd}" if TLS == 1 else ""} \
                                                -h 10.0.1.8 -p 10000 \
                                                -t get -n 3000000 -c {conn} --threads {conn} \
                                                > {DATA_PATH}/{experiment_id}.client']
                                    else:
                                        print(f'Invalid server app: {SERVER_APP}')
                                        exit(1)
                                    # cmd = [f'sudo {HOME}/Capybara/tcp_generator/build/tcp-generator \
                                    #         -a 31:00.1 \
                                    #         -n 4 \
                                    #         -c 0xffff -- \
                                    #         -d exponential \
                                    #         -c {HOME}/Capybara/tcp_generator/addr.cfg \
                                    #         -s 256 \
                                    #         -t 10 \
                                    #         -r {i} \
                                    #         -f {j} \
                                    #         -q {4 if j >= 4 else j} \
                                    #         -o {DATA_PATH}/{experiment_id}.latency \
                                    #         > {DATA_PATH}/{experiment_id}.stats 2>&1']
                                    task = host.run(cmd, quiet=False)
                                    pyrem.task.Parallel([task], aggregate=True).start(wait=True)

                                    print('================ TEST COMPLETE =================\n')
                                    if CLIENT_APP == 'wrk':
                                        result_str = parse_wrk_result(experiment_id)
                                        print(result_str + '\n\n')
                                        final_result = final_result + result_str + '\n'
                                    else:
                                        try:
                                            cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]" | tail -1'
                                            result = subprocess.run(
                                                cmd,
                                                shell=True,
                                                stdout=subprocess.PIPE,
                                                stderr=subprocess.STDOUT,
                                                check=True,
                                            ).stdout.decode()
                                            if result == '':
                                                result = '[RESULT] N/A\n'
                                            print('[RESULT]' + f'{experiment_id}, {NUM_BACKENDS}, {conn}, {mig_delay}, {max_reactive_migs}, {max_proactive_migs}, {mig_per_n},{result[len("[RESULT]"):]}' + '\n\n')
                                            final_result = final_result + f'{experiment_id}, {NUM_BACKENDS}, {conn}, {mig_delay}, {max_reactive_migs}, {max_proactive_migs}, {mig_per_n},{result[len("[RESULT]"):]}'
                                        except subprocess.CalledProcessError as e:
                                            # Handle the exception for a failed command execution
                                            print("EXPERIMENT FAILED\n\n")

                                        except Exception as e:
                                            # Handle any other unexpected exceptions
                                            print("EXPERIMENT FAILED\n\n")
                                    
                                    kill_procs()
                                    time.sleep(7)
                                    if TCPDUMP == True:
                                        parse_tcpdump(experiment_id)
                                        # print("Parsing pcap file is done, finishing test here.\n\n")

                                        # exit()

                                    if EVAL_MIG_DELAY == True:
                                        parse_mig_delay(experiment_id)

                                    if EVAL_POLL_INTERVAL == True:
                                        parse_poll_interval(experiment_id)
                                    
                                    if EVAL_LATENCY_TRACE == True:
                                        parse_latency_trace(experiment_id)

                                    if EVAL_SERVER_REPLY == True:
                                        parse_server_reply(experiment_id)

                                    if EVAL_RPS_SIGNAL == True:
                                        parse_rps_signal(experiment_id)
                                    
                                    if EVAL_MIG_CPU_OVHD == True:
                                        parse_mig_cpu_ovhd(experiment_id)
                                    
                                

            # task = host.run(cmd, return_output=True, quiet=False)
            # task.start()
            # result = task.return_values
            # print(f'{result}\n\n\n')
    
    # cmd = f'rsync -zarvhP \
    # node7:{CAPYBARA_PATH}/eval/ \
    # {local_eval_dir}/'
    # subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE).wait()
    # print('LOGS ARE COPIED')

def exiting():
    global final_result
    print('EXITING')
    result_header = "ID, #BE, #CONN, MIG_DELAY, TOTAL_#_MIG, MIG_PER_N, RPS, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th"
    if CLIENT_APP == 'wrk':
        result_header = "ID, #BE, #CONN, #THREAD, AVG, p50, p75, p90, p99"
        
    print(f'\n\n\n\n\n{result_header}')
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write(f'{result_header}')
        file.write(final_result)
    kill_procs()


def run_compile():
    # only for redis-server
    mig = ''
    if 'manual-tcp-migration' in FEATURES:
        mig = '-mig-manual'
    elif 'tcp-migration' in FEATURES:
        mig = '-mig'

    features = '--features=' if len(FEATURES) > 0 else ''
    for feat in FEATURES:
        features += feat + ','

    # if SERVER_APP == 'http-server':
    
    if SERVER_APP == 'redis-server':
        os.system(f"cd {CAPYBARA_PATH} && CARGO_FEATURES={features} make LIBOS={LIBOS} all-libs")
        clean = 'make distclean &&' if len(sys.argv) > 2 and sys.argv[2] == 'clean' else ''
        os.system(f'cd {CAPYBARA_HOME}/capybara-redis && {clean} make -j BUILD_TLS=yes')
        return os.system(f'cd {CAPYBARA_HOME}/capybara-redis-tlse && {clean} make -j BUILD_TLS=no redis-server')
        # capybara-redis-tlse is implemented to use tlse library, and it should be compiled with "BUILD_TLS=no" always 
        
    else :
        return os.system(f"cd {CAPYBARA_PATH} && EXAMPLE_FEATURES={features} make LIBOS={LIBOS} all-examples-rust")
    
    # else:
    #     print(f'Invalid server app: {SERVER_APP}')
    #     exit(1)


if __name__ == '__main__':
    # parse_server_reply("20240228-065133.628398")
    # exit(1)
    # parse_result()
    # parse_mig_delay("20240318-093754.379579")
    
    # kill_procs()
    # exit()

    if len(sys.argv) > 1 and sys.argv[1] == 'build':
        exit(run_compile())
    
    atexit.register(exiting)
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()