#!/usr/bin/env python3
"""Bounded two-direction integrity traffic for prepared WireGuard/UDP fixtures."""
import argparse
import hashlib
import json
import select
import socket
import struct
import time

HEADER = struct.Struct("!16sBBIH")
SIZES = (64, 128, 512, 1000)
WARMUP = 0xFFFFFFFF


def packet(run, channel, direction, sequence, size):
    head = HEADER.pack(run, channel, direction, sequence, size)
    body = bytes([(sequence + channel + direction) % 251]) * (size - HEADER.size - 16)
    return head + hashlib.sha256(head + body).digest()[:16] + body


def validate(data, run, channel, direction, count):
    if len(data) < HEADER.size + 16:
        return None
    r, c, d, seq, size = HEADER.unpack_from(data)
    if r != run or c != channel or d != direction or size != len(data):
        return None
    if seq != WARMUP and seq >= count:
        return None
    if data != packet(run, channel, direction, seq, size):
        return None
    return seq


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--role", choices=("a", "b"), required=True)
    p.add_argument("--wg-bind", required=True)
    p.add_argument("--wg-peer", required=True)
    p.add_argument("--generic-bind", required=True)
    p.add_argument("--generic-peer")
    p.add_argument("--run-id", required=True)
    p.add_argument("--start-at", type=float, required=True)
    p.add_argument("--seconds", type=int, required=True)
    a = p.parse_args()
    if not 5 <= a.seconds <= 1800 or len(bytes.fromhex(a.run_id)) != 16:
        p.error("bounded profile requires 5..1800 seconds and a 16-byte run id")
    delay = a.start_at - time.time()
    if not 2 <= delay <= 60:
        p.error("start must be 2..60 seconds in the future")
    run = bytes.fromhex(a.run_id)
    direction = 0 if a.role == "a" else 1
    count = a.seconds * 10
    start = time.monotonic() + delay
    finish = start + a.seconds
    channels = []
    def address(value):
        host, port = value.rsplit(":", 1)
        return host, int(port)
    for index, bind, peer in ((0, a.wg_bind, a.wg_peer),
                              (1, a.generic_bind, a.generic_peer)):
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.bind(address(bind))
        s.setblocking(False)
        channels.append(dict(socket=s, index=index, peer=address(peer) if peer else None,
                             sent=0, received=set(), invalid=0, duplicates=0,
                             send_errors=0, warm_send_errors=0, skipped=0, warm_received=0,
                             intervals=[0] * ((a.seconds + 29) // 30)))
    next_warm = time.monotonic()
    sequence = 0
    next_report = start + 30
    print(json.dumps({"event": "ready", "role": a.role, "seconds": a.seconds,
                      "pps_per_channel": 10, "sizes": SIZES}), flush=True)
    try:
        while time.monotonic() < finish + 2:
            now = time.monotonic()
            if now < start and now >= next_warm:
                for c in channels:
                    if c["peer"]:
                        try:
                            c["socket"].sendto(packet(run, c["index"], direction, WARMUP, 64), c["peer"])
                        except OSError:
                            c["warm_send_errors"] += 1
                next_warm = now + 0.1
            if now >= start and now < finish:
                due = min(count, int((now - start) * 10) + 1)
                # Scheduling failures remain visible; no unlimited catch-up burst.
                if due - sequence > 10:
                    for c in channels:
                        c["skipped"] += due - sequence - 10
                    sequence = due - 10
                while sequence < due:
                    for c in channels:
                        try:
                            if c["peer"] is None:
                                raise OSError("peer not learned")
                            c["socket"].sendto(packet(run, c["index"], direction,
                                                      sequence, SIZES[sequence % len(SIZES)]), c["peer"])
                            c["sent"] += 1
                        except OSError:
                            c["send_errors"] += 1
                    sequence += 1
            readable, _, _ = select.select([c["socket"] for c in channels], [], [], 0.01)
            for c in channels:
                if c["socket"] not in readable:
                    continue
                for _ in range(32):
                    try:
                        data, source = c["socket"].recvfrom(2048)
                    except BlockingIOError:
                        break
                    seq = validate(data, run, c["index"], 1 - direction, count)
                    if seq is None or (c["peer"] is not None and source != c["peer"]):
                        c["invalid"] += 1
                        continue
                    if c["peer"] is None:
                        c["peer"] = source
                    if seq == WARMUP:
                        c["warm_received"] += 1
                    elif seq in c["received"]:
                        c["duplicates"] += 1
                    else:
                        c["received"].add(seq)
                        c["intervals"][min(seq // 300, len(c["intervals"]) - 1)] += 1
            if now >= next_report and now < finish:
                print(json.dumps({"event": "progress", "role": a.role,
                                  "elapsed": round(now - start, 3),
                                  "received": [len(c["received"]) for c in channels]}), flush=True)
                next_report += 30
        result = {"event": "result", "role": a.role, "seconds": a.seconds,
                  "expected_per_channel": count, "channels": []}
        for c in channels:
            result["channels"].append({k: v for k, v in c.items() if k not in ("socket", "peer", "received")} | {"received": len(c["received"])})
        result["status"] = "pass" if all(c["sent"] == count and len(c["received"]) == count
            and c["warm_received"] > 0 and not any(c[k] for k in ("invalid", "duplicates", "send_errors", "skipped")) for c in channels) else "fail"
        print(json.dumps(result), flush=True)
        return 0 if result["status"] == "pass" else 1
    finally:
        for c in channels:
            c["socket"].close()


if __name__ == "__main__":
    raise SystemExit(main())
