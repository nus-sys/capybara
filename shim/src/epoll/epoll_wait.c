// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include "../epoll.h"
#include "../error.h"
#include "../log.h"
#include "../qman.h"
#include <assert.h>
#include <demi/wait.h>
#include <errno.h>
#include <pthread.h>
#include <string.h>
#include <sys/epoll.h>
#include <time.h>
#include <glue.h>

int __epoll_wait(int epfd, struct epoll_event *events, int maxevents, int timeout)
{
    // Check if epoll descriptor is managed by Demikernel.
    if (epfd < EPOLL_MAX_FDS)
    {
        TRACE("not managed by Demikernel epfd=%d", epfd);
        errno = EBADF;
        return -1;
    }

    int demikernel_epfd = -1;
    if ((demikernel_epfd = queue_man_get_demikernel_epfd(epfd - EPOLL_MAX_FDS)) == -1)
    {
        TRACE("not managed by Demikernel epfd=%d", epfd);
        errno = EBADF;
        return -1;
    }

    TRACE("demikernel_epfd=%d, events=%p, maxevents=%d, timeout=%d", epfd, (void *)events, maxevents, timeout);

    // Check for invalid epoll file descriptor.
    if ((demikernel_epfd < 0) || (demikernel_epfd >= EPOLL_MAX_FDS))
    {
        ERROR("invalid epoll file descriptor epfd=%d", demikernel_epfd);
        errno = EINVAL;
        return -1;
    }
    // fprintf(stderr, "VALID EPFD\n");
    // We intentionally set the timeout to zero, because
    // millisecond timeouts are too coarse-grain for Demikernel.
    // timeout = 0;
    // timeout = 2000; // temporarily set 2s for debugging purpose

    // long long timeout_us = (timeout == -1) ? -1 : (timeout * 1000);
    long long timeout_us = (timeout == -1) ? -1 : 0;

    demi_qresult_t qrs[MAX_EVENTS];
    size_t ready_offsets[MAX_EVENTS];
    size_t qrs_count = MAX_EVENTS;
    demi_qtoken_t qts[MAX_EVENTS];
    struct demi_event *evs[MAX_EVENTS];

    int nevents = epoll_get_ready(demikernel_epfd, qts, evs);

    if (nevents > 0)
    {
        int ret = __demi_wait_any(qrs, ready_offsets, &qrs_count, qts, nevents, timeout_us);
        if (ret != 0)
        {
            ERROR("demi_timedwait() failed - %s", strerror(ret));
            return 0;
        }
        // if (qrs_count > 0){
        //     printf("called wait_any (nevents: %d)\n", nevents);
        //     printf("%ld results returned\n", qrs_count);
        //     for (size_t i = 0; i < qrs_count; i++)
        //         printf("ready_offsets[%zu]: %p\n", i, (void *)&ready_offsets[i]);
            
        //     for (size_t i = 0; i < qrs_count; i++)
        //     {
        //         size_t ready_offset = ready_offsets[i];
        //         printf("ready_offsets[%zu]: %zu\n", i, ready_offset);
        //     }
        // }
        for (size_t i = 0; i < qrs_count; i++)
        {
            size_t ready_offset = ready_offsets[i];

            evs[ready_offset]->qr = qrs[i];
            // printf("completed qd: %d\n", evs[ready_offset]->qr.qr_qd);
            switch (evs[ready_offset]->qr.qr_opcode)
            {
                case DEMI_OPC_ACCEPT:
                {
                    printf("ACCEPT\n");
                    // Fill in event.
                    events[nevents].events = evs[ready_offset]->ev.events;
                    events[nevents].data.fd = evs[ready_offset]->sockqd;
                    evs[ready_offset]->qt = -1;
                    nevents++;

                    // Store I/O queue operation result.
                    queue_man_set_accept_result(evs[ready_offset]->sockqd, evs[ready_offset]);
                }
                break;

                case DEMI_OPC_CONNECT:
                {
                    // TODO: implement.
                    UNIMPLEMETED("parse result of demi_connect()");
                }
                break;

                case DEMI_OPC_POP:
                {
                    printf("POP, ready_offset: %zu, qd: %d\n", ready_offset, evs[ready_offset]->qr.qr_qd);
                    // Fill in event.
                    events[nevents].events = evs[ready_offset]->ev.events;
                    events[nevents].data.fd = evs[ready_offset]->sockqd;
                    events[nevents].data.ptr = evs[ready_offset]->ev.data.ptr;
                    events[nevents].data.u32 = evs[ready_offset]->ev.data.u32;
                    evs[ready_offset]->qt = -1;
                    nevents++;

                    // Store I/O queue operation result.
                    queue_man_set_pop_result(evs[ready_offset]->sockqd, evs[ready_offset]);
                    // printf("nevents: %d\n", nevents);
                }
                break;

                case DEMI_OPC_PUSH:
                {
                    printf("PUSH\n");
                    // TODO: implement.
                    UNIMPLEMETED("parse result of demi_push()");
                }
                break;

                case DEMI_OPC_FAILED:
                {
                    printf("FAIL\n");
                    // Handle timeout: re-issue operation.
                    if (evs[ready_offset]->qr.qr_value.err == ETIMEDOUT)
                    {
                        // Check if read was requested.
                        if (evs[ready_offset]->ev.events & EPOLLIN)
                        {
                            demi_qtoken_t qt = -1;

                            if (queue_man_is_listen_fd(evs[ready_offset]->sockqd))
                            {
                                assert(__demi_accept(&qt, evs[ready_offset]->sockqd) == 0);
                            }
                            else
                            {
                                assert(__demi_pop(&qt, evs[ready_offset]->sockqd) == 0);
                            }
                            evs[ready_offset]->qt = qt;
                        }

                        // Check if write was requested.
                        if (evs[ready_offset]->ev.events & EPOLLOUT)
                        {
                            // TODO: implement.
                            UNIMPLEMETED("add EPOLLOUT event");
                        }
                    }
                    if (evs[ready_offset]->qr.qr_value.err == 199)
                    {
                        evs[ready_offset]->qt = -1;
                    }

                    WARN("operation failed - %s", strerror(evs[ready_offset]->qr.qr_value.err));
                    errno = EINTR;
                    // return -1;
                }
                break;

                default:
                {
                    // TODO: implement.
                    UNIMPLEMETED("signal that Demikernel operation failed");
                }
                break;
            }
        }
    }

    return (nevents);
}
