from socket import *

s = socket(AF_INET, SOCK_STREAM)
s.connect(("10.0.1.8", 10000))
print(s)

pkt = ""
prompt = "> "
while True:
    try:
        x = input(prompt)
    except EOFError:
        print()
        break
    if x == "a":
        m = input(": ")
        if m:
            pkt += m
            prompt = ", "
        else:
            pkt += "\r\n\r\n"
            prompt = ". "
    elif x == "s":
        c = s.send(pkt.encode())
        pkt = ""
        prompt = f"({c}) "
    elif x == "r":
        r = s.recv(64)
        print(r)
        prompt = "> "
    else:
        prompt = "? "
