import os
import pandas as pd
import numpy as np

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
CAPYBARA_PATH = f'{HOME}/Capybara/capybara'
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
    # 'capy-profile',
    'capy-time-log',
    'server-reply-analysis',
]

################## TEST CONFIG #####################
NUM_BACKENDS = 2
SERVER_APP = 'http-server' # 'http-server', 'prism', 'redis-server'
CLIENT_APP = 'caladan' # 'wrk', 'caladan'
NUM_THREADS = [1] # for wrk load generator
REPEAT_NUM = 1

TCPDUMP = False
EVAL_MIG_DELAY = False
EVAL_POLL_INTERVAL = False
EVAL_LATENCY_TRACE = True
EVAL_SERVER_REPLY = True
EVAL_RPS_SIGNAL = True
EVAL_MIG_CPU_OVHD = False

################## WORKLOAD GENERATOR CONFIG #####################
# All time intervals are in ms, RPS are in KRPS.
PHASE_INTERVAL = 100
TOTAL_TIME = 5000 # Always in multiples of PHASE_INTERVAL
WARMUP_RPS = 100
SERVER_CAPACITY_RPS = 200 #450
TOTAL_RPS_MAX = int(SERVER_CAPACITY_RPS * 0.8) * NUM_BACKENDS
STRESS_FACTOR = 1 #1.25 
MAX_RPS_LIMITS = (150, 300)#(400, 650)
#PHASE_TIME_INTERVAL_LIMITS = (800, 1200)
RPS_LOWER_LIMIT = 10
# RPS_LIMITS = (10, 600)
DURATION_LIMITS = (5, 20)
TRANSITION_LIMITS = (5, 10)
RAND_SEED = 2402271237

################## ENV VARS #####################
### SERVER ###
RECV_QUEUE_THRESHOLD = 50
MIG_DELAYS = [0]
MAX_STAT_MIGS = [0]#[5000, 10000, 15000] # set element to '' if you don't want to set this env var
MIG_PER_N = [10]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
SESSION_DATA_SIZE = 1024 * 0 # bytes
MIN_THRESHOLD = 0.2
RPS_THRESHOLD = 0.55
THRESHOLD_EPSILON = 0.05 

CAPY_LOG = 'all' # 'all', 'mig'
REDIS_LOG = 0


### CALADAN ###
CLIENT_PPS = [i for i in range(10, 10 + 1, 50000)]#[i for i in range(100000, 1_300_001, 100000)]
import workload_spec_generator
# LOADSHIFTS = workload_spec_generator.main()
LOADSHIFTS = '90000:30000,270000:20000,450000:20000,630000:20000,810000:30000/90000:120000'
# LOADSHIFTS = ''
ZIPF_ALPHA = '1.2' # 0.9
ONOFF = '0' # '0', '1'
NUM_CONNECTIONS = [100]
RUNTIME = 1

#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean