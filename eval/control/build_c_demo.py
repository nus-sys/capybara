import build_shim
from common import run_shell
from settings import *


def build():
    build_shim.build()
    run_shell("make", cwd=EPOLL_DIR)


if __name__ == "__main__":
    build()
    run_shell(f"cp -r {CAPYBARA_DIR}/lib .")
    run_shell("mkdir -p bin")
    run_shell(f"cp {EPOLL_DIR}/server bin/")
