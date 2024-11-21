#include "./tlse.c"

#include <inttypes.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h> //strlen
#include <sys/epoll.h>

#ifdef _WIN32
#include <winsock2.h>
#define socklen_t int
#else
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

static char identity_str[0xFF] = {0};

int read_from_file(const char *fname, void *buf, int max_len) {
  FILE *f = fopen(fname, "rb");
  if (f) {
    int size = fread(buf, 1, max_len - 1, f);
    if (size > 0)
      ((unsigned char *)buf)[size] = 0;
    else
      ((unsigned char *)buf)[0] = 0;
    fclose(f);
    return size;
  }
  return 0;
}

void load_keys(struct TLSContext *context, char *fname, char *priv_fname) {
  unsigned char buf[0xFFFF];
  unsigned char buf2[0xFFFF];
  int size = read_from_file(fname, buf, 0xFFFF);
  int size2 = read_from_file(priv_fname, buf2, 0xFFFF);
  if (size > 0 && context) {
    tls_load_certificates(context, buf, size);
    tls_load_private_key(context, buf2, size2);
    // tls_print_certificate(fname);
  }
}

int send_pending(int client_sock, struct TLSContext *context) {
  unsigned int out_buffer_len = 0;
  const unsigned char *out_buffer =
      tls_get_write_buffer(context, &out_buffer_len);
  unsigned int out_buffer_index = 0;
  int send_res = 0;
  while ((out_buffer) && (out_buffer_len > 0)) {
    int res = send(client_sock, (char *)&out_buffer[out_buffer_index],
                   out_buffer_len, 0);
    if (res <= 0) {
      send_res = res;
      break;
    }
    out_buffer_len -= res;
    out_buffer_index += res;
  }
  tls_buffer_clear(context);
  return send_res;
}

// verify signature
int verify_signature(struct TLSContext *context,
                     struct TLSCertificate **certificate_chain, int len) {
  if (len) {
    struct TLSCertificate *cert = certificate_chain[0];
    if (cert) {
      snprintf(identity_str, sizeof(identity_str), "%s, %s(%s) (issued by: %s)",
               cert->subject, cert->entity, cert->location,
               cert->issuer_entity);
      fprintf(stderr, "Verified: %s\n", identity_str);
    }
  }
  return no_error;
}

int main(int argc, char *argv[]) {
  int socket_desc, client_sock, read_size;
  socklen_t c;
  struct sockaddr_in server, client;
  unsigned char client_message[0xFFFF];

#ifdef _WIN32
  WSADATA wsaData;
  WSAStartup(MAKEWORD(2, 2), &wsaData);
#else
  signal(SIGPIPE, SIG_IGN);
#endif

  socket_desc = socket(AF_INET, SOCK_STREAM, 0);
  if (socket_desc == -1) {
    printf("Could not create socket");
    return 0;
  }

  int port = 2000;
  if (argc > 1) {
    port = atoi(argv[1]);
    if (port <= 0)
      port = 2000;
  }
  server.sin_family = AF_INET;
  server.sin_addr.s_addr = INADDR_ANY;
  server.sin_port = htons(port);

  int enable = 1;
  setsockopt(socket_desc, SOL_SOCKET, SO_REUSEADDR, &enable, sizeof(int));

  if (bind(socket_desc, (struct sockaddr *)&server, sizeof(server)) < 0) {
    perror("bind failed. Error");
    return 1;
  }

  listen(socket_desc, 3);

  int epfd = epoll_create1(0);
  if (epfd == -1) {
    perror("epoll_create1");
    exit(EXIT_FAILURE);
  }
  struct epoll_event ev;
  ev.events = EPOLLIN;
  ev.data.fd = socket_desc;
  if (epoll_ctl(epfd, EPOLL_CTL_ADD, socket_desc, &ev) == -1) {
    perror("epoll_ctl: listen_sock");
    exit(EXIT_FAILURE);
  }

  c = sizeof(struct sockaddr_in);

  unsigned int size;

  struct TLSContext *server_context = tls_create_context(1, TLS_V12);
  // load keys
  load_keys(server_context, "/usr/local/tls/svr.crt", "/usr/local/tls/svr.key");

  char source_buf[0xFFFF];
  int source_size = sprintf(source_buf, "tlshelloworld.c");
  fprintf(stderr, "ready\n");

#define MAX_EVENTS 10
  struct epoll_event events[MAX_EVENTS];

  while (1) {
    identity_str[0] = 0;

    epoll_wait(epfd, events, MAX_EVENTS, -1);

    client_sock = accept(socket_desc, (struct sockaddr *)&client, &c);
    if (client_sock < 0) {
      perror("accept failed");
      return 1;
    }
    struct TLSContext *context = tls_accept(server_context);

    // uncomment next line to request client certificate
    tls_request_client_certificate(context);

    // make the TLS context serializable (this must be called before
    // negotiation)
    tls_make_exportable(context, 1);

    struct epoll_event ev;
    ev.events = EPOLLIN;
    ev.data.fd = client_sock;
    if (epoll_ctl(epfd, EPOLL_CTL_ADD, client_sock, &ev) == -1) {
      perror("epoll_ctl: conn_sock");
      exit(EXIT_FAILURE);
    }

    fprintf(stderr, "Client connected\n");
    while (1) {
      int nfds = epoll_wait(epfd, events, MAX_EVENTS, -1);
      if (nfds == -1) {
        perror("epoll_wait");
        return EXIT_FAILURE;
      }
      for (int n = 0; n < nfds; n += 1) {
        if (events[n].data.fd != client_sock) {
          continue;
        }
        while ((read_size = recv(client_sock, client_message,
                                 sizeof(client_message), 0)) > 0) {
          int consumed = tls_consume_stream(context, client_message, read_size,
                                            verify_signature);
          if (consumed < 0) {
            fprintf(stderr, "Error in stream consume\n");
            break;
          }
        }
        if (read_size < 0) {
          perror("cannot recv");
          return (EXIT_FAILURE);
        }
        if (tls_established(context) < 0) {
          fprintf(stderr, "Error in establishing tls\n");
          break;
        }
        if (tls_established(context) == 0) {
          continue;
        }
        fprintf(stderr, "USED CIPHER: %s\n", tls_cipher_name(context));
      }
    }

    send_pending(client_sock, context);

    if (read_size > 0) {
      fprintf(stderr, "USED CIPHER: %s\n", tls_cipher_name(context));
      int ref_packet_count = 0;
      int res;
      while ((read_size = recv(client_sock, client_message,
                               sizeof(client_message), 0)) > 0) {
        if (tls_consume_stream(context, client_message, read_size,
                               verify_signature) < 0) {
          fprintf(stderr, "Error in stream consume\n");
          break;
        }
        send_pending(client_sock, context);
        if (tls_established(context) == 1) {
          unsigned char read_buffer[0xFFFF];
          int read_size =
              tls_read(context, read_buffer, sizeof(read_buffer) - 1);
          if (read_size > 0) {
            read_buffer[read_size] = 0;
            unsigned char export_buffer[0xFFF];
            // simulate serialization / deserialization to another process
            char sni[0xFF];
            sni[0] = 0;
            if (context->sni)
              snprintf(sni, 0xFF, "%s", context->sni);
            /* COOL STUFF => */ int size = tls_export_context(
                context, export_buffer, sizeof(export_buffer), 1);
            if (size > 0) {
              /* COOLER STUFF => */ struct TLSContext *imported_context =
                  tls_import_context(export_buffer, size);
              // This is cool because a context can be sent to an existing
              // process. It will work both with fork and with already existing
              // worker process.
              fprintf(stderr, "Imported context (size: %i): %" PRIxPTR "\n",
                      size, (uintptr_t)imported_context);
              if (imported_context) {
                // destroy old context
                tls_destroy_context(context);
                // simulate serialization/deserialization of context
                context = imported_context;
              }
            }
            // ugly inefficient code ... don't write like me
            char send_buffer[0xF000];
            char send_buffer_with_header[0xF000];
            char out_buffer[0xFFF];
            int tls_version = 2;
            switch (context->version) {
            case TLS_V10:
              tls_version = 0;
              break;
            case TLS_V11:
              tls_version = 1;
              break;
            }
            snprintf(send_buffer, sizeof(send_buffer),
                     "Hello world from TLS 1.%i (used chipher is: %s), SNI: "
                     "%s\r\nYour identity is: %s\r\n\r\nCertificate: "
                     "%s\r\n\r\nBelow is the received header:\r\n%s\r\nAnd the "
                     "source code for this example: \r\n\r\n%s",
                     tls_version, tls_cipher_name(context), sni, identity_str,
                     tls_certificate_to_string(server_context->certificates[0],
                                               out_buffer, sizeof(out_buffer)),
                     read_buffer, source_buf);
            int content_length = strlen(send_buffer);
            snprintf(send_buffer_with_header, sizeof(send_buffer),
                     "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-type: "
                     "text/plain\r\nContent-length: %i\r\n\r\n%s",
                     content_length, send_buffer);
            tls_write(context, (unsigned char const *)send_buffer_with_header,
                      strlen(send_buffer_with_header));
            tls_close_notify(context);
            send_pending(client_sock, context);
            break;
          }
        }
      }
    }
#ifdef __WIN32
    shutdown(client_sock, SD_BOTH);
    closesocket(client_sock);
#else
    shutdown(client_sock, SHUT_RDWR);
    close(client_sock);
#endif
    tls_destroy_context(context);
  }
  tls_destroy_context(server_context);
  return 0;
}