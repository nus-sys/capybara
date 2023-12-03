import os

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
BACKEND_NODE = 'node9'
TCPDUMP_NODE = 'node8'

################## BUILD CONFIG #####################
FEATURES = [
    'tcp-migration',
    # 'manual-tcp-migration',
    # 'capy-log',
    #'capy-profile',
    # 'capy-time-log'
]

################## TEST CONFIG #####################
NUM_BACKENDS = 4
SERVER_APP = 'http-server'
# SERVER_APP = 'redis-server'
# CLIENT_APP = 'wrk'
CLIENT_APP = 'caladan'
REPEAT_NUM = 1
RECV_QUEUE_THRESHOLD = 0
MIG_DELAYS = [0] 
MAX_STAT_MIGS = [0]#[5000, 10000, 15000] # set element to '' if you don't want to set this env var
MIG_PER_N = [10]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [200000]#[i for i in range(1000000, 1000000 + 1, 70000)]#[i for i in range(100000, 1_300_001, 100000)]
NUM_CONNECTIONS = [i for i in range(1, 10 + 1, 3)]
NUM_THREADS = [i for i in range(1, 20 + 1, 3)]
RUNTIME = 10
TCPDUMP = False
EVAL_MIG_LATENCY = False
EVAL_POLL_INTERVAL = False
EVAL_LATENCY_TRACE = False
CAPY_LOG = 'n'

#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean