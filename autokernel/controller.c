#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <sys/mman.h>
#include <sys/eventfd.h>
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <errno.h>
#include <stdarg.h>
#include <signal.h>

#ifdef PROFILE_INFERENCE
#include <time.h>
static uint64_t total_latency_ns = 0;
static uint64_t inference_count = 0;

#define PROFILE_INFER_START struct timespec _t0, _t1; clock_gettime(CLOCK_MONOTONIC, &_t0);
#define PROFILE_INFER_END do { \
    clock_gettime(CLOCK_MONOTONIC, &_t1); \
    total_latency_ns += (_t1.tv_sec - _t0.tv_sec) * 1000000000ULL + (_t1.tv_nsec - _t0.tv_nsec); \
    inference_count++; \
} while (0)

#define PRINT_PROFILE_SUMMARY() \
    if (inference_count > 0) { \
        fprintf(stderr, "run_inference: called %lu times, avg latency = %.2f µs\n", \
            inference_count, total_latency_ns / (double)inference_count / 1000.0); \
    }
#else
#define PROFILE_INFER_START
#define PROFILE_INFER_END
#define PRINT_PROFILE_SUMMARY()
#endif

//======================================================================
// Logging System
//======================================================================

static int debug_enabled = -1;

void init_debug() {
    const char *env = getenv("DEBUG");
    debug_enabled = (env && strcmp(env, "1") == 0) ? 1 : 0;
}

void debug_log(const char *fmt, ...) {
    if (debug_enabled != 1) return;

    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

#define DEBUG_LOG(...) debug_log(__VA_ARGS__)

//======================================================================
// Shared Memory and EventFD Definitions
//======================================================================

#define SHM_NAME "/autokernel_feedback_shm"
#define SOCKET_PATH "/tmp/eventfd_socket"

typedef struct {
    size_t timer_resolution;
    size_t max_recv_iters;
    size_t max_out_of_order;
    double rto_alpha;
    double rto_beta;
    double rto_granularity;
    double rto_lower_bound_sec;
    double rto_upper_bound_sec;
    size_t unsent_queue_cutoff;
    float beta_cubic;
    float cubic_c;
    uint32_t dup_ack_threshold;
    size_t waker_page_size;
    size_t first_slot_size;
    size_t waker_bit_length_shift;
    size_t fallback_mss;
    size_t receive_batch_size;
    size_t pop_size;

    size_t num_rx_pkts;
    double bytes_acked_per_sec;
} CombinedFeedback;

#define SHM_SIZE sizeof(CombinedFeedback)

int sockfd = -1, connfd = -1, shm_fd = -1, efd = -1;
CombinedFeedback *shm_ptr = NULL;

void cleanup() {
    printf("Controller: Cleaning up...\n");
    if (efd >= 0) close(efd);
    if (connfd >= 0) close(connfd);
    if (sockfd >= 0) close(sockfd);
    if (shm_fd >= 0) close(shm_fd);
    if (shm_ptr) munmap(shm_ptr, SHM_SIZE);
    shm_unlink(SHM_NAME);
    unlink(SOCKET_PATH);
}

void handle_signal(int sig) {
    printf("Controller: Caught signal %d, exiting...\n", sig);
    PRINT_PROFILE_SUMMARY();
    cleanup();
    exit(0);
}

int send_fd(int socket, int fd) {
    struct msghdr msg = {0};
    struct iovec io;
    char buf[1] = {0};
    struct cmsghdr *cmsg;
    char cmsg_buf[CMSG_SPACE(sizeof(fd))];

    io.iov_base = buf;
    io.iov_len = sizeof(buf);
    msg.msg_iov = &io;
    msg.msg_iovlen = 1;

    msg.msg_control = cmsg_buf;
    msg.msg_controllen = CMSG_SPACE(sizeof(fd));

    cmsg = CMSG_FIRSTHDR(&msg);
    cmsg->cmsg_level = SOL_SOCKET;
    cmsg->cmsg_type = SCM_RIGHTS;
    cmsg->cmsg_len = CMSG_LEN(sizeof(fd));

    memcpy(CMSG_DATA(cmsg), &fd, sizeof(fd));
    return sendmsg(socket, &msg, 0);
}

void run_inference(CombinedFeedback* shm_ptr) {
    size_t current_rx = shm_ptr->num_rx_pkts;
    size_t new_batch_size;

    if (current_rx <= 3) {
        new_batch_size = 4;
    } else if (current_rx <= 15) {
        new_batch_size = 16;
    } else if (current_rx == 63) {
        new_batch_size = 64;
    } else {
        new_batch_size = 128;
    }
    if (shm_ptr->receive_batch_size != new_batch_size) {
        DEBUG_LOG("📈 Inference adjusted receive_batch_size from %zu to %zu (num_rx_pkts = %zu)\n",
                  shm_ptr->receive_batch_size, new_batch_size, current_rx);
        shm_ptr->receive_batch_size = new_batch_size;
    }
}

void initialize_parameters_from_env(CombinedFeedback* shm_ptr) {
    // Helper macro to parse env var or use default
    #define GET_ENV_SIZE_T(name, def) ({ \
        const char *val = getenv(name); \
        val ? strtoull(val, NULL, 10) : (def); \
    })
    #define GET_ENV_DOUBLE(name, def) ({ \
        const char *val = getenv(name); \
        val ? strtod(val, NULL) : (def); \
    })
    #define GET_ENV_FLOAT(name, def) ((float) GET_ENV_DOUBLE(name, def))
    #define GET_ENV_U32(name, def) ((uint32_t) GET_ENV_SIZE_T(name, def))

    shm_ptr->timer_resolution     = GET_ENV_SIZE_T("TIMER_RESOLUTION", 64);
    shm_ptr->max_recv_iters       = GET_ENV_SIZE_T("MAX_RECV_ITERS", 2);
    shm_ptr->max_out_of_order     = GET_ENV_SIZE_T("MAX_OUT_OF_ORDER", 2048);
    shm_ptr->rto_alpha            = GET_ENV_DOUBLE("RTO_ALPHA", 0.125);
    shm_ptr->rto_beta             = GET_ENV_DOUBLE("RTO_BETA", 0.25);
    shm_ptr->rto_granularity      = GET_ENV_DOUBLE("RTO_GRANULARITY", 0.001);
    shm_ptr->rto_lower_bound_sec  = GET_ENV_DOUBLE("RTO_LOWER_BOUND_SEC", 0.1);
    shm_ptr->rto_upper_bound_sec  = GET_ENV_DOUBLE("RTO_UPPER_BOUND_SEC", 60.0);
    shm_ptr->unsent_queue_cutoff  = GET_ENV_SIZE_T("UNSENT_QUEUE_CUTOFF", 1024);
    shm_ptr->beta_cubic           = GET_ENV_FLOAT("BETA_CUBIC", 0.7f);
    shm_ptr->cubic_c              = GET_ENV_FLOAT("C", 0.4f);
    shm_ptr->dup_ack_threshold    = GET_ENV_U32("DUP_ACK_THRESHOLD", 3);
    shm_ptr->waker_page_size      = GET_ENV_SIZE_T("WAKER_PAGE_SIZE", 64);
    shm_ptr->first_slot_size      = GET_ENV_SIZE_T("FIRST_SLOT_SIZE", 16);
    shm_ptr->waker_bit_length_shift = GET_ENV_SIZE_T("WAKER_BIT_LENGTH_SHIFT", 6);
    shm_ptr->fallback_mss         = GET_ENV_SIZE_T("FALLBACK_MSS", 536);
    shm_ptr->receive_batch_size   = GET_ENV_SIZE_T("RECEIVE_BATCH_SIZE", 4);
    shm_ptr->pop_size             = GET_ENV_SIZE_T("POP_SIZE", 1024);

    shm_ptr->num_rx_pkts = 0;
    shm_ptr->bytes_acked_per_sec = 0.0;
}

int main() {
    init_debug(); // Initialize logging system

    // Register signal handlers
    signal(SIGINT, handle_signal);
    signal(SIGTERM, handle_signal);

    int sockfd_local, connfd_local;
    struct sockaddr_un addr;

    sockfd_local = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sockfd_local == -1) {
        perror("socket");
        return 1;
    }
    sockfd = sockfd_local;

    unlink(SOCKET_PATH);
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strcpy(addr.sun_path, SOCKET_PATH);

    if (bind(sockfd, (struct sockaddr*)&addr, sizeof(addr)) == -1) {
        perror("bind");
        return 1;
    }

    if (listen(sockfd, 1) == -1) {
        perror("listen");
        return 1;
    }

    DEBUG_LOG("Controller: Waiting for feedback.rs to connect...\n");
    connfd_local = accept(sockfd, NULL, NULL);
    if (connfd_local == -1) {
        perror("accept");
        return 1;
    }
    connfd = connfd_local;

    shm_fd = shm_open(SHM_NAME, O_RDWR, 0666);
    if (shm_fd == -1) {
        perror("shm_open");
        return 1;
    }

    shm_ptr = mmap(NULL, SHM_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, shm_fd, 0);
    if (shm_ptr == MAP_FAILED) {
        perror("mmap");
        return 1;
    }

    efd = eventfd(0, EFD_NONBLOCK);
    if (efd == -1) {
        perror("eventfd");
        return 1;
    }

    if (send_fd(connfd, efd) == -1) {
        perror("send_fd");
        return 1;
    }
    DEBUG_LOG("Controller: Sent eventfd to feedback.rs\n");

    // Write initial config
    initialize_parameters_from_env(shm_ptr);
    
    uint64_t init_notify = 1;
    if (write(efd, &init_notify, sizeof(init_notify)) == -1) {
        perror("initial notify write");
        return 1;
    }
    DEBUG_LOG("Controller: Initial parameters written and notified\n");

    while (1) {
        uint64_t val;
    
        // Wait for a signal from feedback.rs via eventfd.
        // This acts as a "new data available" notification.
        int res = read(efd, &val, sizeof(val));
        if (res == -1) {
            if (errno == EAGAIN) {
                // No signal yet, avoid busy-waiting by sleeping briefly.
                // usleep(100000);
                continue;
            } else {
                perror("read eventfd");
                break;
            }
        }

        // At this point, feedback.rs has notified us that new data is ready.
        // Proceed to read from the shared memory.
        DEBUG_LOG("==== FEEDBACK RECEIVED ====\n");
        DEBUG_LOG("Parameters:\n");
        DEBUG_LOG("  timer_resolution: %zu\n", shm_ptr->timer_resolution);
        DEBUG_LOG("  receive_batch_size: %zu\n", shm_ptr->receive_batch_size);
        DEBUG_LOG("  rto_alpha: %.3f\n", shm_ptr->rto_alpha);
        DEBUG_LOG("Observations:\n");
        DEBUG_LOG("  num_rx_pkts: %zu\n", shm_ptr->num_rx_pkts);
        DEBUG_LOG("  bytes_acked_per_sec: %.2f\n", shm_ptr->bytes_acked_per_sec);
        DEBUG_LOG("===========================\n");

        // Apply logic to update parameters based on the feedback
        PROFILE_INFER_START;
        run_inference(shm_ptr);
        PROFILE_INFER_END;

        // Notify feedback.rs that controller has processed the update
        // and written new values into shared memory
        uint64_t notify = 1;
        if (write(efd, &notify, sizeof(notify)) == -1) {
            perror("notify feedback.rs");
            break;
        }
    
        // Optional: sleep(1) could be used here to pace interactions if needed
        // sleep(1)
    }

    cleanup();

    return 0;
}
