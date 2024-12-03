from common import run_shell
from settings import *
from env_build import ENVVARS


def build():
    run_shell(f"cargo build -r --features capy-log", cwd=CAPYBARA_HTTPS_DIR, extra_env=ENVVARS)


if __name__ == "__main__":
    build()
    run_shell(f"mkdir -p bin")
    run_shell(f"cp -r {CAPYBARA_HTTPS_DIR}/target/release/capybara-https bin/")
    run_shell(f"cp -r {CAPYBARA_DIR}/scripts/config .")
