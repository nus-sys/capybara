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
REPEAT_NUM = 10
MIG_DELAYS = [30000]
MIG_VARS = [1]#[5000, 10000, 15000, 20000, 25000, 30000, 40000, 50000, 70000]
CLIENT_PPS = [i for i in range(10000, 250000 + 1, 20000)]
NUM_CONNECTIONS = [1, 10, 30, 50, 100, 150, 200]
RUNTIME = 5
TCPDUMP = True