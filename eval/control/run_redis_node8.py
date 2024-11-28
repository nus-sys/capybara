from os import getcwd


from common import SUDO, WITH_SHIM, check_hostname, run_shell
from env_run import ENVVARS, NODE8_ENVVARS, PRELOAD_ENVVARS
from settings import REDIS_CWD


check_hostname("nsl-node8")
cwd = getcwd()
try:
    run_shell(
        f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node8.conf",
        # f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node8.conf --loglevel debug",
        # f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/node8tcp.conf --loglevel debug",
        extra_env=dict(**ENVVARS, **NODE8_ENVVARS, **PRELOAD_ENVVARS, MIG_AFTER="12"),
        cwd=REDIS_CWD,
    )
    # run_shell(f"{cwd}/bin/redis-server {cwd}/config/redis/node8.conf", cwd=REDIS_CWD)
    # run_shell(f"{cwd}/bin/redis-server {cwd}/config/redis/node8.conf --loglevel debug", cwd=REDIS_CWD)
    # run_shell(f"gdb {cwd}/bin/redis-server", cwd=REDIS_CWD)
except KeyboardInterrupt:
    pass
