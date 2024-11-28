from common import SUDO, WITH_SHIM, check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS, PRELOAD_ENVVARS


check_hostname("nsl-node9")
try:
    run_shell(
        f"{SUDO} {WITH_SHIM} ./bin/server 10.0.1.9",
        extra_env=dict(**ENVVARS, **NODE9_ENVVARS, **PRELOAD_ENVVARS),
    )
    # run_shell("./bin/server")
except KeyboardInterrupt:
    pass
