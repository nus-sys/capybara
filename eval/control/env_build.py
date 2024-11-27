from pathlib import Path

from common import run_shell
from settings import *

PREFIX = Path.home()

completed = run_shell(
    f"find {PREFIX}/lib/ -name '*pkgconfig*' -type d", capture_stdout=True
)
PKG_CONFIG_PATH = ":".join(completed.stdout.strip().splitlines())

ENVVARS = dict(
    PKG_CONFIG_PATH=PKG_CONFIG_PATH,
    RUSTFLAGS="-Awarnings",
    LIBDIR=f"{CAPYBARA_DIR}/lib",
    BINDIR=f"{CAPYBARA_DIR}/bin/shim",
    CFLAGS=f"-I{CAPYBARA_DIR}/include -O3 -Wno-implicit-function-declaration -Wno-format",
)
