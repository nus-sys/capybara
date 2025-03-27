import os
import pandas as pd
import numpy as np
import itertools

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
AUTOKERNEL_PATH = f'{HOME}/Capybara/autokernel'
CALADAN_PATH = f'{HOME}/Capybara/caladan'
DATA_PATH = f'{HOME}/autokernel-data'
TCP_GENERATOR_PATH = f'{HOME}/Autokernel/tcp_generator'

CONFIG_PATH = f'{AUTOKERNEL_PATH}/scripts/config/node9_config.yaml'
LD_LIBRARY_PATH = f'{AUTOKERNEL_PATH}/lib:{HOME}/lib/x86_64-linux-gnu'
    
################## CLUSTER CONFIG #####################
ALL_NODES = ['node7', 'node9']
CLIENT_NODE = 'node7'
SERVER_NODE = 'node9'
NODE9_IP = '10.0.1.9'

# FOR CLOUDLAB TEST
ALL_NODES = ['mghgm@amd137.utah.cloudlab.us', 'mghgm@amd145.utah.cloudlab.us']
CLIENT_NODE = 'mghgm@amd137.utah.cloudlab.us'
SERVER_NODE = 'mghgm@amd145.utah.cloudlab.us'
SERVER_IP = '10.0.1.2'
CLIENT_NIC_PCI = '41:00.0'
CONFIG_PATH = f'{AUTOKERNEL_PATH}/scripts/config/cl_config.yaml'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    'autokernel',
    # 'profiler',
    # 'capy-log',
    # 'capy-profile',
    # 'capy-time-log',
    
    # 'tcp-migration',
    # 'manual-tcp-migration',
    # 'server-reply-analysis',
]
ENV = f'MTU=9000 MSS=9000  RUST_BACKTRACE=full USE_JUMBO=1 CAPY_LOG=all LIBOS={LIBOS} \
        CONFIG_PATH={CONFIG_PATH} \
        LD_LIBRARY_PATH={LD_LIBRARY_PATH} \
        DUP_ACK_THRESHOLD=1 OOO_RATE=0.1 DROP_RATE=0.0 TUNE_GRANULARITY=16'
################## TEST CONFIG #####################
SERVER_APP = 'http-server' # 'tcp-echo', 'http-server' 
CLIENT_APP = 'caladan' # 'tcp_generator', 'caladan', 
REPEAT_NUM = 1

EVAL_LATENCY_TRACE = False

################## VARS #####################
### CLIENT ###
CLIENT_PPS = [i for i in range(100, 100+1, 100000)]#, 550000, 600000, 650000, 700000]#[i for i in range(300000, 650000 + 1, 100000)]#[i for i in range(100000, 1_300_001, 100000)]
LOADSHIFTS = '' #'90000:10000,270000:10000,450000:10000,630000:10000,810000:10000/90000:50000/90000:50000/90000:50000'
PARTIAL_UNIFORM = ''#'5:500000,10:200000,20:200000,40:200000,60:200000,80:200000,100:200000,120:200000,140:200000,160:200000,180:200000,200:200000'
# partial uniform: choose a partial number of connections and uniformly distribute the workload only to those connections (other connections send a minimal traffic)
ZIPF_ALPHA = '' # 0.9, 1.2
DATA_SIZE = [0] #0(index.html), 256, 1024, 8192
NUM_CONNECTIONS = [1] #[1, 32, 64, 128, 512, 1024]
RUNTIME = 5

### DEMIKERNEL PARAMETERS ###
TIMER_RESOLUTION = [64] # Default: 64
MAX_RECV_ITERS = [2] # 2
MAX_OUT_OF_ORDER = [2048, 256, 2048 * 4] # 2048
RTO_ALPHA = [0.125, 0.125 * 4, 0.125 * 16] # 0.125
RTO_BETA = [0.25, 0.25 * 4, 0.25 * 16] # 0.25
RTO_GRANULARITY = [0.001, 0.001 * 16, 0.001 * 128, 0.001 * 512] # 0.001f64
RTO_LOWER_BOUND_SEC = [0.1, 2, 1024] # 0.100f64
RTO_UPPER_BOUND_SEC = [60, 1, 2048] # 60.0f64
UNSENT_QUEUE_CUTOFF = [1024, 128, 1024 * 4] # 1024
BETA_CUBIC = [0.7, 0.7 * 4, 0.7 * 16] # 0.7
C = [0.4, 0.4 * 4, 0.4 * 16] # 0.4
DUP_ACK_THRESHOLD = [3, 1, 8] # 3
WAKER_PAGE_SIZE = [64, 16, 256] # 64
FIRST_SLOT_SIZE = [16, 4, 64] # 16
WAKER_BIT_LENGTH_SHIFT = [6, 4, 8]
FALLBACK_MSS = [536, 128, 2048] # 536
RECEIVE_BATCH_SIZE = [4] # 4
POP_SIZE = [58] # 58*2, 58*4, 58*8, 58*16, 58*32,



#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build
