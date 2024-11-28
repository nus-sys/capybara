from common import check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS


check_hostname("nsl-node9")
try:
    run_shell(
        "sudo -A -E LD_LIBRARY_PATH=$LD_LIBRARY_PATH taskset --cpu-list 0 ./bin/examples/rust/tcp-server 10.0.1.9:10000",
        extra_env=dict(**ENVVARS, **NODE9_ENVVARS),
    )
except KeyboardInterrupt:
    pass
