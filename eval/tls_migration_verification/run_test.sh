#!/usr/bin/env bash
#
# Reproduce the mid-record TLS migration test (concern #1).
#
# It clones the exact tlse revision Capybara depends on, drops in splittest.c,
# compiles it with the SAME preprocessor defines Capybara's build uses
# (see tlse build.rs), and runs it. Expected output: "9/9 split points passed".
#
# No DPDK / switch / cluster required: this is a pure library-level test of
# tls_export_context / tls_import_context across a TLS-record boundary.
set -euo pipefail

TLSE_REPO="https://github.com/nus-sys/tlse.git"
TLSE_BRANCH="rust-bindings"
TLSE_COMMIT="b3001a13bc50bd176c476c0407e9a74d56d24e81"   # pinned for reproducible line numbers
WORK="$(mktemp -d)"
HERE="$(cd "$(dirname "$0")" && pwd)"

echo ">> cloning tlse ($TLSE_BRANCH @ ${TLSE_COMMIT:0:9}) into $WORK"
git clone --branch "$TLSE_BRANCH" --single-branch "$TLSE_REPO" "$WORK/tlse" >/dev/null 2>&1
git -C "$WORK/tlse" checkout -q "$TLSE_COMMIT"

cp "$HERE/splittest.c" "$WORK/tlse/splittest.c"

echo ">> compiling (defines identical to Capybara build.rs)"
( cd "$WORK/tlse" && cc -O2 -w -I. \
    -DTLS_AMALGAMATION -DTLS_REEXPORTABLE -DNO_TLS_LEGACY_SUPPORT \
    -DNO_SSL_COMPATIBLE_INTERFACE -DLTC_NO_TABLES=1 \
    splittest.c -o splittest -lm )

echo ">> running"
( cd "$WORK/tlse" && ./splittest )
rc=$?
echo ">> exit code: $rc  (0 = all split points passed)"
exit $rc
