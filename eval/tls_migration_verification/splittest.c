/* Mid-record TLS migration test for Capybara camera-ready claim.
 * Verifies: when a TLS record is split across the migration point, exporting the
 * server TLSContext (tls_export_context), importing it into a fresh context
 * (tls_import_context), then feeding the remainder reconstructs and decrypts the
 * record correctly -- i.e. import re-feeds the partial record into the record
 * state machine, and sequence-number/IV continuity holds afterward.
 *
 * Built with the SAME defines Capybara uses (see tlse build.rs).
 */
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <signal.h>

#include "tlse.c"

static const char *CERT = "examples/cert/server.crt";
static const char *KEY  = "examples/cert/server.key";

static unsigned char *slurp(const char *path, unsigned int *out_len) {
    FILE *f = fopen(path, "rb");
    if (!f) { fprintf(stderr, "cannot open %s\n", path); exit(2); }
    fseek(f, 0, SEEK_END); long n = ftell(f); fseek(f, 0, SEEK_SET);
    unsigned char *b = malloc(n);
    if (fread(b, 1, n, f) != (size_t)n) { exit(2); }
    fclose(f); *out_len = (unsigned int)n; return b;
}

/* Establish a fresh, fully-handshaked (server,client) pair in memory.
 * server context is made exportable before the handshake (as Capybara does). */
static void establish(struct TLSContext **out_server, struct TLSContext **out_client) {
    unsigned int cl, kl;
    unsigned char *cert = slurp(CERT, &cl);
    unsigned char *key  = slurp(KEY,  &kl);

    struct TLSContext *srv_listen = tls_create_context(1, TLS_V12);
    tls_load_certificates(srv_listen, cert, cl);
    tls_load_private_key(srv_listen, key, kl);
    struct TLSContext *client = tls_create_context(0, TLS_V12);
    tls_client_connect(client);

    struct TLSContext *server = tls_accept(srv_listen);
    tls_make_exportable(server, 1);

    int guard = 0;
    while ((tls_established(server) == 0 || tls_established(client) == 0) && guard++ < 50) {
        unsigned int n = 0;
        const unsigned char *wb = tls_get_write_buffer(client, &n);
        if (n) { tls_consume_stream(server, wb, (int)n, NULL); tls_buffer_clear(client); }
        n = 0;
        wb = tls_get_write_buffer(server, &n);
        if (n) { tls_consume_stream(client, wb, (int)n, NULL); tls_buffer_clear(server); }
    }
    if (tls_established(server) == 0 || tls_established(client) == 0) {
        fprintf(stderr, "handshake did not complete\n"); exit(3);
    }
    tls_buffer_clear(server);
    tls_buffer_clear(client);
    free(cert); free(key);
    *out_server = server; *out_client = client;
}

/* Encrypt `plain` on the client into one TLS application-data record; copy bytes out. */
static unsigned char *make_record(struct TLSContext *client, const char *plain,
                                  unsigned int *rec_len) {
    tls_write(client, (const unsigned char *)plain, (unsigned int)strlen(plain));
    unsigned int n = 0;
    const unsigned char *wb = tls_get_write_buffer(client, &n);
    unsigned char *rec = malloc(n);
    memcpy(rec, wb, n);
    tls_buffer_clear(client);
    *rec_len = n;
    return rec;
}

/* Returns 1 on success (decrypted == plain), 0 on failure. */
static int test_split(unsigned int split, const char *plain) {
    struct TLSContext *server, *client;
    establish(&server, &client);

    unsigned int rec_len;
    unsigned char *rec = make_record(client, plain, &rec_len);
    if (split > rec_len) { free(rec); return 1; } /* skip impossible splits */

    /* Feed FIRST half to the origin server: leaves a partial record in message_buffer. */
    if (split) tls_consume_stream(server, rec, (int)split, NULL);

    /* (No destructive read here: reading on the origin would consume plaintext
     * before export and is not part of the migration path.) */

    /* MIGRATE OUT: export the origin context (small_version=1, as Capybara). */
    unsigned char expbuf[16 << 10];
    int exp = tls_export_context(server, expbuf, sizeof(expbuf), 1);
    if (exp <= 0) { fprintf(stderr, "  [split %u] export failed (%d)\n", split, exp); free(rec); return 0; }

    /* MIGRATE IN: import into a fresh context (the target). */
    struct TLSContext *target = tls_import_context(expbuf, (unsigned int)exp);
    if (!target) { fprintf(stderr, "  [split %u] import returned NULL\n", split); free(rec); return 0; }

    /* Feed the SECOND half to the target. */
    if (split < rec_len) tls_consume_stream(target, rec + split, (int)(rec_len - split), NULL);

    /* Read decrypted plaintext from the target. */
    unsigned char out[8192]; memset(out, 0, sizeof(out));
    int got = tls_read(target, out, sizeof(out));
    int ok = (got == (int)strlen(plain)) && (memcmp(out, plain, got) == 0);
    if (!ok) {
        fprintf(stderr, "  [split %u] MISMATCH: got %d bytes \"%.*s\"\n", split, got, got > 0 ? got : 0, out);
    }

    /* CONTINUITY: after migration, client sends a SECOND record; target must decrypt it
     * (tests remote_sequence_number / IV continuity across the migration). */
    const char *plain2 = "second-record-after-migration-0123456789";
    unsigned int rl2; unsigned char *rec2 = make_record(client, plain2, &rl2);
    tls_consume_stream(target, rec2, (int)rl2, NULL);
    unsigned char out2[8192]; memset(out2, 0, sizeof(out2));
    int got2 = tls_read(target, out2, sizeof(out2));
    int ok2 = (got2 == (int)strlen(plain2)) && (memcmp(out2, plain2, got2) == 0);
    if (!ok2) {
        fprintf(stderr, "  [split %u] CONTINUITY MISMATCH: got %d bytes \"%.*s\"\n", split, got2, got2 > 0 ? got2 : 0, out2);
    }

    /* TX roundtrip: the migrated target encrypts a response that the ORIGINAL client
     * must decrypt (verifies TX state: local_sequence_number + keys/IV restored on import). */
    const char *resp = "HTTP/1.1 200 OK\r\n\r\nmigrated-response-payload-0123456789";
    tls_write(target, (const unsigned char *)resp, (unsigned int)strlen(resp));
    unsigned int rn = 0;
    const unsigned char *rwb = tls_get_write_buffer(target, &rn);
    unsigned char *resprec = malloc(rn);
    memcpy(resprec, rwb, rn);
    tls_buffer_clear(target);
    tls_consume_stream(client, resprec, (int)rn, NULL);
    unsigned char rout[8192]; memset(rout, 0, sizeof(rout));
    int rgot = tls_read(client, rout, sizeof(rout));
    int ok3 = (rgot == (int)strlen(resp)) && (memcmp(rout, resp, rgot) == 0);
    if (!ok3) {
        fprintf(stderr, "  [split %u] TX MISMATCH: got %d bytes \"%.*s\"\n", split, rgot, rgot > 0 ? rgot : 0, rout);
    }

    free(rec); free(rec2); free(resprec);
    tls_destroy_context(server);
    tls_destroy_context(client);
    tls_destroy_context(target);
    return ok && ok2 && ok3;
}

int main(void) {
    signal(SIGPIPE, SIG_IGN);
    const char *plain = "GET /secret HTTP/1.1\r\nHost: capybara\r\nX: the-quick-brown-fox-jumps-over-the-lazy-dog\r\n\r\n";
    unsigned int reclen_probe;
    /* probe a record length to choose meaningful split points */
    struct TLSContext *s, *c; establish(&s, &c);
    unsigned char *probe = make_record(c, plain, &reclen_probe);
    free(probe); tls_destroy_context(s); tls_destroy_context(c);
    printf("record length = %u bytes\n", reclen_probe);

    unsigned int splits[] = { 1, 3, 5, 6, reclen_probe/4, reclen_probe/2,
                              reclen_probe - 10, reclen_probe - 1, reclen_probe };
    int total = 0, pass = 0;
    for (unsigned i = 0; i < sizeof(splits)/sizeof(splits[0]); i++) {
        unsigned int sp = splits[i];
        if (sp > reclen_probe) continue;
        total++;
        int r = test_split(sp, plain);
        pass += r;
        printf("split at %4u / %u : %s\n", sp, reclen_probe, r ? "PASS" : "FAIL");
    }
    printf("\n==== %d/%d split points passed (RX decrypt + post-migration continuity + TX roundtrip) ====\n", pass, total);
    return (pass == total) ? 0 : 1;
}
