from subprocess import PIPE, run
from os import environ


def run_shell(cmd, cwd=None, extra_env=None, capture_stdout=False):
    if extra_env:
        for name, value in extra_env.items():
            print(f"{name}={value}")
    print(cmd)
    options = dict(shell=True, check=True)
    if cwd:
        options["cwd"] = cwd
    if extra_env:
        options["env"] = dict(environ, **extra_env)
    if capture_stdout:
        options["stdout"] = PIPE
        options["text"] = True
    return run(cmd, **options)


CARGO_BUILD = "cargo build -r --no-default-features --features catnip-libos,catnap-libos,mlx5,tcp-migration,capy-log"


def check_hostname(expected):
    assert run_shell("hostname", capture_stdout=True).stdout.strip() == expected

SUDO = "sudo -A -E LD_LIBRARY_PATH=$LD_LIBRARY_PATH"

WITH_SHIM = "LD_PRELOAD=$LD_PRELOAD"