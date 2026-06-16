# Verification: mid-record TLS migration is correct

This folder is the evidence package for one camera-ready claim in the Capybara
paper (Application Connection State section):

> Because the serialized `TLSContext` captures the record-layer state, including
> read/write sequence numbers and any partially consumed record, Capybara can
> migrate a TLS connection at any point in the data stream, not only at record
> boundaries.

The goal of this folder: anyone can read it and be 100% sure the claim is true.

Verification has three independent legs, all confirming the same conclusion:

1. **Source-level (static):** the tlse `export`/`import` functions serialize and
   restore *every* piece of record-layer state needed to resume mid-record.
2. **Library-level end-to-end (dynamic):** a reproducible test splits a TLS record
   across the migration point and confirms the target decrypts it correctly,
   including the *next* record after migration and a response the migrated target
   encrypts back to the original client (TX). **Result: 9/9 split points pass.**
3. **Evaluated code == verified code:** both TLS servers used in the evaluation
   (the Rust `https` server and the Redis `dev-cowsay` fork) call the same tlse
   `export`/`import` API that legs 1–2 verify — and the Redis fork's vendored tlse
   is identical to the tested one on every migration-relevant path (struct,
   `message_buffer` logic, and export/import; the only file diffs are in
   handshake-time certificate parsing and whitespace).

Pinned revisions (so line numbers below are exact):
- `capybara`       @ `2e9aa9524c032244f3c2b249593dd6ce4e709180` (branch `shim`, "sigcomm '26")
- `tlse`           @ `b3001a13bc50bd176c476c0407e9a74d56d24e81` (branch `rust-bindings`)
- `capybara-redis` @ `353b01b5a41879e398e764cc7d3f3d226b6eed77` (branch `dev-cowsay`, the Redis TLS fork)

---

## 0. Background: why this needed checking

Capybara migrates a live TCP connection at an arbitrary byte offset. For TLS,
that offset can land **in the middle of a TLS record** (a record can span several
TCP segments). For the target to keep decrypting, the migration must carry:

- the bytes of the **partial record** the origin already pulled off the socket
  but has not finished decoding, and
- the **record-layer crypto state**: the AEAD key, the AEAD IV/nonce base, and
  the **read/write sequence numbers** (the GCM nonce for a record is derived from
  IV + sequence number, so both must continue exactly).

Two failure modes had to be ruled out:
- **Gap A:** the partial-record bytes are *not* serialized (so the target loses them).
- **Gap B:** even if bytes are serialized, `import` does not restore them into the
  fields the decrypt path reads, so resume is wrong.

Both are closed below.

### Scope of this package
At the TLS layer a migration carries two kinds of in-flight ciphertext:
- **(i) not-yet-consumed** bytes still in the **TCP receive/transmit queue** — moved
  by Capybara's TCP state transfer (Migration Protocol section; the protocol
  exports "all pending packets in the TCP RX/TX queues"). This is the paper's core
  TCP migration, exercised throughout the evaluation.
- **(ii) already-consumed** bytes the TLS layer pulled into its record-reassembly
  buffer, plus the record-layer crypto state — moved by tlse `export`/`import`.

This folder proves **(ii)** — the TLS-specific part — at the source and library
level. (i) is the general TCP migration that the rest of the paper establishes.
Together they cover an arbitrary byte offset. The *combined* behavior on the live
DPDK+switch cluster was not re-run here; that full-system integration test is out
of scope for this library-level evidence package.

---

## 1. Source-level evidence (tlse @ b3001a1)

### 1a. Where the partial record lives
`tlse` buffers an incomplete record internally in `TLSContext.message_buffer`:

| What | File:line | Meaning |
|---|---|---|
| field declaration | `tlse.c:1277-1278` | `message_buffer` / `message_buffer_len` — the record-reassembly buffer |
| consume keeps the remainder | `tlse.c:10171-10180` | after processing whole records, the **leftover (incomplete) record is kept** via `memmove`; only freed when nothing is left |

So a partial record genuinely sits in `message_buffer` at the moment of migration.

### 1b. `tls_export_context` serializes the full record-layer state
`tls_export_context` (`tlse.c:10219`). Precondition at `tlse.c:10221`: the context
must be **established** (`connection_status == 0xFF`) and `exportable`. It then writes:

| State | File:line |
|---|---|
| AEAD IV (local & remote) | `tlse.c:10245-10247` (GCM) / `10255-10257` (ChaCha) |
| session keys (`exportable_keys`) | `tlse.c:10273-10274` |
| **local & remote sequence numbers** | `tlse.c:10305-10308` |
| `tls_buffer` (pending outbound TLS data) | `tlse.c:10310-10311` |
| **`message_buffer` (the partial record)** | `tlse.c:10313-10314` |
| `application_buffer` (decrypted, not yet read) | `tlse.c:10316-10317` |

→ **Gap A closed:** the partial record is *explicitly* appended (`10314`). The
"dynamic export size" observed at runtime is exactly this — `message_buffer`
plus `tls_buffer`, `application_buffer`, and key material — not a guess.

### 1c. `tls_import_context` restores them symmetrically
`tls_import_context` (`tlse.c:10337`) reads the same fields back:

| State | File:line |
|---|---|
| IVs restored + crypto context recreated with the keys | `tlse.c:10362-10429` |
| **local & remote sequence numbers** | `tlse.c:10483-10486` |
| `tls_buffer` | `tlse.c:10489-10499` |
| **`message_buffer` (the partial record)** | `tlse.c:10502-10512` |
| `application_buffer` | `tlse.c:10514-10526` |

→ **Gap B closed:** every field is restored into the *same* struct member the
decrypt path uses. On resume, the next `tls_consume_stream` appends the rest of
the record to the restored `message_buffer`, completing it, and decrypts it with
the restored `remote_sequence_number` + IV. The sequence number is correct
because tlse increments it only *after* a record is fully processed, so it still
points at the not-yet-decoded partial record.

### 1d. Honest caveats (do not affect the claim)
- Export requires `connection_status == 0xFF`, i.e. a **fully established**
  connection (`tlse.c:10221`). Mid-*handshake* migration is not exportable. The
  paper claims mid-*stream* (data phase), which is exactly the established case.
  The claim wording "at any point in the **data stream**" reflects this.
- `tls_make_exportable(ctx, 1)` (`tlse.c:10207`) must be set before keys are
  derived, so they get saved into `exportable_keys`. Capybara does this right
  after accept (see 3a).
- Capybara exports with `small_version = 1`, which skips the TLS master secret
  (`tlse.c:10298-10303`). That secret is only needed for renegotiation/resumption,
  not for decrypting the ongoing stream; the live session keys are exported. The
  evaluation uses TLS 1.2.
- Consequently, "at any point in the **data stream**" assumes no in-stream
  renegotiation (TLS 1.2 renegotiation / TLS 1.3 key update) is in progress at the
  migration instant. The evaluation uses TLS 1.2 without renegotiation, so this
  holds; it is a scope note, not a correctness gap.

---

## 2. Library-level end-to-end test (dynamic) — `splittest.c`

`splittest.c` is a self-contained C program (`#include "tlse.c"`). It:
1. Completes a real TLS 1.2 handshake between an in-memory client and server,
   making the server context exportable (exactly as Capybara does).
2. Has the client encrypt a known plaintext into one TLS record.
3. Splits that record at several offsets (1, 3, 5, 6, ¼, ½, end-10, end-1, full).
4. Feeds the **first** part to the origin → a partial record sits in `message_buffer`.
5. `tls_export_context` (migrate out) → `tls_import_context` into a fresh context
   (migrate in) — the migration boundary.
6. Feeds the **second** part to the target and checks `tls_read` returns the
   exact plaintext.
7. **Continuity check:** the client then sends a *second* record after migration;
   the target must also decrypt it (verifies receive-side sequence-number/IV continuity).
8. **TX roundtrip:** the migrated target encrypts a response that the *original
   client* decrypts (verifies the send-side state: `local_sequence_number` + keys/IV
   restored on import).

### Run it
```bash
./run_test.sh
```
No DPDK/switch/cluster needed. It clones the pinned tlse revision, compiles
`splittest.c` with the **same defines as Capybara's build** (`TLS_AMALGAMATION`,
`TLS_REEXPORTABLE`, `NO_TLS_LEGACY_SUPPORT`, `NO_SSL_COMPATIBLE_INTERFACE`,
`LTC_NO_TABLES=1`; see tlse `build.rs`), and runs it.

### Expected output
```
record length = 117 bytes
split at    1 / 117 : PASS
split at    3 / 117 : PASS
split at    5 / 117 : PASS
split at    6 / 117 : PASS
split at   29 / 117 : PASS
split at   58 / 117 : PASS
split at  107 / 117 : PASS
split at  116 / 117 : PASS
split at  117 / 117 : PASS

==== 9/9 split points passed (RX decrypt + post-migration continuity + TX roundtrip) ====
```
Every mid-record split decrypts correctly, the post-migration record decrypts, **and**
the migrated target's encrypted response is decrypted by the original client — i.e.
import re-feeds the partial record into the record state machine, and both receive-
and send-side crypto state resume correctly. This is observed behavior, not inference.

---

## 3. The evaluated code uses this exact API

The verification above is on `tlse` + the example wiring. The evaluation runs two
TLS servers; both use the same `tls_export_context` / `tls_import_context`.

### 3a. Rust `https` server (capybara @ 2e9aa95)
`examples/rust/https.rs` is run for the TLS experiment when `SERVER_APP == 'https'`
(`eval/run_eval.py:231`, `…/bin/examples/rust/https.elf`). Its connection manager:

| Role | File:line | Call |
|---|---|---|
| migrate-in | `examples/rust/https.rs:102` | `tls_import_context(buf, len)` |
| migrate-out | `examples/rust/https.rs:116-121` | hands over the `TLSContext` |
| serialized size | `examples/rust/https.rs:129` | `tls_export_context(ctx, null, 0, 1)` |
| serialize | `examples/rust/https.rs:138` | `tls_export_context(ctx, buf, len, 1)` |
| make exportable after accept | `examples/rust/https.rs:370` | `tls_make_exportable(ctx, 1)` |

This is *the verified code path*, run as-is in the evaluation.

### 3b. Redis (`capybara-redis` @ `dev-cowsay`)
The Redis TLS fork is `capybara-redis` branch `dev-cowsay` — the path the eval
points at (`eval/control/settings.py:2`, `REDIS_DIR = ".../redis/dev-cowsay"`).
It **vendors the same tlse** in `src/tlse/tlse.c` and registers the connection
manager in `src/tls.c`:

| Role | File:line | Call |
|---|---|---|
| make exportable after accept | `src/tls.c:349` | `tls_make_exportable(ctx, 1)` |
| migrate-in | `src/tls.c:664` | `tls_import_context(data, data_len)` |
| migrate-out | `src/tls.c:670` | returns the `TLSContext` |
| serialized size | `src/tls.c:685` | `tls_export_context(ctx, NULL, 0, 1)` |
| serialize | `src/tls.c:692` | `tls_export_context(ctx, buf, buf_len, 1)` |
| FFI registration | `src/tls.c:697-705` | `capybara_user_connection` table |

The paper's appendix (`porting.tex:49,55-68`) documents the same port.

**The vendored `src/tlse/tlse.c` matches the tested tlse on every migration-relevant
path.** A full-file `diff` against the §1–2 copy shows the two are *not* wholly
identical, but the only three differences are outside the migration path:
1. an extra certificate-chain bounds check (handshake-time cert parsing, ~line 7204),
2. a certificate-extension comparison `>=` vs `<=` (handshake-time cert parsing, ~7283), and
3. whitespace (tabs vs spaces) in `_private_tls_crc32` (a STUN helper).

Every region the claim depends on is **byte-identical** (verified with `diff`):
- the `TLSContext` struct layout (so serialized field offsets match),
- the `tls_consume_stream` partial-record/`message_buffer` logic, and
- `tls_export_context` + `tls_import_context` (320 lines each, line-for-line identical).

In this vendored copy the partial record is serialized at `src/tlse/tlse.c:10312`
and restored at `src/tlse/tlse.c:10504-10506` (grep-confirmed). The
decrypted-but-unread `application_buffer` is also carried — verified in the §1
copy at export `tlse.c:10316-10317`, import `tlse.c:10514-10526` (the same code,
per the byte-identical export/import functions above).

→ Both evaluated TLS servers (`https.rs` and Redis `dev-cowsay`) go through the
**byte-identical** tlse export/import verified in §1–2. No residual gaps.
Reproduce this whole check with **`./verify_code_match.sh`** (clones both pinned
repos and prints the diff + verdict).

---

## 4. Files in this folder
- `README.md`             — this document.
- `splittest.c`           — the mid-record split/migration test (self-contained: RX decrypt + continuity + TX roundtrip).
- `run_test.sh`           — leg 2: clones the pinned tlse, builds with Capybara's defines, runs `splittest.c`.
- `verify_code_match.sh`  — leg 3: clones pinned tlse + Redis `dev-cowsay`, proves their tlse is identical on every migration path and that Redis calls the verified API.

Both scripts clone the upstream repos at the pinned commits (§ top), so the folder
needs nothing else; the upstream sources are not vendored here on purpose.

## 5. One-line conclusion
The partial TLS record and all record-layer crypto state are serialized on export
and restored on import (§1), a split-record migration decrypts correctly end to
end (§2, 9/9), and the evaluated servers use this very API (§3). The claim holds.
