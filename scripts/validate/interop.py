#!/usr/bin/env python3
"""Independent-stack fixtures: private inputs, owned processes, monitored target."""

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import tomllib
from benchmark import Owned, ROOT, write, sha

PIN = "c1cf0e4dd6439aea94d5491b27439a4736f00246"


def run_case(out, peer, family, direction, case):
    owned = []
    with tempfile.TemporaryDirectory(prefix="remora-interop-") as tmp:
        f = Path(tmp) / "fixtures"
        subprocess.run(
            [
                str(ROOT / "scripts/validate/fixtures.sh"),
                "--output",
                str(f),
                "--family",
                family,
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        # Supplied fixtures are validated, not overwritten; each case owns new
        # private credentials and ephemeral ports to avoid concurrent binds.
        client = tomllib.loads((f / "client.toml").read_text())["client"]
        server = tomllib.loads((f / "server.toml").read_text())["server"]
        credentials = tomllib.loads((f / "credentials.toml").read_text())["identities"][
            0
        ]
        target_host = client["target_host"]
        target = (
            f'[{target_host}]:{client["target_port"]}'
            if ":" in target_host
            else f'{target_host}:{client["target_port"]}'
        )
        cfg = {
            "role": "client" if direction == "independent-client" else "server",
            "listen": server["listen"][0],
            "server_address": client["server_address"],
            "authority": client["proxy_authority"],
            "target": target,
            "server_name": client["server_name"],
            "ca_file": str(f / "ca.pem"),
            "certificate_file": str(f / "server.pem"),
            "key_file": str(f / "server-key.pem"),
            "token_file": str(f / "client.token"),
            "digest": credentials["token_sha256"],
            "case": case,
            "flows": 2,
        }
        if case in ["hostname-positive", "mixed-dns"]:
            if family != "ipv6" or direction != "independent-client":
                raise ValueError("hostname case profile")
            answers = {
                entry[4][0]
                for entry in socket.getaddrinfo(
                    "localhost", client["target_port"], type=socket.SOCK_DGRAM
                )
            }
            if not {"127.0.0.1", "::1"}.issubset(answers):
                raise ValueError("localhost dual-answer fixture unavailable")
            p = f / "credentials.toml"
            text = p.read_text().replace('host = "::1"', 'host = "localhost"')
            if case == "hostname-positive":
                text = text.replace(
                    'addresses = ["::1/128"]', 'addresses = ["::1/128", "127.0.0.1/32"]'
                )
            p.write_text(text)
            cfg["target"] = f'localhost:{client["target_port"]}'
        if case == "denied-target":
            allowed_port = client["target_port"] % 65535 + 1
            if direction == "independent-client":
                # Request the monitored loopback endpoint, but authorize a
                # different port. No traffic can escape to an unmonitored IP.
                p = f / "credentials.toml"
                entry = f'port = {client["target_port"]}\n'
                text = p.read_text()
                if text.count(entry) != 1:
                    raise ValueError("denied-target policy fixture")
                p.write_text(text.replace(entry, f"port = {allowed_port}\n"))
            else:
                # This is the Go server's allowlist, not the Remora client's
                # requested endpoint, which remains in client.toml.
                cfg["target"] = (
                    f"[{target_host}]:{allowed_port}"
                    if ":" in target_host
                    else f"{target_host}:{allowed_port}"
                )
        if case == "revoked-token":
            if direction == "independent-client":
                p = f / "credentials.toml"
                p.write_text(
                    p.read_text().replace(credentials["token_sha256"], "0" * 64)
                )
                cfg["case"] = "bad-token-response"
            else:
                cfg["digest"] = "0" * 64
        if direction == "independent-server" and case == "bad-token":
            (f / "client.token").write_text("0" * 64 + "\n")
        if direction == "independent-server" and case == "wrong-name":
            p = f / "client.toml"
            p.write_text(p.read_text().replace("remora.test", "wrong.test"))
        config = f / "peer.json"
        config.write_text(json.dumps(cfg))
        config.chmod(0o600)
        negative = case not in ["positive", "hostname-positive"]
        lengths = Counter()
        protocol_code = None
        observed_refusal = None
        protocol_cases = [
            "status-204",
            "status-205",
            "status-206",
            "content-length",
            "content-type",
            "transfer-encoding",
            "capsule-missing",
            "capsule-oversized",
        ]
        monitor = socket.socket(
            socket.AF_INET6 if family == "ipv6" else socket.AF_INET, socket.SOCK_DGRAM
        )
        monitor.bind((target_host, client["target_port"]))
        monitor.setblocking(False)
        run = out / f"{family}-{direction}-{case}"
        run.mkdir()
        try:
            if direction == "independent-client":
                owned.append(
                    Owned(
                        [
                            str(ROOT / "target/release/remora"),
                            "server",
                            "--config",
                            str(f / "server.toml"),
                        ],
                        run / "remora",
                    )
                )
                test = Owned([str(peer), "--config", str(config)], run / "peer")
                owned.append(test)
            else:
                owned.append(Owned([str(peer), "--config", str(config)], run / "peer"))
                # Wait for the fixture's bound UDP socket: connected client
                # startup must not receive a runner-induced ICMP port refusal.
                until = time.monotonic() + 3
                while (
                    '"event":"listening"'
                    not in (run / "peer.stdout.ndjson").read_text()
                ):
                    if owned[0].exited() or time.monotonic() > until:
                        raise RuntimeError("independent server readiness")
                    time.sleep(0.005)
                remora = Owned(
                    [
                        str(ROOT / "target/release/remora"),
                        "client",
                        "--config",
                        str(f / "client.toml"),
                    ],
                    run / "remora",
                )
                owned.append(remora)
                if not negative:
                    test = Owned(
                        [
                            str(ROOT / "tools/traffic/target/release/remora-traffic"),
                            "exercise",
                            "--to",
                            client["listen"],
                            "--sessions",
                            "2",
                        ],
                        run / "exercise",
                    )
                    owned.append(test)
                else:
                    test = remora
            local = socket.socket(
                socket.AF_INET6 if family == "ipv6" else socket.AF_INET,
                socket.SOCK_DGRAM,
            )
            host, port = client["listen"].rsplit(":", 1)
            host = host.strip("[]")
            deadline = time.monotonic() + 12
            next_probe = 0
            while not test.exited():
                now = time.monotonic()
                if now > deadline:
                    raise TimeoutError("peer case deadline")
                if direction == "independent-server" and case in [
                    "status-204",
                    "status-205",
                    "status-206",
                    "content-length",
                    "content-type",
                    "transfer-encoding",
                    "capsule-missing",
                    "capsule-oversized",
                ]:
                    records = []
                    for log in [
                        run / "remora.stdout.ndjson",
                        run / "remora.stderr.ndjson",
                    ]:
                        for line in log.read_text().splitlines():
                            try:
                                records.append(json.loads(line))
                            except ValueError:
                                pass

                    def observed(value):
                        if isinstance(value, dict):
                            return (
                                (
                                    value.get("failure") == "malformed"
                                    or value.get("class") == "malformed"
                                )
                                and value.get("numeric_code", value.get("code"))
                                == (51 if case == "capsule-oversized" else 270)
                            ) or any(observed(v) for v in value.values())
                        if isinstance(value, list):
                            return any(observed(v) for v in value)
                        return False

                    if any(record.get("coverage_lost", 0) for record in records):
                        raise AssertionError("status coverage loss")
                    if any(observed(record) for record in records):
                        protocol_code = 51 if case == "capsule-oversized" else 270
                        break
                if direction == "independent-server" and negative and now >= next_probe:
                    local.sendto(b"INTEROP_PAYLOAD_SENTINEL", (host, int(port)))
                    next_probe = now + 0.1
                for _ in range(32):
                    try:
                        data, source = monitor.recvfrom(65536)
                    except BlockingIOError:
                        break
                    lengths[len(data)] += 1
                    if not negative:
                        monitor.sendto(data, source)
                time.sleep(0.001)
            # Drain while still monitoring after test completion.
            until = time.monotonic() + (
                1.1 if case in ["early-datagram", "malformed-context"] else 0.1
            )
            while time.monotonic() < until:
                try:
                    data, _ = monitor.recvfrom(65536)
                    lengths[len(data)] += 1
                except BlockingIOError:
                    time.sleep(0.001)
            for proc in owned:
                proc.stop()
            local.close()
            if negative:
                if case in protocol_cases and protocol_code is None:
                    raise AssertionError("expected protocol error code absent")
                if sum(lengths.values()) != 0:
                    raise AssertionError("unauthorized target datagram")
                if direction == "independent-client":
                    records = [
                        json.loads(l)
                        for l in (run / "peer.stdout.ndjson").read_text().splitlines()
                    ]
                    expected_event = (
                        "drop_injection"
                        if case
                        in ["early-datagram", "malformed-context", "absent-settings"]
                        else (
                            "protocol_injection"
                            if case
                            in ["capsule-request-missing", "capsule-request-oversized"]
                            else "denial"
                        )
                    )
                    if test.p.returncode != 0 or not any(
                        r.get("event") == expected_event and r.get("ok")
                        for r in records
                    ):
                        raise AssertionError("independent expected rejection/injection")
                    expected_status = {
                        "bad-token": 401,
                        "revoked-token": 401,
                        "missing-token": 401,
                        "denied-target": 403,
                        "mixed-dns": 403,
                        "malformed-path": 404,
                    }.get(case)
                    if expected_status is not None:
                        if not any(
                            r.get("event") == "denial"
                            and r.get("ok") is True
                            and r.get("case") == cfg["case"]
                            and r.get("status") == expected_status
                            for r in records
                        ):
                            raise AssertionError("expected HTTP denial status absent")
                        observed_refusal = {"http_status": expected_status}
                    server_status = []
                    for log in [
                        run / "remora.stdout.ndjson",
                        run / "remora.stderr.ndjson",
                    ]:
                        for line in log.read_text().splitlines():
                            try:
                                server_status.append(json.loads(line))
                            except ValueError:
                                pass
                    if case == "absent-settings":
                        if not any(
                            (
                                r.get("failure") == "capability"
                                and r.get("numeric_code") == 0
                            )
                            or any(
                                f.get("class") == "capability" and f.get("code") == 0
                                for f in r.get("failures", [])
                            )
                            for r in server_status
                        ):
                            raise AssertionError(
                                "missing exact peer-settings capability/0 refusal"
                            )
                        observed_refusal = {"class": "capability", "numeric_code": 0}
                    if case == "early-datagram" and not any(
                        r.get("counters", {})
                        .get("local_drops", {})
                        .get("unknown_flow", {})
                        .get("to_target", 0)
                        for r in server_status
                    ):
                        raise AssertionError("missing early flow discard")
                    if case == "malformed-context" and not any(
                        r.get("counters", {})
                        .get("local_drops", {})
                        .get("malformed", {})
                        .get("to_target", 0)
                        for r in server_status
                    ):
                        raise AssertionError("missing context discard")
                else:
                    expected_failure = {
                        "bad-token": ("authentication", 401),
                        "revoked-token": ("authentication", 401),
                        "denied-target": ("authorization", 403),
                        "wrong-name": ("trust", 0),
                        "no-datagram": ("capability", 0),
                    }.get(case)
                    if expected_failure is not None:
                        if test.p.returncode != 1:
                            raise AssertionError("Remora initial rejection must fail")
                        records = []
                        for log in [
                            run / "remora.stdout.ndjson",
                            run / "remora.stderr.ndjson",
                        ]:
                            for line in log.read_text().splitlines():
                                try:
                                    record = json.loads(line)
                                except ValueError:
                                    continue
                                if isinstance(record, dict):
                                    records.append(record)
                        failure, code = expected_failure
                        if not any(
                            r.get("event") == "failure"
                            and r.get("role") == "client"
                            and r.get("failure") == failure
                            and r.get("numeric_code") == code
                            for r in records
                        ):
                            raise AssertionError("expected Remora refusal cause absent")
                        observed_refusal = {"class": failure, "numeric_code": code}
            else:
                if test.p.returncode != 0:
                    raise AssertionError("byte/boundary exercise failed")
                if not all(lengths[size] >= 2 for size in [0, 1, 128, 512, 1200]):
                    raise AssertionError("target boundary coverage")
            token = (f / "client.token").read_text().strip()
            for log in run.glob("*.ndjson"):
                text = log.read_text()
                if token in text or "INTEROP_PAYLOAD_SENTINEL" in text:
                    raise AssertionError("diagnostic privacy")
            summary = {
                "family": family,
                "direction": direction,
                "case": case,
                "status": "pass",
                "target_datagrams": sum(lengths.values()),
                "target_length_counts": dict(lengths),
                "independent_stack": True,
                "release_qualification": False,
                "observed_protocol_code": protocol_code,
                "observed_refusal": observed_refusal,
                "target_monitor": target,
            }
            write(run / "summary.json", summary)
            return summary
        except Exception as e:
            write(
                run / "failure.json", {"status": "failed", "reason": type(e).__name__}
            )
            raise
        finally:
            monitor.close()
            for proc in owned:
                if not proc.reaped:
                    proc.stop()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--fixtures", required=True)
    p.add_argument("--peer-ref", default=PIN)
    p.add_argument(
        "--direction",
        choices=["both", "independent-client", "independent-server"],
        default="both",
    )
    p.add_argument("--peer-binary", required=True)
    p.add_argument("--output", required=True)
    a = p.parse_args()
    if a.peer_ref != PIN:
        raise ValueError("unqualified peer revision")
    for role in ["server", "client"]:
        subprocess.run(
            [
                str(ROOT / "target/release/remora"),
                "check-config",
                "--config",
                str(Path(a.fixtures) / f"{role}.toml"),
            ],
            check=True,
        )
    out = Path(a.output)
    out.mkdir(parents=True, exist_ok=False)
    peer = Path(a.peer_binary).resolve()
    build_info = subprocess.check_output(["go", "version", "-m", str(peer)], text=True)
    if (
        "go1.27.2" not in build_info
        or "github.com/quic-go/masque-go\tv0.6.0\t" not in build_info
        or "github.com/quic-go/quic-go\tv0.63.0\t" not in build_info
    ):
        raise ValueError("unqualified peer binary inputs")
    write(
        out / "manifest.json",
        {
            "peer_ref": PIN,
            "go_build_info": build_info,
            "runner_sha256": sha(ROOT / "scripts/validate/interop.py"),
            "masque_go": "v0.6.0",
            "quic_go": "v0.63.0",
            "go": subprocess.check_output(["go", "version"], text=True).strip(),
            "module_hash": sha(ROOT / "tools/interop/go.mod"),
            "sum_hash": sha(ROOT / "tools/interop/go.sum"),
            "peer_binary_sha256": sha(peer),
            "remora_binary_sha256": sha(ROOT / "target/release/remora"),
            "source_sha256": {
                str(q.relative_to(ROOT)): sha(q)
                for q in (ROOT / "tools/interop").glob("*.go")
            },
            "release_qualification": False,
        },
    )
    results = []
    directions = (
        ["independent-client", "independent-server"]
        if a.direction == "both"
        else [a.direction]
    )
    for direction in directions:
        for family in ["ipv4", "ipv6"]:
            results.append(run_case(out, peer, family, direction, "positive"))
        if direction == "independent-client":
            for case in ["hostname-positive", "mixed-dns"]:
                results.append(run_case(out, peer, "ipv6", direction, case))
        cases = ["bad-token", "revoked-token", "denied-target"]
        cases += (
            [
                "missing-token",
                "malformed-path",
                "early-datagram",
                "malformed-context",
                "absent-settings",
                "capsule-request-missing",
                "capsule-request-oversized",
            ]
            if direction == "independent-client"
            else [
                "wrong-name",
                "no-datagram",
                "status-204",
                "status-205",
                "status-206",
                "content-length",
                "content-type",
                "transfer-encoding",
                "capsule-missing",
                "capsule-oversized",
            ]
        )
        for case in cases:
            results.append(run_case(out, peer, "ipv4", direction, case))
    write(out / "summary.json", results)
    print(
        json.dumps(
            {
                "event": "interop",
                "passed_cases": len(results),
                "release_qualification": False,
            }
        )
    )


if __name__ == "__main__":
    main()
