Compile controller: gcc -o controller controller.c -lrt [-DPROFILE_INFERENCE]
Run controller: sudo [DEBUG=1] taskset -c 4 ./controller