#!/usr/bin/env python3
"""Test-only opaque UDP integrity probe, executed only in owned namespaces."""

import argparse
import hashlib
import json
import socket
import signal
import time


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--role", choices=["endpoint", "probe"], required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--max-size", type=int, default=1200)
    a = p.parse_args()
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    if a.role == "endpoint":
        s.bind(("127.0.0.1", a.port))
        running = True
        counts = {str(n): 0 for n in [0, 1, 128, 512, 960, 1200, 1201]}
        counts["other"] = 0

        def stop(_sig, _frame):
            nonlocal running
            running = False

        signal.signal(signal.SIGTERM, stop)
        s.settimeout(0.1)
        while running:
            try:
                data, peer = s.recvfrom(65536)
            except TimeoutError:
                continue
            key = str(len(data)) if str(len(data)) in counts else "other"
            counts[key] += 1
            s.sendto(data, peer)
        print(json.dumps({"received_by_size": counts}), flush=True)
        return
    s.bind(("127.0.0.1", 0))
    s.settimeout(0.25)
    to = ("127.0.0.1", a.port)
    serial = 0
    known = set()
    late_replies = 0

    def packet(size):
        nonlocal serial
        serial += 1
        data = hashlib.sha256(serial.to_bytes(8, "big")).digest()
        payload = (serial.to_bytes(8, "big") + data + bytes(size))[:size]
        known.add(payload)
        return payload

    def probe(payload, deadline=0.25):
        nonlocal late_replies
        started = time.monotonic()
        s.sendto(payload, to)
        while time.monotonic() - started < deadline:
            try:
                response, peer = s.recvfrom(65536)
            except TimeoutError:
                return False, None
            if peer != to:
                raise RuntimeError("unexpected probe association")
            if response == payload:
                return True, (time.monotonic() - started) * 1000
            if response not in known:
                raise RuntimeError("corrupted or cross-delivered UDP payload")
            late_replies += 1
            # A late reply from the same immutable association is accounted
            # separately in pressure below. Here every probe has a unique tag.

    deadline = time.monotonic() + 5
    while True:
        ok, _ = probe(packet(128))
        if ok:
            break
        if time.monotonic() > deadline:
            raise TimeoutError("healthy small probe setup")
    rows = []
    for size in sorted(set([0, 1, 128, 512, min(a.max_size, 960), a.max_size])):
        rtts, lost = [], 0
        for _ in range(20):
            ok, rtt = probe(packet(size))
            if ok:
                rtts.append(rtt)
            else:
                lost += 1
        rows.append(
            {
                "bytes": size,
                "offered": 20,
                "received": len(rtts),
                "lost": lost,
                "rtt_ms": rtts,
            }
        )
    # Above configured application ceiling must not echo. Match only its tag.
    oversized = packet(1201)
    above_received, _ = probe(oversized)
    pending = {packet(128) for _ in range(64)}
    for payload in pending:
        s.sendto(payload, to)
    original = len(pending)
    duplicates = corrupt = 0
    seen = set()
    deadline = time.monotonic() + 1
    while time.monotonic() < deadline:
        try:
            data, peer = s.recvfrom(65536)
        except TimeoutError:
            continue
        if peer != to:
            corrupt += 1
        elif data in pending:
            pending.remove(data)
            seen.add(data)
        elif data in seen:
            duplicates += 1
        elif data not in known:
            corrupt += 1
        else:
            late_replies += 1
        # Earlier, timed-out sequential replies are not pressure corruption.
    healthy, _ = probe(packet(128), 1)
    result = {
        "sizes": rows,
        "above_limit_received": above_received,
        "burst": {
            "offered": original,
            "received": len(seen),
            "lost": len(pending),
            "duplicates": duplicates,
            "wrong_association": corrupt,
        },
        "control_progress": healthy,
        "late_replies": late_replies,
        "capacity_claim": False,
    }
    print(json.dumps(result), flush=True)
    if (
        above_received
        or duplicates
        or corrupt
        or not healthy
        or any(r["received"] == 0 for r in rows if r["bytes"] <= 512)
    ):
        raise RuntimeError("boundary, association or healthy-flow failure")


if __name__ == "__main__":
    main()
