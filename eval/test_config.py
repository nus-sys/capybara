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
ALL_NODES = ['node7', 'node8', 'node9']
CLIENT_NODE = 'node7'
FRONTEND_NODE = 'node8'
BACKEND_NODE = 'node9'
TCPDUMP_NODE = 'node8'
NODE8_IP = '10.0.1.8'
NODE8_MAC = '08:c0:eb:b6:e8:05'
NODE9_IP = '10.0.1.9'
NODE9_MAC = '08:c0:eb:b6:c5:ad'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    # 'tcp-migration',
    # 'manual-tcp-migration',
    # 'capy-log',
    # 'capy-profile',
    # 'capy-time-log'
]

################## TEST CONFIG #####################
NUM_BACKENDS = 1
SERVER_APP = 'http-server' # 'http-server', 'prism', 'redis-server'
CLIENT_APP = 'caladan' # 'wrk', 'caladan'
NUM_THREADS = [1] # for wrk load generator
REPEAT_NUM = 1
RECV_QUEUE_THRESHOLD = 20
MIG_DELAYS = [0] 
MAX_STAT_MIGS = [100000]#[5000, 10000, 15000] # set element to '' if you don't want to set this env var
MIG_PER_N = [1]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [i for i in range(500000, 500000 + 1, 100)]#[i for i in range(100000, 1_300_001, 100000)]
LOADSHIFTS = '' # 800000:2000000,1100000:1,700000:2000000
ZIPF_ALPHA = '' # 0.9
ONOFF = '0' # '0', '1'
NUM_CONNECTIONS = [1]
RUNTIME = 10
TCPDUMP = False
EVAL_MIG_LATENCY = False
EVAL_POLL_INTERVAL = False
EVAL_LATENCY_TRACE = False
EVAL_RECV_QLEN = False
EVAL_REQS_VS_TIME = True
CAPY_LOG = 'n'

#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean