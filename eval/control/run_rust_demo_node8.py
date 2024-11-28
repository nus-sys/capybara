from common import check_hostname, run_shell
from env_run import ENVVARS, NODE8_ENVVARS


check_hostname("nsl-node8")
try:
    run_shell(
        "sudo -A -E LD_LIBRARY_PATH=$LD_LIBRARY_PATH taskset --cpu-list 0 ./bin/examples/rust/tcp-server 10.0.1.8:10000",
        extra_env=dict(**ENVVARS, **NODE8_ENVVARS, MIG_AFTER="3"),
    )
except KeyboardInterrupt:
    pass
