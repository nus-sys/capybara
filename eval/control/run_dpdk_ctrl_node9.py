from common import SUDO, check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS, PRELOAD_ENVVARS


check_hostname("nsl-node9")
try:
    run_shell(
        f"{SUDO} ./bin/examples/rust/dpdk-ctrl",
        extra_env=dict(**ENVVARS, **NODE9_ENVVARS, **PRELOAD_ENVVARS),
    )
except KeyboardInterrupt:
    pass
