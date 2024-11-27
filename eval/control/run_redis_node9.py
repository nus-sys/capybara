from os import getcwd


from common import SUDO, WITH_SHIM, check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS, PRELOAD_ENVVARS
from settings import REDIS_CWD


check_hostname("nsl-node9")
cwd = getcwd()
try:
    run_shell(
        f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node9.conf",
        # f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node9.conf --loglevel debug",
        extra_env=dict(**ENVVARS, **NODE9_ENVVARS, **PRELOAD_ENVVARS),
        cwd=REDIS_CWD,
    )
except KeyboardInterrupt:
    pass
