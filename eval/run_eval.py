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


from test_config import *

final_result = ''


def kill_procs():
    cmd = ['sudo pkill -INT -f Capybara && sudo pkill -INT -f caladan']
    if TCPDUMP:
        cmd[0] += ' && sudo pkill -INT -f tcpdump'
    # cmd = [f'sudo pkill -INT -f Capybara && sleep 2 && sudo pkill -f Capybara && sudo pkill -f caladan']
    kill_tasks = []
    for node in ALL_NODES:
        host = pyrem.host.RemoteHost(node)
        task = host.run(cmd, quiet=False)
        # print(task)
        kill_tasks.append(task)
    
    pyrem.task.Parallel(kill_tasks, aggregate=True).start(wait=True)
    print('KILLED CAPYBARA PROCESSES')


def run_server(mig_delay, mig_var, mig_per_n):
    global experiment_id
    
    print('SETUP SWITCH')
    cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_setup.py"'] 
    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    # print(result + '\n\n')
    
    print('RUNNING BACKENDS')
    tasks = []
    host = pyrem.host.RemoteHost(BACKEND_NODE) 
    cmd = [f'cd {CAPYBARA_PATH} && make be-dpdk-ctrl'] 
    task = host.run(cmd, quiet=True)
    pyrem.task.Parallel([task], aggregate=True).start(wait=False)
    time.sleep(3)
    print('be-dpdk-ctrl is running')

    server_tasks = []
    for j in range(NUM_BACKENDS):
        cmd = [f'cd {CAPYBARA_PATH} && \
                sudo -E \
                LIBOS=catnip \
                RECV_QUEUE_LEN=100 \
                MTU=1500 \
                MSS=1500 \
                NUM_CORES=4 \
                RUST_BACKTRACE=full \
                CORE_ID={j+1} \
                MIG_DELAY={int(mig_delay/10) * 76} \
                MIG_VAR={int(mig_var)} \
                MIG_PER_N={int(mig_per_n)} \
                CONFIG_PATH={CAPYBARA_PATH}/config/node9_config.yaml \
                LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
                PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
                numactl -m0 {CAPYBARA_PATH}/bin/examples/rust/{SERVER_APP} 10.0.1.9:1000{j} \
                > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        if SERVER_APP == 'redis-server':
            cmd = [f'cd {CAPYBARA_PATH} && \
                    make run-redis-server \
                    CORE_ID={j+1} \
                    CONF=redis{j} \
                    MIG_VAR={int(mig_var)} \
                    > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        task = host.run(cmd, quiet=False)
        server_tasks.append(task)
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'{NUM_BACKENDS} backends are running')

def run_tcpdump(experiment_id):
    print(f'RUNNING TCPDUMP to node8:{PCAP_PATH}/{experiment_id}.pcap')
    
    host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    cmd = [f'sudo tcpdump -i ens85f1 -w {PCAP_PATH}/{experiment_id}.pcap']
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
  

def parse_mig_latency(experiment_id):
    print(f'PARSING {experiment_id} migration latency') 
    
    # host = pyrem.host.RemoteHost(TCPDUMP_NODE)
    start_printing = False
    
    clusters = {}
    for i in [0, 1]:
        file_path = f'{DATA_PATH}/{experiment_id}.be{i}'
        with open(file_path, "r") as file:
            # Iterate through each line in the file
            for line in file:
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
                    time_str = columns[0]
                    time_components = time_str.split(':')
                    hours, minutes = map(int, time_components[:2])
                    seconds = float(time_components[2])  # Convert sub-second part to a float
                    nanosecond_timestamp = int((hours * 3600 + minutes * 60 + seconds) * 1_000_000_000)
                    columns[0] = str(nanosecond_timestamp)

                    last_column = columns[-1]

                    if last_column in clusters:
                        clusters[last_column].append(columns[0] + ',' + columns[1] + ',' + columns[2])
                    else:
                        clusters[last_column] = [columns[0] + ',' + columns[1] + ',' + columns[2]]
    

    steps = ['',
            'INIT_MIG', 
            'SEND_PREPARE_MIG', 
            'RECV_PREPARE_MIG', 
            'SEND_PREPARE_MIG_ACK', 
            'RECV_PREPARE_MIG_ACK', 
            'SERIALIZE_STATE', 
            'SEND_STATE',
            'RECV_STATE',
            'SEND_STATE_ACK',
            'RECV_STATE_ACK']
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
        for item in sorted_list:
            # print(item)
            columns = item.split(',')
            ns = int(columns[0])
            step = columns[1]
            
            step_idx = steps.index(step)
            if step_idx != prev_step+1:
                print("[PANIC] migration step is wrong!")
                exit()
            prev_step = step_idx

            if step_idx == 1:
                init_ns = ns
            if step_idx >= 2:
                latency = ns - prev_ns
                result = result + str(latency) + ','
            if step_idx == 10:
                result = result + str(ns - init_ns) + "\n"
                prev_step = 0
            prev_ns = ns
        print(result)
        final_result = final_result + '\n'.join(result.split('\n')[100:])
    
    print(len(final_result.split('\n')))
    
    # print('\n'.join(final_result.split('\n')[-10001:]))

    with open(f'{DATA_PATH}/{experiment_id}.mig_latency', 'w') as file:
        # Write the content to the file
        file.write('\n'.join(final_result.split('\n')[-10001:]))

  


def run_eval():
    global experiment_id
    global final_result
    for repeat in range(0, REPEAT_NUM):
        for mig_delay in MIG_DELAYS:
            for mig_var in MIG_VARS: 
                for mig_per_n in MIG_PER_N:
                    for pps in CLIENT_PPS:
                        for conn in NUM_CONNECTIONS:
                            kill_procs()
                            experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                            
                            print(f'================ RUNNING TEST =================\n'
                                    f'RUNTIME: {RUNTIME} / NUM_BACKENDS: {NUM_BACKENDS} / TCPDUMP: {TCPDUMP}\n'
                                    f'SERVER_APP: {SERVER_APP}\n'
                                    f'REPEAT: {repeat}\n'
                                    f'RUN ID: {experiment_id}\n'
                                    f'RATE: {pps}\n'
                                    f'NUM_CONNECTIONS: {conn}\n'
                                    f'MIG_DELAY: {mig_delay}\n'
                                    f'MIG_VAR: {mig_var}\n'
                                    f'MIG_PER_N: {mig_per_n}\n')
                            

                            if TCPDUMP == True:
                                run_tcpdump(experiment_id)
                            
                            run_server(mig_delay, mig_var, mig_per_n)
                            
                            host = pyrem.host.RemoteHost(CLIENT_NODE)
                            cmd = [f'sudo numactl -m0 {HOME}/caladan/apps/synthetic/target/release/synthetic \
                                    10.0.1.1:10000 \
                                    --config {HOME}/caladan/client.config \
                                    --mode runtime-client \
                                    --protocol=http \
                                    --transport=tcp \
                                    --samples=1 \
                                    --pps={pps} \
                                    --threads={conn} \
                                    --runtime={RUNTIME} \
                                    --discard_pct=10 \
                                    --output=buckets \
                                    --rampup=0 \
                                    --exptid={DATA_PATH}/{experiment_id} \
                                    > {DATA_PATH}/{experiment_id}.client']
                            if SERVER_APP == 'redis-server':
                                cmd = [f'sudo numactl -m0 {HOME}/caladan/apps/synthetic/target/release/synthetic \
                                        10.0.1.1:10000 \
                                        --config {HOME}/caladan/client.config \
                                        --mode runtime-client \
                                        --transport=tcp \
                                        --samples=1 \
                                        --pps={pps} \
                                        --threads={conn} \
                                        --runtime={RUNTIME} \
                                        --discard_pct=10 \
                                        --output=buckets \
                                        --rampup=0 \
                                        --exptid={DATA_PATH}/{experiment_id} \
                                        --protocol=resp \
                                        --redis-string=1000000 \
                                        > {DATA_PATH}/{experiment_id}.client']
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
                            
                            try:
                                cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]" | tail -1'
                                result = subprocess.run(
                                    cmd,
                                    shell=True,
                                    stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT,
                                    check=True,
                                ).stdout.decode()
                                print('[RESULT]' + f'{experiment_id}, {conn}, {mig_delay}, {mig_var}, {mig_per_n},{result[len("[RESULT]"):]}' + '\n\n')
                                final_result = final_result + f'{experiment_id}, {conn}, {mig_delay}, {mig_var}, {mig_per_n},{result[len("[RESULT]"):]}'
                            except subprocess.CalledProcessError as e:
                                # Handle the exception for a failed command execution
                                print("EXPERIMENT FAILED\n\n")

                            except Exception as e:
                                # Handle any other unexpected exceptions
                                print("EXPERIMENT FAILED\n\n")
                            
                            if TCPDUMP == True:
                                time.sleep(3)
                                kill_procs()
                                parse_tcpdump(experiment_id)
                                
                                print("Parsing pcap file is done, finishing test here.\n\n")

                                exit()

                            if EVAL_MIG_LATENCY == True:
                                kill_procs()
                                time.sleep(3)
                                parse_mig_latency(experiment_id)

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
    print("\n\n\n\n\nID, #CONN, MIG_DELAY, TOTAL_#_MIG, MIG_PER_N, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc")
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write("ID, #CONN, MIG_DELAY, TOTAL_#_MIG, MIG_PER_N, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc\n")
        file.write(final_result)
    kill_procs()

atexit.register(exiting)


if __name__ == '__main__':
    # parse_result()
    # parse_mig_latency("20231027-100855.950472")
    # exit()
    
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()