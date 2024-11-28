from common import CARGO_BUILD, run_shell
from settings import *
from env_build import ENVVARS


def build():
    run_shell(f"{CARGO_BUILD} --lib", cwd=CAPYBARA_DIR, extra_env=ENVVARS)
    run_shell(f"mkdir -p {CAPYBARA_DIR}/lib")
    run_shell(
        f"cp {CAPYBARA_DIR}/target/release/libdemikernel.so {CAPYBARA_DIR}/lib/"
    )
    run_shell(f"cp -r {CAPYBARA_DIR}/scripts/config .")


if __name__ == "__main__":
    build()
    run_shell(f"cp -r {CAPYBARA_DIR}/lib .")
