// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.
#define _GNU_SOURCE
#include <demi/libos.h>
#include <demi/sga.h>
#include <demi/wait.h>

#include <stdio.h>
#include <unistd.h>
#include <assert.h>
#include <sys/types.h>

#include "utils.h"
#include "log.h"

static struct hashset *__demi_reent_guards;

#define MAX_THREADS_LOG2 10

#define DEMI_CALL(type, fn, ...)                        \
    {                                                   \
        pid_t tid = gettid();                           \
        hashset_insert(__demi_reent_guards, tid);       \
        type ret = fn(__VA_ARGS__);                     \
        hashset_remove(__demi_reent_guards, tid);       \
        return (ret);                                   \
    }


int is_reentrant_demi_call()
{
    pid_t tid = gettid();
    return hashset_contains(__demi_reent_guards, tid);
}

void init_reent_guards() {
    __demi_reent_guards = hashset_create(MAX_THREADS_LOG2);
    assert(__demi_reent_guards != NULL);
}

int __demi_init(int argc, char *const argv[])
{
    DEMI_CALL(int, demi_init, argc, argv);
}

int __demi_create_pipe(int *memqd_out, const char *name)
{
    DEMI_CALL(int, demi_create_pipe, memqd_out, name);
}

int __demi_open_pipe(int *memqd_out, const char *name)
{
    DEMI_CALL(int, demi_open_pipe, memqd_out, name);
}

int __demi_socket(int *sockqd_out, int domain, int type, int protocol)
{
    DEMI_CALL(int, demi_socket, sockqd_out, domain, type, protocol);
}

int __demi_listen(int sockqd, int backlog)
{
    DEMI_CALL(int, demi_listen, sockqd, backlog);
}

int __demi_bind(int sockqd, const struct sockaddr *addr, socklen_t size)
{
    DEMI_CALL(int, demi_bind, sockqd, addr, size);
}

int __demi_accept(demi_qtoken_t *qt_out, int sockqd)
{
    DEMI_CALL(int, demi_accept, qt_out, sockqd);
}

int __demi_connect(demi_qtoken_t *qt_out, int sockqd, const struct sockaddr *addr, socklen_t size)
{
    DEMI_CALL(int, demi_connect, qt_out, sockqd, addr, size);
}

int __demi_close(int qd)
{
    DEMI_CALL(int, demi_close, qd);
}

int __demi_push(demi_qtoken_t *qt_out, int qd, const demi_sgarray_t *sga)
{
    printf("glue.c::__demi_push\n");
    DEMI_CALL(int, demi_push, qt_out, qd, sga);
}

int __demi_pushto(demi_qtoken_t *qt_out, int sockqd, const demi_sgarray_t *sga,
                  const struct sockaddr *dest_addr, socklen_t size)
{
    DEMI_CALL(int, demi_pushto, qt_out, sockqd, sga, dest_addr, size);
}

int __demi_pop(demi_qtoken_t *qt_out, int qd)
{
    printf("glue.c::__demi_pop()\n");
    DEMI_CALL(int, demi_pop, qt_out, qd);
}

demi_sgarray_t __demi_sgaalloc(size_t size)
{
    DEMI_CALL(demi_sgarray_t, demi_sgaalloc, size);
}

int __demi_sgafree(demi_sgarray_t *sga)
{
    DEMI_CALL(int, demi_sgafree, sga);
}

int __demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt)
{
    DEMI_CALL(int, demi_wait, qr_out, qt);
}

int __demi_wait_any(demi_qresult_t *qrs_out, size_t *ready_offsets, size_t *qrs_count,
                    demi_qtoken_t qts[], size_t num_qts, long long timeout_us)
{
    DEMI_CALL(int, demi_wait_any, qrs_out, ready_offsets, qrs_count, qts, num_qts, timeout_us);
}

int __demi_getsockopt(int sockfd, int level, int optname,
        void *optval, socklen_t *optlen)
{
    DEMI_CALL(int, demi_getsockopt, sockfd, level, optname, optval, optlen);
}

int __demi_setsockopt(int sockfd, int level, int optname,
        const void *optval, socklen_t optlen)
{
    DEMI_CALL(int, demi_setsockopt, sockfd, level, optname, optval, optlen);
}

int __demi_getpeername(int qd, struct sockaddr *addr, socklen_t *addrlen)
{
    DEMI_CALL(int, demi_getpeername, qd, addr, addrlen);
}
