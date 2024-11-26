import socket
import ssl

hostname = '10.0.1.8:10000'
context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
context.load_verify_locations('/usr/local/tls/CA.pem')

with socket.socket(socket.AF_INET, socket.SOCK_STREAM, 0) as sock:
    with context.wrap_socket(sock, server_hostname=hostname) as ssock:
        ssock.connect(('10.0.1.7', 2000))
        #ssock.send(b'hello')
        request = f"GET / HTTP/1.1\r\nHost: 10.0.1.7:2000\r\nConnection: close\r\n\r\n"

        # Send the request
        ssock.send(request.encode())

        # Receive the response
        response = b''
        data = ssock.recv(4096)
        response += data

        # Print the response (headers + body)
        print(response.decode())