from socket import *

sock = socket(AF_INET, SOCK_STREAM)
sock.connect(("10.0.1.8", 10000))
print(sock)
while True:
    try:
        x = input("> ")
    except EOFError:
        print()
        break
    l = sock.send(f"{x}\r\n\r\n".encode())
    print("sent bytes:", l)
    print(sock.recv(32))
