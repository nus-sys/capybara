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

#define SHM_NAME "/shm_example"
#define SHM_SIZE sizeof(uint64_t)
#define SOCKET_PATH "/tmp/eventfd_socket"

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

int main() {
    int sockfd, connfd;
    struct sockaddr_un addr;

    // Create Unix domain socket
    sockfd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sockfd == -1) {
        perror("socket");
        return 1;
    }

    // Remove any existing socket
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

    printf("Controller: Waiting for test.rs to connect...\n");
    connfd = accept(sockfd, NULL, NULL);
    if (connfd == -1) {
        perror("accept");
        return 1;
    }

    // Create shared memory
    int shm_fd = shm_open(SHM_NAME, O_CREAT | O_RDWR, 0666);
    if (shm_fd == -1) {
        perror("shm_open");
        return 1;
    }

    // Resize shared memory
    if (ftruncate(shm_fd, SHM_SIZE) == -1) {
        perror("ftruncate");
        return 1;
    }

    // Map shared memory
    uint64_t *shm_ptr = mmap(NULL, SHM_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, shm_fd, 0);
    if (shm_ptr == MAP_FAILED) {
        perror("mmap");
        return 1;
    }

    // Write value to shared memory
    *shm_ptr = 7;
    printf("Controller: wrote 7 to shared memory\n");

    // Create eventfd
    int efd = eventfd(0, 0);
    if (efd == -1) {
        perror("eventfd");
        return 1;
    }

    // Send eventfd to Rust via Unix socket
    if (send_fd(connfd, efd) == -1) {
        perror("send_fd");
        return 1;
    }
    printf("Controller: Sent eventfd to test.rs\n");

    while(1){
        // Notify Rust
        uint64_t event_val = 1;
        if (write(efd, &event_val, sizeof(event_val)) == -1) {
            perror("write to eventfd");
            return 1;
        }
        printf("Controller: Sent notification via eventfd\nsleep for 3 seconds...");
        // Wait before notifying Rust
        sleep(5);
    }

    // Cleanup
    close(efd);
    close(connfd);
    close(sockfd);
    close(shm_fd);
    munmap(shm_ptr, SHM_SIZE);
    shm_unlink(SHM_NAME);
    unlink(SOCKET_PATH);

    return 0;
}
