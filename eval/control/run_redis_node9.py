import sys
from os import getcwd

from common import SUDO, WITH_SHIM, check_hostname, run_shell
from env_run import ENVVARS, NODE9_ENVVARS, PRELOAD_ENVVARS
from settings import REDIS_CWD

def main():
    # Check for the -port parameter
    if len(sys.argv) != 2 or not sys.argv[1].startswith("-port="):
        print("Usage: python3 run_redis_node9.py -port=<port>")
        sys.exit(1)
    
    # Extract the port value
    port = sys.argv[1].split("=")[1]
    if not port.isdigit():
        print("Error: Port must be a numeric value.")
        sys.exit(1)
    
    check_hostname("nsl-node9")
    cwd = getcwd()
    try:
        run_shell(
            f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node9_{port}.conf",
            # f"{SUDO} {WITH_SHIM} {cwd}/bin/redis-server {cwd}/config/redis/node9_{port}.conf --loglevel debug",
            extra_env=dict(**ENVVARS, **NODE9_ENVVARS, **PRELOAD_ENVVARS),
            cwd=REDIS_CWD,
        )
    except KeyboardInterrupt:
        pass

if __name__ == "__main__":
    main()