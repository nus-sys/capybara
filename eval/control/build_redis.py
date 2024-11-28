import build_dpdk_ctrl
import build_shim
from common import run_shell
from settings import *


def build():
    build_shim.build()
    build_dpdk_ctrl.build()
    run_shell("make -j BUILD_TLS=no redis-server", cwd=REDIS_DIR)
    run_shell("make -j BUILD_TLS=yes redis-cli", cwd=REDIS_CLI_DIR)


if __name__ == "__main__":
    build()
    run_shell(f"cp -r {CAPYBARA_DIR}/lib .")
    run_shell(f"cp -r {CAPYBARA_DIR}/bin .")
    run_shell(f"cp {REDIS_DIR}/src/redis-server bin/")
    run_shell(f"cp {REDIS_CLI_DIR}/src/redis-cli bin/")
    run_shell("mkdir -p config")
    run_shell(f"cp -rT {REDIS_DIR}/config config/redis")
