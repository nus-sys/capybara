from common import SUDO, WITH_SHIM, check_hostname, run_shell
from env_run import ENVVARS, NODE8_ENVVARS, PRELOAD_ENVVARS


check_hostname("nsl-node8")
try:
    run_shell(
        f"{SUDO} {WITH_SHIM} ./bin/server",
        extra_env=dict(**ENVVARS, **NODE8_ENVVARS, **PRELOAD_ENVVARS, MIG_AFTER="8"),
        # extra_env=dict(**ENVVARS, **NODE8_ENVVARS, **PRELOAD_ENVVARS),
    )
    # run_shell("./bin/server")
except KeyboardInterrupt:
    pass
