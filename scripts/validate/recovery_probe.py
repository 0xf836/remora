#!/usr/bin/env python3
"""Bounded test-only tagged UDP probes; never imported by production roles."""

import argparse
import hashlib
import json
from pathlib import Path
import signal
import socket
import struct
import time

MAGIC = b"RMR6"
LIMIT = 4096


def packet(sequence, sent_ns):
    body = struct.pack("!4sQQ", MAGIC, sequence, sent_ns) + b"Z" * 76
    return body + hashlib.sha256(body).digest()


def decode(data):
    if (
        len(data) != 128
        or data[:4] != MAGIC
        or hashlib.sha256(data[:-32]).digest() != data[-32:]
    ):
        raise ValueError("invalid test packet")
    _, sequence, sent_ns = struct.unpack("!4sQQ", data[:20])
    if sequence >= LIMIT:
        raise ValueError("sequence budget")
    return sequence, sent_ns


def publish(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, sort_keys=True))
    temporary.replace(path)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--role", choices=["echo", "client"], required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--report", type=Path, required=True)
    p.add_argument("--control", type=Path)
    a = p.parse_args()
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("127.0.0.1", a.port if a.role == "echo" else 0))
    s.settimeout(0.025)
    destination = ("127.0.0.1", a.port)
    running = True

    def stop(*_):
        nonlocal running
        running = False

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    begun = time.monotonic()
    next_send = begun
    last_publish = begun
    offered, received, seen = [], [], set()
    corrupt = duplicates = 0
    while running and time.monotonic() - begun < 180:
        now = time.monotonic()
        control = json.loads(a.control.read_text()) if a.control else {}
        if a.role == "client" and now >= next_send and not control.get("paused", False):
            if len(offered) == LIMIT:
                raise RuntimeError("probe budget exhausted")
            sent = time.monotonic_ns()
            sequence = len(offered)
            data = packet(sequence, sent)
            if s.sendto(data, destination) != len(data):
                raise RuntimeError("test generator submission")
            offered.append(dict(sequence=sequence, sent_ns=sent))
            next_send = now + 0.1
        try:
            data, source = s.recvfrom(65536)
            sequence, sent = decode(data)
            if a.role == "client":
                if (
                    source != destination
                    or sequence >= len(offered)
                    or offered[sequence]["sent_ns"] != sent
                ):
                    raise ValueError("wrong test association")
            if sequence in seen:
                duplicates += 1
            else:
                if len(received) == LIMIT:
                    raise RuntimeError("receive budget exhausted")
                seen.add(sequence)
                received.append(
                    dict(
                        sequence=sequence, sent_ns=sent, received_ns=time.monotonic_ns()
                    )
                )
            if a.role == "echo":
                s.sendto(data, source)
        except socket.timeout:
            pass
        except ValueError:
            corrupt += 1
        if now - last_publish >= 0.2 or not running:
            publish(
                a.report,
                dict(
                    offered=offered,
                    received=received,
                    corrupt=corrupt,
                    duplicates=duplicates,
                ),
            )
            last_publish = now
    s.close()
    publish(
        a.report,
        dict(
            offered=offered, received=received, corrupt=corrupt, duplicates=duplicates
        ),
    )


if __name__ == "__main__":
    main()
