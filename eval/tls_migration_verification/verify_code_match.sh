#!/usr/bin/env bash
#
# Reproduce "evaluated code == verified code" (README §1c / §3b).
#
# Confirms the tlse that the Redis TLS fork (capybara-redis @ dev-cowsay) vendors
# is identical, on every migration-relevant code path, to the tlse that
# splittest.c tests (tlse @ rust-bindings) -- and that the Redis fork actually
# calls the verified export/import API.
set -euo pipefail

TLSE_REPO=https://github.com/nus-sys/tlse.git
TLSE_BRANCH=rust-bindings
TLSE_PIN=b3001a13bc50bd176c476c0407e9a74d56d24e81

REDIS_REPO=https://github.com/nus-sys/capybara-redis.git
REDIS_BRANCH=dev-cowsay
REDIS_PIN=353b01b5a41879e398e764cc7d3f3d226b6eed77

W="$(mktemp -d)"
echo ">> cloning tlse ($TLSE_BRANCH @ ${TLSE_PIN:0:9})"
git clone -q --branch "$TLSE_BRANCH" --single-branch "$TLSE_REPO" "$W/tlse"
git -C "$W/tlse" checkout -q "$TLSE_PIN"
echo ">> cloning capybara-redis ($REDIS_BRANCH @ ${REDIS_PIN:0:9})"
git clone -q --branch "$REDIS_BRANCH" --single-branch "$REDIS_REPO" "$W/redis"
git -C "$W/redis" checkout -q "$REDIS_PIN"

TESTED="$W/tlse/tlse.c"            # the copy splittest.c is compiled against
VENDORED="$W/redis/src/tlse/tlse.c" # the copy the evaluated Redis links
TLSC="$W/redis/src/tls.c"
# match the ENCLOSING function/struct *definition* (avoid matching "struct TLSContext *context"
# that appears as a parameter type in many unrelated function signatures)
CRIT='tls_export_context\(|tls_import_context\(|tls_consume_stream\(|struct TLSContext \{'

HUNKS="$(diff -up "$VENDORED" "$TESTED" | grep -E '^@@' || true)"

echo
echo "=== 1) Full diff of the two tlse.c, annotated by enclosing C function ==="
if [ -n "$HUNKS" ]; then echo "$HUNKS" | sed 's/^/  /'; else echo "  (the two files are byte-identical)"; fi

echo
echo "=== 2) Do any differences touch a migration-relevant symbol? ==="
if printf '%s\n' "$HUNKS" | grep -E "$CRIT"; then
    echo "  FAIL: a migration-relevant region differs (above)"; exit 1
fi
echo "  PASS: no diff hunk lies in export/import/consume_stream/struct TLSContext"
echo "        (=> those regions are byte-identical between the two copies)"

echo
echo "=== 3) Positive byte-identical check of the two most critical regions ==="
ex_struct(){ awk '/^struct TLSContext \{/{f=1} f{print} f&&/^};/{exit}' "$1"; }
ex_eximp(){ awk '/^int tls_export_context/{f=1} f{print} /^struct TLSContext \*tls_import_context/{g=1} g&&/^}/{c++; if(c==1)exit}' "$1"; }
[ "$(ex_struct "$VENDORED")" = "$(ex_struct "$TESTED")" ] && echo "  IDENTICAL: struct TLSContext" || { echo "  DIFFER: struct"; exit 1; }
[ "$(ex_eximp  "$VENDORED")" = "$(ex_eximp  "$TESTED")" ] && echo "  IDENTICAL: tls_export_context + tls_import_context" || { echo "  DIFFER: export/import"; exit 1; }

echo
echo "=== 4) The evaluated Redis fork calls the verified API (src/tls.c) ==="
grep -nE 'tls_export_context|tls_import_context|tls_make_exportable' "$TLSC" | sed 's/^/  /'

echo
echo ">> VERDICT: the tlse linked by the evaluated Redis server is identical to the"
echo "   tested tlse on every migration path (struct, consume_stream, export/import),"
echo "   and Redis calls export/import exactly as verified. Evaluated code == verified code."
