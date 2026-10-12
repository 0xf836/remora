#!/usr/bin/env python3
"""Read-only, bounded Linux ethtool ioctl inventory for a test-owned veth."""

import ctypes
import fcntl
import json
import socket
import struct
import sys


def features(device):
    encoded = device.encode("ascii")
    if len(encoded) >= 16 or device not in ["p5c", "p5s"]:
        raise ValueError("expected owned test veth")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:

        def query(data):
            # Initialized, stable ctypes storage remains owned through ioctl.
            # ifreq contains only the fixed interface name and borrowed data
            # pointer; no SET command is permitted here, and all lengths are
            # bounded before allocating the flexible arrays.
            buffer = ctypes.create_string_buffer(data, len(data))
            request = struct.pack("16sP", encoded, ctypes.addressof(buffer)).ljust(
                40, b"\0"
            )
            fcntl.ioctl(sock.fileno(), 0x8946, request)  # SIOCETHTOOL
            return buffer.raw

        info = query(struct.pack("IIQI", 0x37, 0, 1 << 4, 0))
        mask, count = struct.unpack_from("QI", info, 8)
        if mask != 1 << 4 or not 1 <= count <= 256:
            raise ValueError("unsupported or unbounded feature set")
        names = query(struct.pack("III", 0x1B, 4, count) + bytes(32 * count))
        blocks = (count + 31) // 32
        flags = query(struct.pack("II", 0x3A, blocks) + bytes(16 * blocks))
        if struct.unpack_from("I", flags, 4)[0] != blocks:
            raise ValueError("changing feature set")
        result = {}
        for i in range(count):
            name = (
                names[12 + i * 32 : 12 + (i + 1) * 32]
                .split(b"\0", 1)[0]
                .decode("ascii")
            )
            available, requested, active, fixed = struct.unpack_from(
                "IIII", flags, 8 + i // 32 * 16
            )
            bit = 1 << (i % 32)
            result[name] = {
                "available": bool(available & bit),
                "requested": bool(requested & bit),
                "active": bool(active & bit),
                "never_changed": bool(fixed & bit),
            }
        return result


if __name__ == "__main__":
    print(json.dumps(features(sys.argv[1])))
