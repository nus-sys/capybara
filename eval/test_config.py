import os
import pandas as pd
import numpy as np
import itertools

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
AUTOKERNEL_PATH = f'{HOME}/Capybara/capybara'
CALADAN_PATH = f'{HOME}/Capybara/caladan'
DATA_PATH = f'{HOME}/autokernel-data'
TCP_GENERATOR_PATH = f'{HOME}/Autokernel/tcp_generator'

################## CLUSTER CONFIG #####################
ALL_NODES = ['node7', 'node9']
CLIENT_NODE = 'node7'
SERVER_NODE = 'node9'
NODE9_IP = '10.0.1.9'
NODE9_MAC = '08:c0:eb:b6:c5:ad'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    'autokernel',
    # 'tcp-migration',
    # 'manual-tcp-migration',
    # 'capy-log',
    # 'capy-profile',
    # 'capy-time-log',
    # 'server-reply-analysis',
]

################## TEST CONFIG #####################
SERVER_APP = 'http-server' # 'tcp-echo', 'http-server' 
CLIENT_APP = 'caladan' # 'tcp_generator', 'caladan', 
REPEAT_NUM = 1


################## VARS #####################
### CLIENT ###
CLIENT_PPS = [i for i in range(520000, 600000 + 1, 30000)]#, 550000, 600000, 650000, 700000]#[i for i in range(300000, 650000 + 1, 100000)]#[i for i in range(100000, 1_300_001, 100000)]
DATA_SIZE = [256] #256, 1024, 8192
NUM_CONNECTIONS = [128] #[1, 32, 64, 128]
RUNTIME = 10

### SERVER ###
# CAPY_LOG = 'all' # 'all', 'mig'
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



#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build
