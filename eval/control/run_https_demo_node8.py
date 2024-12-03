from common import SUDO, check_hostname, run_shell
from env_run import ENVVARS, NODE8_ENVVARS


check_hostname("nsl-node8")
try:
    run_shell(
        f"{SUDO} taskset --cpu-list 0 ./bin/capybara-https 10.0.1.8:10000",
        extra_env=dict(**ENVVARS, **NODE8_ENVVARS, MIG_AFTER="10"),
    )
except KeyboardInterrupt:
    pass
