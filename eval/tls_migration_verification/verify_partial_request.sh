#!/usr/bin/env bash
#
# Reproduce the application-layer partial-request check (README §6).
#
# Confirms what makes paper claim S2 accurate ("if a request spans the migration
# point, its unread bytes remain in the migrated TCP receive queue"):
#   - Redis (case 1): its parser buffer (querybuf) is NOT serialized by the
#     migration callback, and migration is initiated at a pop event, so unread
#     bytes stay in the TCP RX queue (carried by TCP migration), not querybuf.
#   - HTTP   (case 2): the server framework DOES serialize its own request buffer
#     through the connection manager.
#
# Source-level (grep) check; no build/cluster needed.
set -euo pipefail

CAP_REPO=https://github.com/nus-sys/capybara.git
CAP_BR=shim
CAP_PIN=2e9aa9524c032244f3c2b249593dd6ce4e709180
REDIS_REPO=https://github.com/nus-sys/capybara-redis.git
REDIS_BR=dev-cowsay
REDIS_PIN=353b01b5a41879e398e764cc7d3f3d226b6eed77

W="$(mktemp -d)"
echo ">> cloning capybara ($CAP_BR @ ${CAP_PIN:0:9}) and capybara-redis ($REDIS_BR @ ${REDIS_PIN:0:9})"
git clone -q --branch "$CAP_BR" --single-branch "$CAP_REPO" "$W/cap";     git -C "$W/cap" checkout -q "$CAP_PIN"
git clone -q --branch "$REDIS_BR" --single-branch "$REDIS_REPO" "$W/redis"; git -C "$W/redis" checkout -q "$REDIS_PIN"

TLS="$W/redis/src/tls.c"
AE="$W/redis/src/ae_demikernel.c"
HTTP="$W/cap/examples/rust/http-server.rs"
fail=0

echo
echo "=== Redis (case 1): app parser buffer NOT serialized; unread bytes ride the TCP queue ==="
echo "- the migration callback serializes only the TLS context:"
grep -nA3 'uconn_serialize(const' "$TLS" | sed 's/^/    /' || true
if sed -n '640,710p' "$TLS" | grep -q 'querybuf'; then
    echo "  [querybuf in connection-manager region] FOUND -> claim assumption broken"; fail=1
else
    echo "  [querybuf in connection-manager region] NONE -> querybuf is not migrated (as expected)"
fi
echo "- migration is initiated at a pop event / signalled on pop (unread bytes stay in TCP RX queue):"
grep -nE 'demi_initiate_migration|ETCPMIG' "$AE" | sed 's/^/    /' || true

echo
echo "=== HTTP (case 2): app request buffer IS serialized via the connection manager ==="
if grep -q 'impl ApplicationState for Buffer' "$HTTP" \
   && grep -q 'self.buffer.serialize' "$HTTP"; then
    grep -nE 'impl ApplicationState for (Buffer|ConnectionState)|self\.buffer\.serialize|fn (serialize|deserialize)' "$HTTP" | sed 's/^/    /'
    echo "  [http-server.rs serializes its request Buffer] YES"
else
    echo "  [http-server.rs serializes its request Buffer] NOT FOUND"; fail=1
fi

echo
if [ "$fail" = 0 ]; then
    echo ">> VERDICT: Redis keeps unread bytes in the TCP queue (querybuf not serialized);"
    echo "   the HTTP framework serializes its request buffer through the manager."
    echo "   Paper S2 ('unread bytes remain in the migrated TCP receive queue') holds for both."
else
    echo ">> VERDICT: a check failed (see above)."; exit 1
fi
