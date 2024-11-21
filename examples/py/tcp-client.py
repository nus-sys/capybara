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
    eom = not x.endswith("..")
    if eom:
        x += "\r\n\r\n"
    else:
        x = x[:-2]
    l = sock.send(x.encode())
    print("sent bytes:", l)
    if eom:
        print(sock.recv(32))
