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
    cmd = ['sudo pkill -INT -e dpdk-ctrl.elf ; \
            sudo pkill -INT -f http-server ; \
            ps aux | grep wrk | grep -v grep | awk \'{print $2}\' | xargs --no-run-if-empty sudo kill -9 ']
    # print(cmd)
    # exit(1)
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


def run_server():
    global experiment_id
    print('SETUP SWITCH')
    cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/port_forward/port_forward.py"']  
    
    cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4/switch_fe/capybara_switch_fe_src_rewriting_by_server_setup.py"'] 
    
    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    # print(result + '\n\n')
    # time.sleep(1)
    
    print('RUNNING BACKENDS')

    dpdk_ctrl_task = []
    for server in SERVER_NODES:
        host = pyrem.host.RemoteHost(server)
        cmd = [f'cd {CAPYBARA_PATH} && make dpdk-ctrl-{server}'] 
        task = host.run(cmd, quiet=True)
        dpdk_ctrl_task.append(task)
    pyrem.task.Parallel(dpdk_ctrl_task, aggregate=True).start(wait=False)    
    print('dpdk-ctrl is running')
    time.sleep(3)
    
        

    server_tasks = []
    for j in range(NUM_BACKENDS):
        host = pyrem.host.RemoteHost(f'node{8 + (j%3)}')
        run_cmd = f'{CAPYBARA_PATH}/bin/examples/rust/http-server.elf 10.0.1.{8 + (j%3)}:1000{int(j/3)}'
        
        
        cmd = [f'cd {CAPYBARA_PATH} && \
            {f"taskset --cpu-list {int(j/3) + 1}" if LIBOS == "catnap" else ""} \
            sudo -E \
            CORE_ID={int(j/3) + 1} \
            CONFIG_PATH={CAPYBARA_CONFIG_PATH}/node{8 + (j%3)}_config.yaml \
            {ENV} \
            numactl -m0 \
            {run_cmd} \
            > {DATA_PATH}/{experiment_id}.{SERVER_NODES[j%3]}_{int(j/3)} 2>&1']
            
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
  

def parse_wrk_result(experiment_id):
    import re
    result_str = ''
    total_connections = 0
    total_requests_per_sec = 0.0
    socket_errors = []

    for client in CLIENT_NODES:
        for i in range(4):  # 4 wrk processes
            port = 10000 + i
            file_path = f'{DATA_PATH}/{experiment_id}.{client}.p{port}'
            
            with open(file_path, "r") as file:
                text = file.read()

            # Extract number of connections
            conn_match = re.search(r'(\d+)\s+threads and\s+(\d+)\s+connections', text)
            if conn_match:
                connections = int(conn_match.group(2))
                total_connections += connections

            # Extract Requests/sec
            rps_match = re.search(r'Requests/sec:\s+([\d.]+)', text)
            if rps_match:
                rps = float(rps_match.group(1))
                total_requests_per_sec += rps

            # Check for socket error line
            err_match = re.search(r'Socket errors:.*', text)
            if err_match:
                socket_errors.append(f"{client} port {port}: {err_match.group()}")

    # Final output
    print(f"Total Connections: {total_connections}")
    print(f"Total Requests/sec: {total_requests_per_sec:.2f}")

    error_found = 'no_error'
    if socket_errors:
        print("\nSocket Errors Detected:")
        for err in socket_errors:
            print(f"  {err}")
        error_found = 'error_happend'
    else:
        print("\nNo socket errors found.")
        
    result_str += f'{experiment_id}, {total_connections}, {total_requests_per_sec:.2f}, {error_found}\n'
    return result_str
        

def load_data(filename):
    df = pd.read_csv(filename, header=None, names=["latency", "count", "_"], usecols=[0, 1])
    df["latency"] = pd.to_numeric(df["latency"], errors="coerce")
    df["count"] = pd.to_numeric(df["count"], errors="coerce")
    return df

def aggregate_data(dfs):
    combined = pd.concat(dfs)
    aggregated = combined.groupby("latency", as_index=False).sum()
    return aggregated

def expand_latencies(aggregated_df):
    expanded = np.repeat(aggregated_df["latency"], aggregated_df["count"])
    return expanded

def analyze_latency_csv(experiment_id):
    # Load data from all client nodes
    dfs = [load_data(f'{DATA_PATH}/{experiment_id}.{client}.latency') for client in CLIENT_NODES]

    # Aggregate
    aggregated = aggregate_data(dfs)

    # Expand according to counts
    all_latencies = expand_latencies(aggregated)

    # Total number of latencies
    total_latencies = len(all_latencies)

    # 99th percentile latency
    p99_latency = np.percentile(all_latencies, 99)

    return total_latencies, p99_latency

def run_eval():
    global experiment_id
    global final_result
    
    for repeat in range(0, REPEAT_NUM):
        # for conn in [256]:#[1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384]:
        # for conn in [1, 2, 4, 8, 16, 24, 48, 96]:# , 192, 384, 768, 1536, 3072, 5000, 6144, 10000, 11000, 12288]:
        for conn in [12]:#, 120, 240, 600, 1200, 2400, 3600, 4800, 6000, 7200, 8400, 9600, 10800, 12000, 13200, 14400, 15600, 16128]:
            # conn = conn * 12
            kill_procs()
            experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
            print(f'================ RUNNING TEST =================')
            print(f'\n\nEXPTID: {experiment_id}')
            run_server()

            arp_task = []
            for client in ALL_NODES:
                host = pyrem.host.RemoteHost(client)
                cmd = [f'sudo arp -f {CAPYBARA_HOME}/arp_table']
                task = host.run(cmd, quiet=True)
                arp_task.append(task)
            pyrem.task.Parallel(arp_task, aggregate=True).start(wait=True)
            
            
            client_task = []
            servers_idx = [8, 9, 10]
            for client_idx in range(3):
                client = CLIENT_NODES[client_idx]
                server_idx = servers_idx[client_idx]
                host = pyrem.host.RemoteHost(client)
                t_val = conn if conn <= 10 else 10
                 
                for i in range(4):  # 4 wrk processes
                    src_port = 1024 + (16128 * i)
                    port = 10000 + i
                    start_core = i * 10
                    end_core = start_core + 9
                    core_range = f'{start_core}-{end_core}'

                    cmd = [f'taskset -c {core_range} sudo numactl -m0 {WRK_PATH}/wrk \
                        -t{t_val} \
                        -c{conn} \
                        -P{src_port}-{src_port+conn-1} \
                        -d20s \
                        -R200000 \
                        --latency \
                        http://round-robin/get \
                        > {DATA_PATH}/{experiment_id}.{client}.p{port}']
                    cmd = [f'taskset -c {core_range} sudo numactl -m0 {WRK_PATH}/wrk \
                        -t{t_val} \
                        -c{conn} \
                        -P{src_port}-{src_port+conn-1} \
                        -d20s \
                        -R200000 \
                        --latency \
                        http://10.0.1.8:55555/get \
                        > {DATA_PATH}/{experiment_id}.{client}.p{port}']
                    
                    
                    # print(cmd)
                    # host.run(cmd, background=True)  # optionally execute it remotely
                    task = host.run(cmd, quiet=False)
                    client_task.append(task)
            
            pyrem.task.Parallel(client_task, aggregate=True).start(wait=True)

            print(f'================ {experiment_id} TEST COMPLETE =================\n')
            res = parse_wrk_result(experiment_id)
            final_result = final_result + f'{res}'



def exiting():
    global final_result
    print('EXITING')
    result_header = "ID, DATA_SIZE, target_rate, tput, 99th"
        
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
    # print(LOADSHIFTS)
    # analyze_latency_csv('202
    
    if len(sys.argv) > 1 and sys.argv[1] == 'build':
        exit(run_compile())
    
    atexit.register(exiting)
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()