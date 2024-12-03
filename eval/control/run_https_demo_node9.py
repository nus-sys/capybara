from common import SUDO, check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS


check_hostname("nsl-node9")
try:
    run_shell(
        f"{SUDO} taskset --cpu-list 0 ./bin/examples/rust/https 10.0.1.9:10000",
        extra_env=dict(**ENVVARS, **NODE9_ENVVARS),
    )
except KeyboardInterrupt:
    pass
