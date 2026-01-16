import os
import pandas as pd
import numpy as np
import pyrem.host
from shared_config import *

################## PATHS #####################
DATA_PATH = f'{HOME}/capybara-sigcomm26'

################## CLUSTER CONFIG #####################
ALL_NODES = ['node5', 'node6', 'node7', 'node8', 'node9', 'node10']
LONGFLOW_CLIENT_NODES = ['node5', 'node6']  # Long-lived connections clients
SHORTFLOW_CLIENT_NODE = 'node7'  # Short-lived connections client
SERVER_NODES = ['node8', 'node9', 'node10']
LS_SERVER_NODES = ['node8', 'node9', 'node10']
FRONTEND_NODE = 'node8'
BACKEND_NODE = 'node9'
TCPDUMP_NODE = 'node10'
NODE8_IP = '10.0.1.8'
NODE8_MAC = '08:c0:eb:b6:e8:05'
NODE9_IP = '10.0.1.9'
NODE9_MAC = '08:c0:eb:b6:c5:ad'
FE_IP = '10.0.1.8'
FE_PORT = '55555' # 55555, 10000

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    # 'tcp-migration',
    'server-rewriting',
    # 'manual-tcp-migration',
    # 'capy-log',
    # 'capy-profile',
    # 'capy-time-log',
    # 'server-reply-analysis',
]

################## TEST CONFIG #####################
NUM_BACKENDS = 12 #12
SERVER_APP = 'http-server' # 'capy-proxy', 'https', 'capybara-switch' 'http-server', 'prism', 'redis-server', 'proxy-server'
TLS = 0
CLIENT_APP = 'caladan' # 'wrk', 'caladan', 'redis-bench'
# NUM_THREADS = [1] # for wrk load generator
REPEAT_NUM = 1

TCPDUMP = False
EVAL_MIG_DELAY = False
EVAL_POLL_INTERVAL = False
EVAL_LATENCY_TRACE = False
EVAL_SERVER_REPLY = False
EVAL_RPS_SIGNAL = False
EVAL_MIG_CPU_OVHD = False
EVAL_MAINTENANCE = False

################## WORKLOAD GENERATOR CONFIG #####################
# All time intervals are in ms, RPS are in KRPS.
PHASE_INTERVAL = 100
TOTAL_TIME = 100 # Always in multiples of PHASE_INTERVAL
SERVER_CAPACITY_RPS = 500 # HTTP: 450 REDIS: 200
WARMUP_RPS = int(SERVER_CAPACITY_RPS * 0.1)
TOTAL_RPS_MAX = int(SERVER_CAPACITY_RPS * 0.5) * NUM_BACKENDS
STRESS_FACTOR = 1 #1.25, 0.75
MAX_RPS_LIMITS = (400, 400)# HTTP: (400, 600) REDIS: (150, 300)
RPS_LOWER_LIMIT = 10
DURATION_LIMITS = (5, 20)
TRANSITION_LIMITS = (5, 10)
RAND_SEED = 2402271237

################## ENV VARS #####################
### SERVER ###
RECV_QUEUE_LEN_THRESHOLD = 10000
MIG_DELAYS = [0]
MAX_PROACTIVE_MIGS = [0] # set element to '' if you don't want to set this env var
MAX_REACTIVE_MIGS = [100000] # set element to '' if you don't want to set this env var
MIG_PER_N = [0]# 1000000, 100000, 10000
CONFIGURED_STATE_SIZE = 1024 * 0 # bytes
MIN_THRESHOLD = 1000000 # K rps
RPS_THRESHOLD = 0.3
THRESHOLD_EPSILON = 0.1
DATA_SIZE = 256 #0(index.html), 256, 1024, 8192

CAPY_LOG = 'all' # 'all', 'mig'
REDIS_LOG = 1 # 1, 0

ENV = f'MTU=9000 MSS=9000 \
        NUM_CORES=4 \
        RUST_BACKTRACE=full \
        USE_JUMBO=1 \
        CAPY_LOG={CAPY_LOG} \
        LIBOS={LIBOS} \
        DATA_SIZE={DATA_SIZE} \
        LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu'
# MTU=8964 MSS=8964 for proxy-server
        




### CALADAN ###
CLIENT_PPS = [i for i in range(3500000, 3600000 + 1, 100000)]#[i for i in range(100000, 1_300_001, 100000)]
import workload_spec_generator
# LOADSHIFTS = workload_spec_generator.main()
# LOADSHIFTS = workload_spec_generator.zipf_for_12_servers(1000000)
# LOADSHIFTS = '90000:10000,270000:10000,450000:10000,630000:10000,810000:10000/90000:50000/90000:50000/90000:50000'
LOADSHIFTS = ''#'10000:10000/10000:10000/10000:10000/10000:10000'
ZIPF_ALPHA = '' # 0, 0.9, 1.2
ONOFF = '' # '0', '1'
NUM_CONNECTIONS = [500] #[i for i in range(100, 100 + 1, 5)]
RUNTIME = 10


#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build