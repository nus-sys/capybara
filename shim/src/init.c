// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "epoll.h"
#include "log.h"
#include "qman.h"
#include "utils.h"
#include <demi/libos.h>
#include <errno.h>
#include <glue.h>

int __init()
{
    int ret = -1;
    int argc = 1;

    char *const argv[] = {"shim"};

    // TODO: Pass down arguments correctly.
    TRACE("argc=%d argv={%s}", argc, argv[0]);

    queue_man_init();
    epoll_table_init();

    TRACE("__demi_init start");
    ret = __demi_init(argc, argv);
    TRACE("__demi_init done %d", ret);

    if (ret != 0)
    {
        errno = ret;
        return -1;
    }

    TRACE("set user connection");
    demi_set_user_connection_peer_ffi(NULL, NULL, NULL, NULL);
    TRACE("set user connection done");
    return 0;
}
