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
    # 'autokernel',
    # 'tcp-migration',
    # 'manual-tcp-migration',
    # 'capy-log',
    # 'capy-profile',
    # 'capy-time-log',
    # 'server-reply-analysis',
]

################## TEST CONFIG #####################
SERVER_APP = 'tcp-echo' # 'tcp-echo', 'http-server' 
CLIENT_APP = 'tcp_generator' # 'caladan', 'tcp_generator'
REPEAT_NUM = 1


################## VARS #####################
### CLIENT ###
CLIENT_PPS = [i for i in range(50000, 650000 + 1, 100000)]#[i for i in range(100000, 1_300_001, 100000)]
NUM_CONNECTIONS = [16, 64, 128]
RUNTIME = 5

### SERVER ###
# CAPY_LOG = 'all' # 'all', 'mig'
RECEIVE_BATCH_SIZE = [4, 4 * 8, 4 * 32, 4 * 64] # Default: 4
TIMER_RESOLUTION = [64, 64 * 64, 64 * 1024] # 64
DEFAULT_BODY_POOL_SIZE = [8192 - 1, 65536 - 1] # 8192 - 1
DEFAULT_CACHE_SIZE = [250, 250*2, 250*4] # 250





#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build
