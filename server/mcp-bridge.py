"""Relays a stdio MCP session to the turn's or attempt's MCP Unix socket.

Harnesses start this as an ordinary stdio MCP server. It runs on the host or,
for containerized harnesses, inside the container with the socket mounted.
"""
import os
import socket
import sys
import threading

conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
conn.connect(sys.argv[1])


def upstream():
    while True:
        data = os.read(0, 65536)
        if not data:
            conn.shutdown(socket.SHUT_WR)
            return
        conn.sendall(data)


threading.Thread(target=upstream, daemon=True).start()
out = sys.stdout.buffer
while True:
    data = conn.recv(65536)
    if not data:
        break
    out.write(data)
    out.flush()
