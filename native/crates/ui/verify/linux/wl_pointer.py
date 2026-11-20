"""Pointer input and window close on the private cage, as xdotool gives on
X11: a Wayland client speaking the wire protocol itself (no library), with a
virtual pointer (`zwlr_virtual_pointer_manager_v1`) and the foreign-toplevel
manager (`zwlr_foreign_toplevel_manager_v1`) for close.

    wl_pointer.py globals
    wl_pointer.py mousemove X Y
    wl_pointer.py mousemove_relative DX DY
    wl_pointer.py mousedown BUTTON | mouseup BUTTON
    wl_pointer.py click [--repeat N] [--delay MS] BUTTON
    wl_pointer.py close
    wl_pointer.py identity   (each toplevel's app_id and title, one line each)
    wl_pointer.py output WIDTHxHEIGHT   (the one output's size, as cage's
                                         client is drawn: like Xvfb's screen)
    wl_pointer.py serve   (the pointer commands, one per line on stdin)

Buttons as xdotool numbers them: 1 left, 2 middle, 3 right, 4 wheel up,
5 wheel down. X and Y are output pixels (cage shows its one client
fullscreen, so they are window pixels too). The socket must be in the
session's private runtime folder (XDG_RUNTIME_DIR, never under /run): the
client checks before it connects."""

import os
import socket
import struct
import sys
import time

BTN = {1: 0x110, 2: 0x112, 3: 0x111}


def socket_path():
    runtime = os.environ["XDG_RUNTIME_DIR"]
    display = os.environ["WAYLAND_DISPLAY"]
    path = display if display.startswith("/") else os.path.join(runtime, display)
    real, private = os.path.realpath(path), os.path.realpath(runtime)
    if private.startswith("/run/") or not real.startswith(private + "/"):
        sys.exit(f"wl_pointer.py: {real} is not in the session's private runtime folder")
    return real


def string(text):
    data = text.encode() + b"\0"
    return struct.pack("<I", len(data)) + data + b"\0" * (-len(data) % 4)


def fixed(value):
    return struct.pack("<i", int(value * 256))


class Client:
    def __init__(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(socket_path())
        self.next_id = 2
        self.buffer = b""
        self.handlers = {}
        self.globals = {}
        self.registry = self.new_id()
        self.send(1, 1, struct.pack("<I", self.registry))  # wl_display.get_registry
        self.handlers[self.registry] = self.on_registry
        self.roundtrip()

    def new_id(self):
        self.next_id += 1
        return self.next_id - 1

    def send(self, obj, opcode, payload=b""):
        self.sock.sendall(struct.pack("<II", obj, ((8 + len(payload)) << 16) | opcode) + payload)

    def on_registry(self, opcode, payload):
        if opcode == 0:
            name, length = struct.unpack_from("<II", payload)
            interface = payload[8:8 + length - 1].decode()
            offset = 8 + length + (-length % 4)
            (version,) = struct.unpack_from("<I", payload, offset)
            self.globals[interface] = (name, version)

    def bind(self, interface, version):
        name, available = self.globals[interface]
        new = self.new_id()
        self.send(self.registry, 0, struct.pack("<I", name) + string(interface)
                  + struct.pack("<II", min(version, available), new))
        return new

    def dispatch(self):
        while len(self.buffer) >= 8:
            obj, word = struct.unpack_from("<II", self.buffer)
            size, opcode = word >> 16, word & 0xFFFF
            if len(self.buffer) < size:
                return False
            payload, self.buffer = self.buffer[8:size], self.buffer[size:]
            if obj == 1 and opcode == 0:
                sys.exit(f"wl_pointer.py: protocol error {payload!r}")
            handler = self.handlers.get(obj)
            if handler:
                handler(opcode, payload)
        return True

    def roundtrip(self):
        callback = self.new_id()
        done = []
        self.handlers[callback] = lambda opcode, payload: done.append(True)
        self.send(1, 0, struct.pack("<I", callback))  # wl_display.sync
        while not done:
            self.buffer += self.sock.recv(65536)
            self.dispatch()

    def output_size(self):
        output = self.bind("wl_output", 2)
        size = []

        def on_output(opcode, payload):
            if opcode == 1:  # mode(flags, width, height, refresh)
                flags, width, height, _ = struct.unpack_from("<Iiii", payload)
                if flags & 1:
                    size[:] = [width, height]

        self.handlers[output] = on_output
        self.roundtrip()
        return size


def now_ms():
    return int(time.monotonic() * 1000) & 0xFFFFFFFF


def main():
    args = sys.argv[1:]
    client = Client()
    if args[0] == "globals":
        for interface, (_, version) in sorted(client.globals.items()):
            print(interface, version)
        return
    if args[0] == "output":
        width, height = (int(v) for v in args[1].split("x"))
        manager = client.bind("zwlr_output_manager_v1", 4)
        heads, serial = [], []

        def on_manager(opcode, payload):
            if opcode == 0:  # head(new head)
                heads.append(struct.unpack_from("<I", payload)[0])
            elif opcode == 1:  # done(serial)
                serial.append(struct.unpack_from("<I", payload)[0])

        client.handlers[manager] = on_manager
        client.roundtrip()
        if len(heads) != 1 or not serial:
            sys.exit(f"wl_pointer.py: {len(heads)} outputs, expected one")
        config, config_head, result = client.new_id(), client.new_id(), []
        client.handlers[config] = lambda opcode, payload: result.append(opcode)
        client.send(manager, 0, struct.pack("<II", config, serial[-1]))  # create_configuration
        client.send(config, 0, struct.pack("<II", config_head, heads[0]))  # enable_head
        client.send(config_head, 1, struct.pack("<iii", width, height, 0))  # set_custom_mode
        client.send(config, 2)  # apply
        while not result:
            client.roundtrip()
        if result[0] != 0:
            sys.exit(f"wl_pointer.py: the output configuration failed ({result[0]})")
        return
    if args[0] == "close":
        manager = client.bind("zwlr_foreign_toplevel_manager_v1", 3)
        handles = []

        def on_manager(opcode, payload):
            if opcode == 0:  # toplevel(new handle)
                handles.append(struct.unpack_from("<I", payload)[0])

        client.handlers[manager] = on_manager
        client.roundtrip()
        if len(handles) != 1:
            sys.exit(f"wl_pointer.py: {len(handles)} toplevels, expected the app's one")
        client.send(handles[0], 5)  # zwlr_foreign_toplevel_handle_v1.close
        client.roundtrip()
        return
    if args[0] == "identity":
        manager = client.bind("zwlr_foreign_toplevel_manager_v1", 3)
        toplevels = {}

        def on_handle(handle):
            def handler(opcode, payload):
                if opcode in (0, 1):  # title(string), app_id(string)
                    (length,) = struct.unpack_from("<I", payload)
                    value = payload[4:4 + length - 1].decode()
                    toplevels[handle]["title" if opcode == 0 else "app_id"] = value
            return handler

        def on_manager(opcode, payload):
            if opcode == 0:  # toplevel(new handle)
                handle = struct.unpack_from("<I", payload)[0]
                toplevels[handle] = {}
                client.handlers[handle] = on_handle(handle)

        client.handlers[manager] = on_manager
        client.roundtrip()
        client.roundtrip()
        for fields in toplevels.values():
            print(f"app_id={fields.get('app_id', '')!r} title={fields.get('title', '')!r}")
        return
    pointer = Pointer(client)
    if args[0] == "serve":
        # One command per line on stdin, "ok" per command on stdout: one
        # virtual pointer for a whole gesture (a held button stays held).
        for line in sys.stdin:
            pointer.run(line.split())
            print("ok", flush=True)
    else:
        pointer.run(args)
    pointer.destroy()


class Pointer:
    def __init__(self, client):
        self.client = client
        self.width, self.height = client.output_size()
        manager = client.bind("zwlr_virtual_pointer_manager_v1", 1)
        self.id = client.new_id()
        client.send(manager, 0, struct.pack("<II", 0, self.id))  # create_virtual_pointer(no seat)
        client.roundtrip()

    def frame(self):
        self.client.send(self.id, 4)

    def button(self, number, pressed):
        if number in (4, 5):
            if pressed:
                self.client.send(self.id, 5, struct.pack("<I", 0))  # axis_source: wheel
                self.client.send(self.id, 7, struct.pack("<II", now_ms(), 0) + fixed(-15 if number == 4 else 15)
                                 + struct.pack("<i", -1 if number == 4 else 1))  # axis_discrete, vertical
                self.frame()
            return
        self.client.send(self.id, 2, struct.pack("<III", now_ms(), BTN[number], 1 if pressed else 0))
        self.frame()

    def run(self, args):
        command = args[0]
        if command == "mousemove":
            x, y = int(args[1]), int(args[2])
            self.client.send(self.id, 1, struct.pack("<IIIII", now_ms(), x, y, self.width, self.height))
            self.frame()
        elif command == "mousemove_relative":
            # motion(time, dx, dy): relative, as a mouse reports it.
            self.client.send(self.id, 0, struct.pack("<I", now_ms()) + fixed(float(args[1])) + fixed(float(args[2])))
            self.frame()
        elif command in ("mousedown", "mouseup"):
            self.button(int(args[1]), command == "mousedown")
        elif command == "click":
            repeat, delay, rest = 1, 0.1, args[1:]
            while rest[0].startswith("--"):
                if rest[0] == "--repeat":
                    repeat = int(rest[1])
                elif rest[0] == "--delay":
                    delay = int(rest[1]) / 1000
                rest = rest[2:]
            number = int(rest[0])
            for i in range(repeat):
                self.button(number, True)
                self.button(number, False)
                self.client.roundtrip()
                if i + 1 < repeat:
                    time.sleep(delay)
        else:
            sys.exit(f"wl_pointer.py: unknown command {command}")
        self.client.roundtrip()

    def destroy(self):
        self.client.send(self.id, 8)
        self.client.roundtrip()

main()
