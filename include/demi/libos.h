// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_LIBOS_H_IS_INCLUDED
#define DEMI_LIBOS_H_IS_INCLUDED

#include <demi/types.h>
#include <stddef.h>
#include <sys/socket.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Initializes Demikernel.
     *
     * @param argc Number of arguments.
     * @param argv Argument values.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_init(int argc, char *const argv[]);

    /**
     * @brief Creates a socket I/O queue.
     *
     * @param sockqd_out Store location for the socket I/O queue descriptor.
     * @param domain     Communication domain for the new socket.
     * @param type       Type of the socket.
     * @param protocol   Communication protocol for the new socket.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_socket(int *sockqd_out, int domain, int type, int protocol);

    /**
     * @brief Sets as passive a socket I/O queue.
     *
     * @param sockqd  I/O queue descriptor of the target socket.
     * @param backlog Maximum length for the queue of pending connections.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_listen(int sockqd, int backlog);

    /**
     * @brief Binds an address to a socket I/O queue.
     *
     * @param sockqd I/O queue descriptor of the target socket.
     * @param addr   Bind address.
     * @param size   Effective size of the socked address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_bind(int sockqd, const struct sockaddr *addr, socklen_t size);

    /**
     * @brief Asynchronously accepts a connection request on a socket I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param sockqd I/O queue descriptor of the target socket.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_accept(demi_qtoken_t *qt_out, int sockqd);

    /**
     * @brief Asynchronously initiates a connection on a socket I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param sockqd I/O queue descriptor of the target socket.
     * @param addr   Address of remote host.
     * @param size   Effective size of the socked address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_connect(demi_qtoken_t *qt_out, int sockqd, const struct sockaddr *addr, socklen_t size);

    /**
     * @brief Closes an I/O queue descriptor.
     *
     * @param qd Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_close(int qd);

    /**
     * @brief Asynchronously pushes a scatter-gather array to an I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param qd     Target I/O queue descriptor.
     * @param sga    Scatter-gather array to push.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_push(demi_qtoken_t *qt_out, int qd, const demi_sgarray_t *sga);

    /**
     * @brief Asynchronously pushes a scatter-gather array to a socket I/O queue.
     *
     * @param qt_out    Store location for I/O queue token.
     * @param sockqd    I/O queue descriptor of the target socket.
     * @param sga       Scatter-gather array to push.
     * @param dest_addr Address of destination host.
     * @param size      Effective size of the socked address data structure.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_pushto(demi_qtoken_t *qt_out, int sockqd, const demi_sgarray_t *sga,
                           const struct sockaddr *dest_addr, socklen_t size);

    /**
     * @brief Asynchronously pops a scatter-gather array from an I/O queue.
     *
     * @param qt_out Store location for I/O queue token.
     * @param qd     Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_pop(demi_qtoken_t *qt_out, int qd);

#ifdef __DEMIKERNEL_TCPMIG__

#define ETCPMIG 199

    /**
     * @brief Signals that the TCP connection represented by `qd` can be migrated right now.
     *
     * @param was_migration_done Store location for whether migration was initiated (1 if yes, 0 if no).
     * @param qd                 Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_notify_migration_safety(int *was_migration_done, int qd);

    /**
     * @brief Initiates migration of the TCP connection represented by `qd`.
     *
     * @param qd    Target I/O queue descriptor.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_initiate_migration(int qd);

    /**
     * @brief Prints log of receive queue lengths.
     */
    extern void demi_print_queue_length_log(void);

#endif /* __DEMIKERNEL_TCPMIG__ */

#ifdef __cplusplus
}
#endif

#endif /* DEMI_LIBOS_H_IS_INCLUDED */
