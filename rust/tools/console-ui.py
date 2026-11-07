#!/usr/bin/env python3
"""Local SSH UI driver using Linux input events and fbgrab.

No application internals are bypassed. Start explicitly on the console and
submit JSON batches through its private Unix socket. Never enables Bluetooth.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import time

KEYS = {"UP": 103, "DOWN": 108, "LEFT": 105, "RIGHT": 106,
        "A": 28, "B": 14, "X": 57, "Y": 21, "START": 59,
        "L": 18, "R": 20, "HOLD": 35, "MENU": 50, "SEARCH": 33,
        "QUIT": 16, "VOLUP": 13, "VOLDOWN": 12, "BRIGHTUP": 104,
        "BRIGHTDOWN": 109}


def event(fd, kind, code, value):
    os.write(fd, struct.pack("llHHi", 0, 0, kind, code, value))
    os.write(fd, struct.pack("llHHi", 0, 0, 0, 0, 0))


def serve(address, output, device):
    output.mkdir(parents=True, exist_ok=True)
    if device:
        fd = os.open(device, os.O_WRONLY)
        bindings = {key: (1, code, 1) for key, code in {
            "A": 304, "B": 305, "X": 307, "Y": 306, "L": 308,
            "R": 309, "HOLD": 310, "START": 311, "MENU": 312,
            "VOLUP": 115, "VOLDOWN": 114}.items()}
        bindings.update({"UP": (3, 17, -1), "DOWN": (3, 17, 1),
                         "LEFT": (3, 16, -1), "RIGHT": (3, 16, 1)})
    else:
        fd = os.open("/dev/uinput", os.O_WRONLY)
        fcntl.ioctl(fd, 0x40045564, 1)  # UI_SET_EVBIT(EV_KEY)
        for code in KEYS.values():
            fcntl.ioctl(fd, 0x40045565, code)  # UI_SET_KEYBIT
        os.write(fd, struct.pack("80sHHHHI", b"betterPod QA keyboard", 3,
                                0x1209, 0x4250, 1, 0) + bytes(1024))
        fcntl.ioctl(fd, 0x5501)  # UI_DEV_CREATE
        bindings = {key: (1, code, 1) for key, code in KEYS.items()}
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    address.unlink(missing_ok=True)
    sock.bind(str(address))
    os.chmod(address, 0o600)
    sock.listen(2)
    running = True

    def stop(_signum, _frame):
        nonlocal running
        running = False
        sock.close()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    time.sleep(.5)
    print("ready", flush=True)
    try:
        while running:
            try:
                conn, _ = sock.accept()
            except OSError:
                break
            with conn:
                conn.settimeout(5)
                try:
                    data = bytearray()
                    while b"\n" not in data:
                        chunk = conn.recv(65536)
                        if not chunk:
                            raise ValueError("Incomplete request")
                        data.extend(chunk)
                        if len(data) > 256 * 1024:
                            raise ValueError("Request too large")
                    request = json.loads(data.partition(b"\n")[0])
                    subprocess.run(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "1"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   check=True, timeout=5)
                    results = []
                    for step in request.get("steps", []):
                        if "key" in step:
                            kind, code, value = bindings[step["key"]]
                            for _ in range(min(100, max(1, int(step.get("n", 1))))):
                                event(fd, kind, code, value)
                                time.sleep(min(1.5, max(.04, step.get("ms", 100) / 1000)))
                                event(fd, kind, code, 0)
                                time.sleep(max(.04, min(1., step.get("gap", .09))))
                        time.sleep(max(0., min(15., step.get("wait", 0.))))
                        if "capture" in step:
                            name = step["capture"]
                            if not name or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for c in name):
                                raise ValueError("Invalid capture name")
                            path = output / (name + ".png")
                            subprocess.run(["fbgrab", "-a", str(path)], check=True,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                           timeout=5)
                            results.append({"capture": name, "bytes": path.stat().st_size})
                    response = {"ok": True, "captures": results}
                except Exception as error:
                    response = {"ok": False, "error": str(error)}
                conn.sendall(json.dumps(response).encode() + b"\n")
    finally:
        for kind, code, _value in bindings.values():
            event(fd, kind, code, 0)
        if not device:
            fcntl.ioctl(fd, 0x5502)  # UI_DEV_DESTROY
        os.close(fd)
        sock.close()
        address.unlink(missing_ok=True)


def run(address, file):
    request = json.loads(file.read_text())
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(300)
        sock.connect(str(address))
        sock.sendall(json.dumps(request).encode() + b"\n")
        data = bytearray()
        while b"\n" not in data:
            chunk = sock.recv(65536)
            if not chunk:
                raise ConnectionError("QA driver closed without a reply")
            data.extend(chunk)
    print(data.decode().strip())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["serve", "run"])
    parser.add_argument("--socket", type=Path, default=Path("/tmp/betterpod-rust-qa.sock"))
    parser.add_argument("--output", type=Path, default=Path("/userdata/system/betterpod-rust-qa/captures"))
    parser.add_argument("--file", type=Path)
    parser.add_argument("--device", type=Path)
    args = parser.parse_args()
    if args.mode == "serve":
        serve(args.socket, args.output, args.device)
    else:
        run(args.socket, args.file)
