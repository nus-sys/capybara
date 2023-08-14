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


def run_server(mig_delay, mig_var):
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
               MTU=1500 \
               MSS=1500 \
               NUM_CORES=4 \
               RUST_BACKTRACE=full \
               CORE_ID={j+1} \
               MIG_DELAY={int(mig_delay/10) * 72} \
               MIG_VAR={int(mig_var)} \
               CONFIG_PATH={CAPYBARA_PATH}/config/node9_config.yaml \
               LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
               PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
               {CAPYBARA_PATH}/bin/examples/rust/{SERVER_APP} 10.0.1.9:1000{j} \
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
  


def run_eval():
    global experiment_id
    global final_result
    for repeat in range(0, REPEAT_NUM):
        for mig_delay in MIG_DELAYS:
            for mig_var in MIG_VARS: 
                for pps in CLIENT_PPS:
                    for conn in NUM_CONNECTIONS:
                        kill_procs()
                        experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                        
                        print(f'================ RUNNING TEST =================\n'
                                f'REPEAT: {repeat}\n'
                                f'RUN ID: {experiment_id}\n'
                                f'RATE: {pps}\n'
                                f'NUM_CONNECTIONS: {conn}\n'
                                f'MIG_DELAY: {mig_delay}\n'
                                f'MIG_VAR: {mig_var}\n')
                        

                        if TCPDUMP == True:
                            run_tcpdump(experiment_id)
                        
                        run_server(mig_delay, mig_var)
                        
                        host = pyrem.host.RemoteHost(CLIENT_NODE)
                        cmd = [f'sudo {HOME}/caladan/apps/synthetic/target/release/synthetic \
                                10.0.1.8:10000 \
                                --config {HOME}/caladan/client.config \
                                --mode runtime-client \
                                --protocol=http \
                                --transport=tcp \
                                --samples=1 \
                                --pps={pps} \
                                --threads={conn} \
                                --runtime={RUNTIME} \
                                --discard_pct=0 \
                                --exptid={DATA_PATH}/{experiment_id} \
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
                            cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]"'
                            result = subprocess.run(
                                cmd,
                                shell=True,
                                stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT,
                                check=True,
                            ).stdout.decode()
                            print(result + '\n\n')
                            final_result = final_result + f'{experiment_id}, {conn}, {mig_delay}, {mig_var},{result[len("[RESULT]"):]}'
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
    print("\n\n\n\n\nID, #CONN, MIG_DELAY, TOTAL_MIG, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc")
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write("ID, #CONN, MIG_DELAY, TOTAL_MIG, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc\n")
        file.write(final_result)
    kill_procs()

atexit.register(exiting)


if __name__ == '__main__':
    # parse_result()
    # exit()
    
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()