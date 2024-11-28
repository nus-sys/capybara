from common import CARGO_BUILD, run_shell
from settings import *
from env_build import ENVVARS


def build():
    run_shell(
        f"{CARGO_BUILD} --example tcp-server", cwd=CAPYBARA_DIR, extra_env=ENVVARS
    )
    run_shell(f"mkdir -p {CAPYBARA_DIR}/bin/examples")
    run_shell(f"cp -rT {CAPYBARA_DIR}/target/release/examples {CAPYBARA_DIR}/bin/examples/rust")


if __name__ == "__main__":
    build()
    run_shell(f"cp -r {CAPYBARA_DIR}/bin .")
    run_shell(f"cp -r {CAPYBARA_DIR}/scripts/config .")
