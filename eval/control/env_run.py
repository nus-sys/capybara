from pathlib import Path
from os import getcwd

from common import run_shell
from settings import *

PREFIX = Path.home()
CWD = getcwd()

completed = run_shell(
    f"find {PREFIX}/lib/ -name '*x86_64-linux-gnu*' -type d", capture_stdout=True
)
LD_LIBRARY_PATH = ":".join([f"{CWD}/lib", *completed.stdout.strip().splitlines()])

ENVVARS = dict(
    LD_LIBRARY_PATH=LD_LIBRARY_PATH,
    CAPY_LOG="all",
    LIBOS="catnip",
    USE_JUMBO="1",
    MTU="9000",
    MSS="9000",
)

PRELOAD_ENVVARS = dict(LD_PRELOAD=f"{CWD}/lib/libshim.so", C_LOG="info")
# PRELOAD_ENVVARS = dict(LD_PRELOAD=f"{CWD}/lib/libshim.so", C_LOG="trace")

NODE8_ENVVARS = dict(CONFIG_PATH=f"{CWD}/config/node8_config.yaml")
NODE9_ENVVARS = dict(CONFIG_PATH=f"{CWD}/config/node9_config.yaml")
