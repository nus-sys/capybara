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
    #'mig-per-n-req',
    #'capy-log',
    #'capy-profile',
    #'capy-time-log'
]

################## TEST CONFIG #####################
NUM_BACKENDS = 2
SERVER_APP = 'http-server.elf'#'redis-server'
REPEAT_NUM = 1
RECV_QUEUE_THRESHOLD = 10
MIG_DELAYS = [0]
MAX_STAT_MIGS = 0 # set to '' if you don't want to set this env var
MIG_VARS = [5]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
MIG_PER_N = [200]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [1_300_000]#[i for i in range(250000, 500001, 50000)]
NUM_CONNECTIONS = [100]
RUNTIME = 5
TCPDUMP = False
EVAL_MIG_LATENCY = False
EVAL_POLL_INTERVAL = False
EVAL_LATENCY_TRACE = False
CAPY_LOG = 'n'