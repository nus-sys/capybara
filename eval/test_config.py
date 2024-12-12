import os
import pandas as pd
import numpy as np

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
CAPYBARA_HOME = f'{HOME}/Capybara'
CAPYBARA_PATH = f'{HOME}/Capybara/capybara'
CAPYBARA_CONFIG_PATH = f'{CAPYBARA_PATH}/scripts/config'
CALADAN_PATH = f'{HOME}/Capybara/caladan'
DATA_PATH = f'{HOME}/capybara-data'
PCAP_PATH = f'{LOCAL}/capybara-pcap'

################## CLUSTER CONFIG #####################
ALL_NODES = ['node1', 'node7', 'node8', 'node9']
CLIENT_NODE = 'node7'
FRONTEND_NODE = 'node8'
BACKEND_NODE = 'node9'
TCPDUMP_NODE = 'node1'
NODE8_IP = '10.0.1.8'
NODE8_MAC = '08:c0:eb:b6:e8:05'
NODE9_IP = '10.0.1.9'
NODE9_MAC = '08:c0:eb:b6:c5:ad'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    'tcp-migration',
    # 'manual-tcp-migration',
    # 'capy-log',
    'capy-profile',
    # 'capy-time-log',
    # 'server-reply-analysis',
]

################## TEST CONFIG #####################
NUM_BACKENDS = 2
SERVER_APP = 'redis-server' # 'https', 'capybara-switch' 'http-server', 'prism', 'redis-server', 'proxy-server'
TLS = 1
CLIENT_APP = 'redis-bench' # 'wrk', 'caladan', 'redis-bench'
NUM_THREADS = [1] # for wrk load generator
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
TOTAL_TIME = 5000 # Always in multiples of PHASE_INTERVAL
WARMUP_RPS = 100
SERVER_CAPACITY_RPS = 450 # HTTP: 450 REDIS: 200
TOTAL_RPS_MAX = int(SERVER_CAPACITY_RPS * 0.7) * NUM_BACKENDS
STRESS_FACTOR = 1 #1.25, 0.75
MAX_RPS_LIMITS = (400, 600)# HTTP: (400, 600) REDIS: (150, 300)
RPS_LOWER_LIMIT = 10
DURATION_LIMITS = (5, 20)
TRANSITION_LIMITS = (5, 10)
RAND_SEED = 2402271237

################## ENV VARS #####################
### SERVER ###
RECV_QUEUE_LEN_THRESHOLD = 20
MIG_DELAYS = [0]
MAX_PROACTIVE_MIGS = [24] # set element to '' if you don't want to set this env var
MAX_REACTIVE_MIGS = [0] # set element to '' if you don't want to set this env var
MIG_PER_N = [0]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CONFIGURED_STATE_SIZE = 1024 * 0 # bytes
MIN_THRESHOLD = 190 # K rps
RPS_THRESHOLD = 0.3
THRESHOLD_EPSILON = 0.1

CAPY_LOG = 'all' # 'all', 'mig'
REDIS_LOG = 1 # 1, 0


### CALADAN ###
CLIENT_PPS = [0] #[i for i in range(10000, 250000 + 1, 30000)]#[i for i in range(100000, 1_300_001, 100000)]
import workload_spec_generator
LOADSHIFTS = workload_spec_generator.main()
# LOADSHIFTS = '90000:10000,270000:10000,450000:10000,630000:10000,810000:10000/90000:50000/90000:50000/90000:50000'
LOADSHIFTS = ''#'10000:10000/10000:10000/10000:10000/10000:10000'
ZIPF_ALPHA = '' # 0.9, 1.2
ONOFF = '0' # '0', '1'
NUM_CONNECTIONS = [16]
RUNTIME = 10

#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build