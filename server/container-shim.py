"""Entry point for a harness container started with --network none.

The container has only loopback networking. This shim listens on
127.0.0.1:3128 and relays each connection to the host's allowlisting egress
proxy through a mounted Unix socket, then runs the harness command.
"""
import signal
import socket
import subprocess
import sys
import threading

EGRESS = '/run/mcbench/egress.sock'


def pipe(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        try:
            dst.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def serve(listener):
    while True:
        client, _ = listener.accept()
        upstream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        upstream.connect(EGRESS)
        threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
        threading.Thread(target=pipe, args=(upstream, client), daemon=True).start()


listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(('127.0.0.1', 3128))
listener.listen(64)
threading.Thread(target=serve, args=(listener,), daemon=True).start()

child = subprocess.Popen(sys.argv[1:])
for sig in (signal.SIGTERM, signal.SIGINT):
    signal.signal(sig, lambda number, frame: child.send_signal(number))
sys.exit(child.wait())
