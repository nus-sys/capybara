from common import run_shell
from settings import *
from env_build import ENVVARS


run_shell(
    "cargo clippy --no-default-features --features catnip-libos,catnap-libos,mlx5,tcp-migration,capy-log --all-targets",
    cwd=CAPYBARA_DIR,
    extra_env=ENVVARS,
)
