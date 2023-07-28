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


# PATH
CAPYBARA_PATH = '/homes/inho/Capybara/capybara'
DATA_PATH = '/homes/inho/capybara-data'
local_eval_dir = '/home/ihchoi/Dropbox/Research/Capybara/capybara/eval'

# NODE
ALL_NODES = ['node7', 'node8', 'node9']
BACKEND_NODE = 'node9'
CLIENT_NODE = 'node7'

config = toml.load('./config.toml')
REPEAT_NUM = 10
RATE_FROM = 110000
RATE_TO = 251000 #2000000
RATE_INTERVAL = 20000
NUM_BACKENDS = 2
BACKEND_APP = 'http-server-be'
NUM_CONNECTIONS = [1, 10, 30, 50, 100, 150, 200]
MIG_DELAYS = [0, 5, 10, 15, 20, 30, 40, 50, 100]
MIG_PER_RXS = [0, 1, 2, 3, 5, 10, 15, 20, 30, 50, 100, 200, 300, 500]
final_result = ''

DEFAULT_WIDTH = 6.4
C = [
    'xkcd:grass green',
    'xkcd:blue',
    'xkcd:purple',
    'xkcd:orange',
    'xkcd:teal',
    'xkcd:brick red',
    'xkcd:black',
    'xkcd:brown',
    'xkcd:grey',
]
LS = [
    'solid',
    'dashed',
    'dotted',
    'dashdot'
]

M = [
    'o',
    's',
    '^',
    'v',
    'D'
]

def millions(x, pos):
    'The two args are the value and tick position'
    return str(int(x/1000000)) + '.' + str(int((x%1000000)/100000)) + 'M'
def thousands(x, pos):
    'The two args are the value and tick position'
    return str(int(x/1000)) + 'K'

def plot_setup():
    '''Called before every plot_ function'''

    def lcm(a, b):
        return abs(a*b) // math.gcd(a, b)

    def a(c1, c2):
        '''Add cyclers with lcm.'''
        l = lcm(len(c1), len(c2))
        c1 = c1 * (l//len(c1))
        c2 = c2 * (l//len(c2))
        return c1 + c2

    def add(*cyclers):
        s = None
        for c in cyclers:
            if s is None:
                s = c
            else:
                s = a(s, c)
        return s
    plt.rc('axes', prop_cycle=(add(cycler(color=C),
                                   cycler(linestyle=LS),
                                   cycler(marker=M))))
    plt.rc('lines', markersize=5)
    plt.rc('legend', handlelength=3, handleheight=1.5, labelspacing=0.25)
    plt.rcParams['font.family'] = 'sans'
    plt.rcParams['font.size'] = 10
    plt.rcParams['pdf.fonttype'] = 42
    plt.rcParams['ps.fonttype'] = 42

def parse_result():
    print('PARSING RESULT')
    plot_setup()
    xs, tails_99th, avgs = dict(), dict(), []
    for j in range(1, 2):
        xs[j] = []
        tails_99th[j] = []
    print('#BE', '1', '2', '3', '4')
    for i in rates:
        print(i, end=' ', flush=True)
        for j in range(1, 2):
            xs[j].append(i*j)
            latency_list = []
            with open(f'{local_eval_dir}/data/r{i}_be{j}.log', 'r') as f:
                for line in f:
                    columns = line.strip().split('\t')
                    latency_list.append(int(columns[1]))
                    # latency_list = np.array(f.read().splitlines()).astype(int)
            
            # print(np.percentile(latency_list, 99))
            tails_99th[j].append(np.percentile(latency_list, 99)/1000)
            # avgs.append(np.average(latency_list))
            print(int(tails_99th[j][len(tails_99th[j])-1]), end=' ', flush=True)
        print()

    fig, ax = plt.subplots(figsize=(6, 4))
    # ax.set_title(f'{config['parameters']['connections']} connections')
    ax.set_xlabel(f'Rate (pps)')  # Add an x-label to the axes.
    ax.set_ylabel('99th-latency(µs)')
    ax.set_xticks(np.arange(0, 3000000 + 1, 500000))
    ax.xaxis.set_major_formatter(tick.FuncFormatter(millions))
    
    for j in range(1, n_backends+1):
        ax.plot(xs[j], tails_99th[j])
        
    # print(max(max(x) for x in tails_99th.values()))
    # ax.plot(xs, avgs)
    plt.legend(range(1, n_backends+1))
    # plt.ylim([0, sum(max(x) for x in tails_99th.values()) / len(tails_99th.keys())])
    plt.ylim([0, 300])
    plt.xlim([0, 3000000])
    
    ax.grid()
    fig.savefig(f'{local_eval_dir}/dpdk.pdf', bbox_inches='tight')
    plt.show()
    


def cleaning():
    cmd = f'rm {local_eval_dir}/data/*.log & rm {local_eval_dir}/data/*.log.tput'
    subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE).wait()

    cmd = ['rm {CAPYBARA_PATH}/eval/data/*.log & rm {CAPYBARA_PATH}/eval/data/*.log.tput']
    host = pyrem.host.RemoteHost(list(config['clients'].keys())[0])
    task = host.run(cmd, quiet=False)
    pyrem.task.Parallel([task], aggregate=True).start(wait=True)

    print('CLEANED OLD OUTPUTS')

def kill_procs():
    cmd = [f'sudo pkill -INT -f Capybara && sudo pkill -INT -f caladan']
    # cmd = [f'sudo pkill -INT -f Capybara && sleep 2 && sudo pkill -f Capybara && sudo pkill -f caladan']
    kill_tasks = []
    for node in ALL_NODES:
        host = pyrem.host.RemoteHost(node)
        task = host.run(cmd, quiet=False)
        # print(task)
        kill_tasks.append(task)
    
    pyrem.task.Parallel(kill_tasks, aggregate=True).start(wait=True)
    print('KILLED CAPYBARA PROCESSES')


def run_server(mig_delay, mig_per_rx):
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
        # cmd = [f'cd {CAPYBARA_PATH} && make {BACKEND_APP}{j} > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        cmd = [f'cd {CAPYBARA_PATH} && \
               sudo -E \
               LIBOS=catnip \
               MTU=1500 \
               MSS=1500 \
               NUM_CORE=4 \
               RUST_BACKTRACE=full \
               CORE_ID={j+1} \
               MIG_DELAY={int(mig_delay/10) * 72} \
               MIG_PER_RX={int(mig_per_rx)} \
               CONFIG_PATH=/homes/inho/Capybara/config/be0_config.yaml \
               LD_LIBRARY_PATH=/homes/inho/lib:/homes/inho/lib/x86_64-linux-gnu \
               PKG_CONFIG_PATH=/homes/inho/lib/x86_64-linux-gnu/pkgconfig \
               {CAPYBARA_PATH}/bin/examples/rust/http-server.elf 10.0.1.9:1000{j} \
               > {DATA_PATH}/{experiment_id}.be{j} 2>&1']
        task = host.run(cmd, quiet=False)
        server_tasks.append(task)
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'{NUM_BACKENDS} backends are running')

def run_eval():
    global experiment_id
    global final_result
    for repeat in range(0, REPEAT_NUM):
        for mig_delay in MIG_DELAYS:
            for mig_per_rx in MIG_PER_RXS: 
                for i in range(RATE_FROM, RATE_TO, RATE_INTERVAL):
                    for j in NUM_CONNECTIONS:
                        kill_procs()
                        experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                        print(f'******* REPEAT: {repeat}  RUN ID: {experiment_id}   RATE: {i}   NUM_CONNECTIONS: {j}    MIG_DELAY: {mig_delay} MIG_PER_RX: {mig_per_rx} *******')
                        run_server(mig_delay, mig_per_rx)
                        host = pyrem.host.RemoteHost(CLIENT_NODE)
                        cmd = [f'sudo /homes/inho/caladan/apps/synthetic/target/release/synthetic \
                                10.0.1.8:10000 \
                                --config /homes/inho/caladan/client.config \
                                --mode runtime-client \
                                --protocol=http \
                                --transport=tcp \
                                --samples=1 \
                                --pps={i} \
                                --threads={j} \
                                --runtime=10 \
                                --exptid={DATA_PATH}/{experiment_id} \
                                > {DATA_PATH}/{experiment_id}.client']
                        # cmd = [f'sudo /homes/inho/Capybara/tcp_generator/build/tcp-generator \
                        #         -a 31:00.1 \
                        #         -n 4 \
                        #         -c 0xffff -- \
                        #         -d exponential \
                        #         -c /homes/inho/Capybara/tcp_generator/addr.cfg \
                        #         -s 256 \
                        #         -t 10 \
                        #         -r {i} \
                        #         -f {j} \
                        #         -q {4 if j >= 4 else j} \
                        #         -o {DATA_PATH}/{experiment_id}.latency \
                        #         > {DATA_PATH}/{experiment_id}.stats 2>&1']
                        task = host.run(cmd, quiet=False)
                        pyrem.task.Parallel([task], aggregate=True).start(wait=True)

                        print('******* FINISH *******')
                        
                        cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]"'
                        result = subprocess.run(
                            cmd,
                            shell=True,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            check=True,
                        ).stdout.decode()
                        print(result + '\n\n')
                        final_result = final_result + f'{experiment_id}, {j}, {mig_delay}, {mig_per_rx},{result[len("[RESULT]"):]}'
                
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
    print("\n\n\n\n\nID, #CONN, MIG_DELAY, MIG_PER_RX, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc")
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write("ID, #CONN, MIG_DELAY, MIG_PER_RX, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc\n")
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