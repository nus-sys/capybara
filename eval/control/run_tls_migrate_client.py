from socket import *
from sys import argv
from ssl import *

ip = "10.0.1.8"
port = 10000

pos = 1
if len(argv) > pos:
    ip = argv[pos]
    pos += 1
if len(argv) > pos:
    port = int(argv[pos])


hostname = '10.0.1.8:10000'
context = SSLContext(PROTOCOL_TLS_CLIENT)
context.load_verify_locations('/usr/local/tls/CA.pem')

with socket(AF_INET, SOCK_STREAM) as sock:
    with context.wrap_socket(sock, server_hostname=hostname) as s:
        s.connect((ip, port))
        print(s)
