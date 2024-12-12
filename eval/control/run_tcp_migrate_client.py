from socket import *
from sys import argv

ip = "10.0.1.8"
port = 10000

# Read command-line arguments if provided
pos = 1
if len(argv) > pos:
    ip = argv[pos]
    pos += 1
if len(argv) > pos:
    port = int(argv[pos])

# Create a plain TCP socket and connect
with socket(AF_INET, SOCK_STREAM) as s:
    s.connect((ip, port))
    print(f"Connected to {ip}:{port}")