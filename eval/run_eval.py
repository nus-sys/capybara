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

result_header = ''
final_result = ''


def kill_procs():
    cmd = [f'sudo pkill -INT -e iokerneld ; \
            sudo pkill -INT -f synthetic ; \
            sudo pkill -INT -e dpdk-ctrl.elf ; \
            sudo pkill -INT -e phttp-bench ; \
            sudo pkill -INT -f tcpdump ; \
            sudo pkill -INT -f http-server ; \
            sudo pkill -INT -f tcp-generator ; \
            sudo pkill -INT -f tcp-echo ; \
            sudo pkill -INT -e {SERVER_APP}']
    
    # print(cmd)
    
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
    print('KILLED AUTOKERNEL PROCESSES')


def parse_latency_trace(experiment_id):
    print(f'PARSING {experiment_id} latency_trace') 
    
    cmd = f"cd {AUTOKERNEL_PATH}/eval\
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



def run_server(test_values, data_size):
    global experiment_id
    print('SETUP SWITCH')
    cmd = [f'ssh sw1 "source /home/singtel/tools/set_sde.bash && \
        /home/singtel/bf-sde-9.4.0/run_bfshell.sh -b /home/singtel/inho/Capybara/capybara/p4//port_forward/port_forward.py"'] 
    result = subprocess.run(
        cmd,
        shell=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=True,
    ).stdout.decode()
    
    
    host = pyrem.host.RemoteHost(SERVER_NODE)
    
    # print(test_values)
    AK_ENV = ''
    if 'autokernel' in FEATURES:
        for param_name, value in test_values.items():
            AK_ENV += f"{param_name}={value} "
    else:
        AK_ENV = 'BASELINE=1 '
    # print(ENV)
    
    print('RUNNING SERVER')
    
    # tasks = []
    # host = pyrem.host.RemoteHost(SERVER_NODE) 
    # cmd = [f'cd {AUTOKERNEL_PATH} && make be-dpdk-ctrl'] 
    # task = host.run(cmd, quiet=True)
    # pyrem.task.Parallel([task], aggregate=True).start(wait=False)
    # time.sleep(3)
    # print('Backend dpdk-ctrl is running')
    
    server_tasks = []
    if SERVER_APP == 'tcp-echo':
        cmd = [f'cd {AUTOKERNEL_PATH} && \
                {ENV} \
                make tcp-echo-server9 \
                > {DATA_PATH}/{experiment_id}.server 2>&1']
    elif SERVER_APP == 'http-server':
        AK_ENV += f' DATA_SIZE={data_size} '
        output_file = f'{DATA_PATH}/{experiment_id}.server'
        cmd = [f'cd {AUTOKERNEL_PATH} && \
                sudo -E \
                {ENV} \
                {AK_ENV} \
                numactl -m0 \
                {AUTOKERNEL_PATH}/bin/examples/rust/http-server.elf {NODE9_IP}:10000 \
                >> {output_file} 2>&1']
        cmd[0] = cmd[0] + f' && echo "cmd: {cmd[0]}" >> {output_file}'
    else:
        print(f'Check SERVER_APP: {SERVER_APP}\n')
        exit(1)
    
    # print(cmd)
    task = host.run(cmd, quiet=False)
    server_tasks.append(task)    
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'Server is running')
    
def run_eval():
    global experiment_id
    global result_header
    global final_result
    
    # Create a dictionary that stores each parameter and its values
    parameters = {
        "TIMER_RESOLUTION": TIMER_RESOLUTION,
        "MAX_RECV_ITERS": MAX_RECV_ITERS,
        # "MAX_OUT_OF_ORDER": MAX_OUT_OF_ORDER, 
        # "RTO_ALPHA": RTO_ALPHA,
        # "RTO_BETA": RTO_BETA,
        # "RTO_GRANULARITY": RTO_GRANULARITY,
        # "RTO_LOWER_BOUND_SEC": RTO_LOWER_BOUND_SEC,
        # "RTO_UPPER_BOUND_SEC": RTO_UPPER_BOUND_SEC,
        # "UNSENT_QUEUE_CUTOFF": UNSENT_QUEUE_CUTOFF,
        # "BETA_CUBIC": BETA_CUBIC,
        # "C": C,
        # "DUP_ACK_THRESHOLD": DUP_ACK_THRESHOLD,
        # "WAKER_PAGE_SIZE": WAKER_PAGE_SIZE,
        # "FIRST_SLOT_SIZE": FIRST_SLOT_SIZE,
        # "WAKER_BIT_LENGTH_SHIFT": WAKER_BIT_LENGTH_SHIFT,
        # "FALLBACK_MSS": FALLBACK_MSS,
        "RECEIVE_BATCH_SIZE": RECEIVE_BATCH_SIZE,   
        "POP_SIZE": POP_SIZE,
    }

    # Define the default values (the first element from each array)
    default_values = {k: v[0] for k, v in parameters.items()}
    
    if CLIENT_APP == 'tcp_generator':
        # Generate the result string with the values
        result_header = "EXPT_ID," + '#conn,frame_size,#queues,Rate,Throughput,Median,p90,p95,p99,p99.9,'
    else:
        result_header = "EXPT_ID, " + '#conn, Data size, Rate, Throughput, Dropped, Never Sent, Median, p90, p99, p99.9, p99.99,'
    
    result_header += ",".join(parameters.keys())
    
    
    
    
    for pps in CLIENT_PPS:
        for conn in NUM_CONNECTIONS:
            for data_size in DATA_SIZE:
                # print(pps, conn, data_size)
                combinations = itertools.product(
                    parameters["TIMER_RESOLUTION"],
                    parameters["MAX_RECV_ITERS"],
                    parameters["RECEIVE_BATCH_SIZE"],
                    parameters["POP_SIZE"],
                )
                is_done = 0
                for combination in combinations:
                    test_values = dict(zip(parameters.keys(), combination))
                    # print(pps, conn, data_size, test_values)
                    # continue
                    if 'autokernel' not in FEATURES and is_done == 1:
                        continue
                    is_done = 1
                # is_done = 0
                
                # for param_name, values in parameters.items():
                #     for value in values:
                        
                #         if 'autokernel' not in FEATURES and is_done == 1:
                #             continue
                #         is_done = 1
                        
                #         # Create a copy of the default values
                #         test_values = default_values.copy()
                #         # Update the current parameter being tested
                #         test_values[param_name] = value
                #         # Run the test with the updated values
                        
                    kill_procs()
                    experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                    with open(f'{AUTOKERNEL_PATH}/eval/test_config.py', 'r') as file:
                        print(f'================ RUNNING TEST =================')
                        print(f'\n\nEXPTID: {experiment_id}')
                        with open(f'{DATA_PATH}/{experiment_id}.test_config', 'w') as output_file:
                            output_file.write(file.read())
                    
                    
                    host = pyrem.host.RemoteHost(CLIENT_NODE)
                    if CLIENT_APP == 'caladan':
                        cmd = [f'cd {CALADAN_PATH} && sudo ./iokerneld ias nicpci 0000:31:00.1']
                        task = host.run(cmd, quiet=True)
                        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                        print('iokerneld is running')
                        
                    run_server(test_values, data_size)
                    
                    
                    if CLIENT_APP == 'tcp_generator':
                        cmd = [f'cd {TCP_GENERATOR_PATH} && \
                                sudo ./build/tcp-generator \
                                -a 31:00.1 \
                                -n 4 \
                                -c 0xffff -- \
                                -d exponential \
                                -r {pps} \
                                -f {conn} \
                                -s {data_size} \
                                -t {RUNTIME} \
                                -q 1 \
                                -c addr.cfg \
                                -o {DATA_PATH}/{experiment_id}.lat \
                                > {DATA_PATH}/{experiment_id}.client 2>&1']
                        task = host.run(cmd, quiet=False)
                        print('Running client\n')
                        pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                        time.sleep(RUNTIME * 2 + 7)
                    elif CLIENT_APP == 'caladan':
                        cmd = [f'sudo numactl -m0 {CALADAN_PATH}/apps/synthetic/target/release/synthetic \
                                10.0.1.9:10000 \
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
                                {f"--partial_uniform={PARTIAL_UNIFORM}" if PARTIAL_UNIFORM != "" else ""} \
                                {f"--zipf={ZIPF_ALPHA}" if ZIPF_ALPHA != "" else ""} \
                                --exptid={DATA_PATH}/{experiment_id} \
                                > {DATA_PATH}/{experiment_id}.client']
                        task = host.run(cmd, quiet=False)
                        print('Running client\n')
                        pyrem.task.Parallel([task], aggregate=True).start(wait=True)
                    
                    
                    print('================ TEST COMPLETE =================\n')
                    
                    try:
                        cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]" | tail -1'
                        client_result = subprocess.run(
                            cmd,
                            shell=True,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            check=True,
                        ).stdout.decode()
                        if client_result == '':
                            client_result = '[RESULT] N/A\n'
                        
                        if CLIENT_APP == 'tcp_generator':
                            # Generate the result string with the values
                            result = f'{experiment_id},' + f'{client_result[len("[RESULT]"):].rstrip()},'
                        else:
                            result = f'{experiment_id}, {conn}, {data_size},' + f'{client_result[len("[RESULT]"):].rstrip()}, '
                        
                        if 'autokernel' in FEATURES:
                            result += ",".join(map(str, test_values.values())) + "\n"
                        else:
                            result += "BASELINE\n"
                        print('\n\n' + "***TEST RESULT***\n" + result_header + '\n' + result)
                        
                        final_result += result
                        
                    except subprocess.CalledProcessError as e:
                        # Handle the exception for a failed command execution
                        print("EXPERIMENT FAILED\n\n")

                    except Exception as e:
                        # Handle any other unexpected exceptions
                        print("EXPERIMENT FAILED\n\n")
                    
                    kill_procs()
                    time.sleep(7)
                    if EVAL_LATENCY_TRACE == True:
                        parse_latency_trace(experiment_id)
                                    
def exiting():
    
    global result_header
    global final_result
    
    print('EXITING')
   
    print(f'\n\n\n\n\n{result_header}\n')
    print(final_result)
    
    experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
    print(f'Writing results to {experiment_id}.result')
    with open(f'{DATA_PATH}/{experiment_id}.result', "w") as file:
        file.write(f'{result_header}\n')
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
    
    
    return os.system(f"cd {AUTOKERNEL_PATH} && EXAMPLE_FEATURES={features} make LIBOS={LIBOS} all-examples-rust")
    
    # else:
    #     print(f'Invalid server app: {SERVER_APP}')
    #     exit(1)


if __name__ == '__main__':

    if len(sys.argv) > 1 and sys.argv[1] == 'build':
        exit(run_compile())
    
    atexit.register(exiting)
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()