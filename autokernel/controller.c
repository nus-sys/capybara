/*
 * controller.c - handles interprocess communication with user-space applications
 */

 #include <sys/socket.h>
 #include <sys/un.h>
 #include <unistd.h>
 #include <stdio.h>
 #include <stdlib.h>
 #include <string.h>
 #include <errno.h>
 #include <pthread.h>
 
//  #include "defs.h"
//  #include "sched.h"
 
 #define SOCKET_PATH "/tmp/controller_socket"
 
 static int server_socket;
 static pthread_t ipc_thread;
 
 static void *ipc_handler(void *arg)
 {
     int client_socket;
     char buffer[256];
     ssize_t bytes_received;
 
     printf("controller: IPC handler thread started.");
 
     while (1) {
         client_socket = accept(server_socket, NULL, NULL);
         if (client_socket < 0) {
             printf("controller: accept failed: %s", strerror(errno));
             continue;
         }
 
         bytes_received = read(client_socket, buffer, sizeof(buffer) - 1);
         if (bytes_received > 0) {
             buffer[bytes_received] = '\0';
             printf("controller: received message: %s", buffer);
             
             // Process message and send a response
             const char *response = "ACK";
             write(client_socket, response, strlen(response));
         }
 
         close(client_socket);
     }
     return NULL;
 }
 
 static int setup_ipc_server(void)
 {
     struct sockaddr_un addr;
 
     server_socket = socket(AF_UNIX, SOCK_STREAM, 0);
     if (server_socket < 0) {
         printf("controller: failed to create socket: %s", strerror(errno));
         return -1;
     }
 
     memset(&addr, 0, sizeof(addr));
     addr.sun_family = AF_UNIX;
     strncpy(addr.sun_path, SOCKET_PATH, sizeof(addr.sun_path) - 1);
     
     unlink(SOCKET_PATH);
     if (bind(server_socket, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
         printf("controller: bind failed: %s", strerror(errno));
         close(server_socket);
         return -1;
     }
 
     if (listen(server_socket, 5) < 0) {
         printf("controller: listen failed: %s", strerror(errno));
         close(server_socket);
         return -1;
     }
 
     printf("controller: IPC server listening on %s", SOCKET_PATH);
     return 0;
 }
 
 int controller_init(void)
 {
     if (setup_ipc_server() < 0)
         return -1;
 
     if (pthread_create(&ipc_thread, NULL, ipc_handler, NULL) != 0) {
         printf("controller: failed to create IPC thread");
         return -1;
     }
 
     return 0;
 }
 int main(void)
{
    printf("controller: starting IPC server");
    if (controller_init() < 0) {
        printf("controller: failed to initialize");
        return EXIT_FAILURE;
    }

    printf("controller: running");
    pthread_join(ipc_thread, NULL);
    return EXIT_SUCCESS;
}
