import os

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
CAPYBARA_PATH = f'{HOME}/Capybara/capybara'
DATA_PATH = f'{HOME}/capybara-data'
PCAP_PATH = f'{LOCAL}/capybara-pcap'

################## CLUSTER CONFIG #####################
ALL_NODES = ['node7', 'node8', 'node9']
CLIENT_NODE = 'node7'
BACKEND_NODE = 'node9'
TCPDUMP_NODE = 'node8'




################## TEST CONFIG #####################
NUM_BACKENDS = 2
SERVER_APP = 'http-server.elf'
REPEAT_NUM = 1
MIG_DELAYS = [10]
MIG_VARS = [100]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
MIG_PER_N = [0]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [i for i in range(100000, 100000 + 1, 100000)]
NUM_CONNECTIONS = [100]  
RUNTIME = 5
TCPDUMP = False