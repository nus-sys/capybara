from socket import *

sock = socket(AF_INET, SOCK_STREAM)
sock.connect(("10.0.1.8", 10000))
print(sock)
xs = []
while True:
    try:
        x = input("> ")
    except EOFError:
        print()
        break
    if x.endswith(".."):
        xs.append(x[:-2])
        continue
    x += "\r\n\r\n"
    for xx in xs + [x]:
        l = sock.send(xx.encode())
        print("sent bytes:", l)
    xs = []
    print(sock.recv(32))
