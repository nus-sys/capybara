import build_demikernel
from common import run_shell
from settings import *
from env_build import ENVVARS


def build():
    build_demikernel.build()
    run_shell(f"make -C shim all-shim", cwd=CAPYBARA_DIR, extra_env=ENVVARS)


if __name__ == "__main__":
    build()
    run_shell(f"cp -r {CAPYBARA_DIR}/lib .")
