> These scripts need to be put in the network-shared directory. They should work from any exact location.

Listing:
* `build_*.py` Partially duplicating `Makefile`s but with minimal chain of reasoning. Should be executed from the building machine.
* `run_*.py` Thin wrapper of commands to be run. Should be executed from the running machines.
* `settings.py` User specific settings.
* `*_c_demo*` and `*_rust_demo*` Scripts for testing programs. Can be ignored.

### Steps for running migration-enabled TLS Redis

Make two copies of `capybara-redis`, one for building `redis-server` and the other for building client-side programs. In the one for building `redis-server`, checkout `dev-cowsay` branch.

Modify `settings.py` with desired values. `REDIS_DIR` is the copy of `capybara-redis` for building `redis-server` and `REDIS_CLI_DIR` is the other one. `REDIS_CWD` is the working directory for running `redis-server`. The `appendonlydir` will be created there.

On building machine, run `python3 build_redis.py`. This will build `libdemikernal.so`, `libshim.so`, `dpdk-ctrl`, `redis-server` and `redis-cli` each from respective directory. It will then copy compiled artifacts and necessary configurations to this directory.

On source server (node8) and target server (node9), run `python3 run_dpdk_ctrl_node8.py` and `python3 run_dpdk_ctrl_node9.py`. Then run `python3 run_redis_node8.py` and `python3 run_redis_node9.py`.

On client server (node7), run `python3 run_redis_client.py`. The connection will be migrated after a few commands.

Run `python3 clean.py` to clean up all files that were copied to this directory.

### Steps for running migration-enabled HTTPS

On building machine, run `python3 build_https_demo.py`. This will build `https` and copy built binary to this directory.

On source server (node8), run `python3 run_https_demo_node8.py`.

On target server (node9), run `python3 run_https_demo_node9.py`.

On client server (node7), run `python3 run_demo_client.py tls`. The HTTP response is received multiple times, first few before migration and last few after.

### Steps for running TLS migration micro-benchmark

On building machine, run `python3 build_https_demo.py`. This will build `https` and copy built binary to this directory.

On source server (node8), run `python3 run_tls_migrate_node8.py`.

On target server (node9), run `python3 run_https_demo_node9.py`.

On client server (node7), run `python3 run_tls_migrate_client.py`. The client will establish one TLS connection to source server, print the connection and exit. The ping-pong migration is initiated right after the establishment.
