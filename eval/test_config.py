import os

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
CAPYBARA_PATH = f'{HOME}/Capybara/capybara'
CALADAN_PATH = f'{HOME}/caladan'
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
    'mig-per-n-req',
    #'capy-log',
    #'capy-profile',
    #'capy-time-log'
]

################## TEST CONFIG #####################
NUM_BACKENDS = 2
SERVER_APP = 'http-server.elf'#'redis-server'
REPEAT_NUM = 1
MIG_DELAYS = [0]
MIG_VARS = [0]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
MIG_PER_N = [1000]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [100000]#[i for i in range(300000, 300000 + 1, 20000)]
NUM_CONNECTIONS = [100]
RUNTIME = 5
TCPDUMP = False
EVAL_MIG_LATENCY = False
EVAL_POLL_INTERVAL = False
CAPY_LOG = 'none'