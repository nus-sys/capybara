// https://github.com/onestraw/epoll-example/blob/master/epoll.c
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/epoll.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#define DEFAULT_PORT 10000
#define MAX_CONN 16
#define MAX_EVENTS 32
#define BUF_SIZE 16
#define MAX_LINE 256

int server_run();

int main(int argc, char *argv[]) { return server_run(); }

/*
 * register events of fd to epfd
 */
static int epoll_ctl_add(int epfd, int fd, uint32_t events) {
  struct epoll_event ev;
  ev.events = events;
  ev.data.fd = fd;
  if (epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &ev) == -1) {
    perror("epoll_ctl()\n");
    return -1;
  }
  return 0;
}

static void set_sockaddr(struct sockaddr_in *addr) {
  bzero((char *)addr, sizeof(struct sockaddr_in));
  addr->sin_family = AF_INET;
  addr->sin_addr.s_addr = INADDR_ANY;
  addr->sin_port = htons(DEFAULT_PORT);
}

static int setnonblocking(int sockfd) {
  if (fcntl(sockfd, F_SETFL, fcntl(sockfd, F_GETFL, 0) | O_NONBLOCK) == -1) {
    return -1;
  }
  return 0;
}

/*
 * epoll echo server
 */
int server_run() {
  int i, n, epfd, nfds, listen_sock, conn_sock, code;
  socklen_t socklen;
  char buf[BUF_SIZE];
  struct sockaddr_in srv_addr;
  struct sockaddr_in cli_addr;
  struct epoll_event events[MAX_EVENTS];

  listen_sock = socket(AF_INET, SOCK_STREAM, 0);

  set_sockaddr(&srv_addr);
  if ((code =
           bind(listen_sock, (struct sockaddr *)&srv_addr, sizeof(srv_addr)))) {
    perror("bind");
    return code;
  }

  if ((code = setnonblocking(listen_sock))) {
    perror("setnonblocking listen_sock");
    return code;
  }
  if ((code = listen(listen_sock, MAX_CONN))) {
    perror("bind");
    return code;
  }

  epfd = epoll_create1(0);
  if (epfd < 0) {
    perror("epoll_create1");
    return epfd;
  }
  if ((code = epoll_ctl_add(epfd, listen_sock, EPOLLIN))) {
    return code;
  }

  socklen = sizeof(cli_addr);
  int N = 0;
  for (; N < 100; N += 1) {
    nfds = epoll_wait(epfd, events, MAX_EVENTS, -1);
    if (nfds < 0) {
      perror("epoll_wait");
      return nfds;
    }
    printf("nfds=%d\n", nfds);
    for (i = 0; i < nfds; i += 1) {
      if (events[i].data.fd == listen_sock) {
        /* handle new connection */
        conn_sock = accept(listen_sock, (struct sockaddr *)&cli_addr, &socklen);
        if (conn_sock < 0) {
          perror("accept");
          return conn_sock;
        }

        inet_ntop(AF_INET, (char *)&(cli_addr.sin_addr), buf, sizeof(cli_addr));
        printf("[+] connected with %s:%d\n", buf, ntohs(cli_addr.sin_port));

        if ((code = setnonblocking(conn_sock))) {
          perror("setnonblocking conn_sock");
          return code;
        }
        if ((code = epoll_ctl_add(epfd, conn_sock, EPOLLIN | EPOLLRDHUP))) {
          return code;
        }
      } else if (events[i].events & EPOLLIN) {
        /* handle EPOLLIN event */
        for (;;) {
          bzero(buf, sizeof(buf));
          n = read(events[i].data.fd, buf, sizeof(buf));
          if (n <= 0) {
            if (n == 0 || errno == EAGAIN) {
              break;
            }
            perror("read");
            return n;
          }
          printf("[+] data: %s\n", buf);
          n = write(events[i].data.fd, buf, strlen(buf));
          if (n < 0 && n != EAGAIN) {
            perror("write");
            return n;
          }
        }
      } else {
        printf("[+] unexpected fd=%d events=%u\n", events[i].data.fd,
               events[i].events);
      }
      /* check if the connection is closing */
      if (events[i].events & EPOLLRDHUP) {
        printf("[+] connection closed fd=%d\n", events[i].data.fd);
        if ((code = epoll_ctl(epfd, EPOLL_CTL_DEL, events[i].data.fd, NULL))) {
          perror("epoll_ctl");
          return code;
        }
        if ((code = close(events[i].data.fd))) {
          perror("close");
          return code;
        }
        continue;
      }
    }
  }
  return 0;
}

#if 0
/*
 * test clinet
 */
void client_run() {
  int n;
  int c;
  int sockfd;
  char buf[MAX_LINE];
  struct sockaddr_in srv_addr;

  sockfd = socket(AF_INET, SOCK_STREAM, 0);

  set_sockaddr(&srv_addr);

  if (connect(sockfd, (struct sockaddr *)&srv_addr, sizeof(srv_addr)) < 0) {
    perror("connect()");
    exit(1);
  }

  for (;;) {
    printf("input: ");
    fgets(buf, sizeof(buf), stdin);
    c = strlen(buf) - 1;
    buf[c] = '\0';
    write(sockfd, buf, c + 1);

    bzero(buf, sizeof(buf));
    while (errno != EAGAIN && (n = read(sockfd, buf, sizeof(buf))) > 0) {
      printf("echo: %s\n", buf);
      bzero(buf, sizeof(buf));

      c -= n;
      if (c <= 0) {
        break;
      }
    }
  }
  close(sockfd);
}
#endif