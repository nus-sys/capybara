> These scripts need to be put in the network-shared directory. They should work from any exact location.

Categories:
* `build_*.py` Partially duplicating `Makefile`s but with minimal chain of reasoning. Should be executed from the building machine.
* `run_*.py` Thin wrapper of commands to be run. Should be executed from the running machines.
* `settings.py` User specific settings.
* `*_c_demo*` and `*_rust_demo*` Scripts for testing programs. Can be ignored.

### Steps

Make two copies of `capybara-redis`, one for building `redis-server` and the other for building client-side programs. In the one for building `redis-server`, checkout `dev-cowsay` branch.

Modify `settings.py` with desired values. `REDIS_DIR` is the copy of `capybara-redis` for building `redis-server` and `REDIS_CLI_DIR` is the other one. `REDIS_CWD` is the working directory for running `redis-server`. The `appendonlydir` will be created there.

On building machine, run `python3 build_redis.py`. This will build `libdemikernal.so`, `libshim.so`, `redis-server` and `redis-cli` each from respective directory. It will then copy compiled artifacts and necessary configurations to this directory.

On building machine, run `python3 build_dpdk_ctrl.py`. This will build `dpdk-ctrl` and copy to this directory.

On source server (node8) and target server (node9), run `python3 run_dpdk_ctrl_node8.py` and `python3 run_dpdk_ctrl_node9.py`. Then run `python3 run_redis_node8.py` and `python3 run_redis_node9.py`.

On client server (node7), run `python3 run_redis_client.py`. The connection will be migrated after a few commands.

Run `python3 clean.py` to clean up all files that were copied to this directory.