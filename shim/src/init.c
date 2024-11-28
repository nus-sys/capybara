// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#define _GNU_SOURCE
#include "epoll.h"
#include "log.h"
#include "qman.h"
#include "utils.h"
#include <demi/libos.h>
#include <dlfcn.h>
#include <errno.h>
#include <glue.h>
#include <stdint.h>

struct user_connection_peer_ffi {
    void (*migrate_in)(int, const uint8_t *, size_t);
    void *(*migrate_out)(int);
    size_t (*serialized_size)(const void *);
    size_t (*serialize)(const void *, uint8_t *, size_t);
};

int __init()
{
    int ret = -1;
    int argc = 1;

    char *const argv[] = {"shim"};

    // TODO: Pass down arguments correctly.
    TRACE("argc=%d argv={%s}", argc, argv[0]);

    queue_man_init();
    epoll_table_init();

    ret = __demi_init(argc, argv);

    if (ret != 0)
    {
        errno = ret;
        return -1;
    }

    struct user_connection_peer_ffi *ffi = dlsym(RTLD_DEFAULT, "capybara_user_connection");
    if (ffi) {
        demi_set_user_connection_peer_ffi(
            ffi->migrate_in, ffi->migrate_out, ffi->serialized_size, ffi->serialize);
    } else {
        TRACE("skip set user connection peer ffi: %s", dlerror());
    }
    TRACE("set user connection done");
    return 0;
}
