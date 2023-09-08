// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#ifndef DEMI_WAIT_H_IS_INCLUDED
#define DEMI_WAIT_H_IS_INCLUDED

#include <demi/types.h>
#include <time.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /**
     * @brief Waits for an asynchronous I/O operation to complete.
     *
     * @param qr_out Store location for the result of the completed I/O operation.
     * @param qt     I/O queue token of the target operation to wait for completion.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_wait(demi_qresult_t *qr_out, demi_qtoken_t qt);

    /**
     * @brief Waits for an asynchronous I/O operation to complete or a timeout to expire.
     *
     * @param qr_out  Store location for the result of the completed I/O operation.
     * @param qt      I/O queue token of the target operation to wait for completion.
     * @param abstime Absolute timeout in seconds and nanoseconds since Epoch.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_timedwait(demi_qresult_t *qr_out, demi_qtoken_t qt, const struct timespec *abstime);

    /**
     * @brief Waits for the first asynchronous I/O operation in a list to complete.
     *
     * @param qr_out       Store location for the result of the completed I/O operation.
     * @param ready_offset Store location for the offset in the list of I/O queue tokens of the completed I/O operation.
     * @param qts          List of I/O queue tokens to wait for completion.
     * @param num_qts      Length of the list of I/O queue tokens to wait for completion.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_wait_any(demi_qresult_t *qr_out, int *ready_offset, const demi_qtoken_t qts[], int num_qts);

    /**
     * @brief Returns all completed asynchronous I/O operation in a list.
     *
     * @param qrs_out           Array to store results of the completed I/O operations.
     * @param ready_offsets     Array to store offsets in the list of I/O queue tokens of the completed I/O operations.
     * @param num_out           Pointer to integer that contains the maximum number of results that can be stored. This is overwritten with the actual number of results.
     * @param qts               List of I/O queue tokens to wait for completion.
     * @param num_qts           Length of the list of I/O queue tokens to wait for completion.
     *
     * @return On successful completion, zero is returned. On failure, a positive error code is returned instead.
     */
    extern int demi_try_wait_any(demi_qresult_t qrs_out[], int ready_offsets[], int *num_out, const demi_qtoken_t qts[], int num_qts);

#ifdef __cplusplus
}
#endif

#endif /* DEMI_WAIT_H_IS_INCLUDED */
