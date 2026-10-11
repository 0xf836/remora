#!/usr/bin/env python3
"""Read-only peer observations and owned traffic; no VPN/route/key provisioning."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class QualificationFailure(Exception):
    """Fixed local reason; never contains remote output."""


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--fixture", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--seconds", type=int, default=1800)
    p.add_argument("--start-remora", action="store_true",
                   help="start fresh run-owned Remora units from prepared /opt/remora-t037 configs")
    a = p.parse_args()
    if not 5 <= a.seconds <= 1800:
        p.error("duration must be 5..1800 seconds; only 1800 qualifies duration")
    fpath = Path(a.fixture).resolve()
    if fpath.stat().st_mode & 0o077:
        p.error("private fixture must have mode 0600")
    f = json.loads(fpath.read_text())
    if f.get("schema_version") != 1 or set(f["roles"]) != {"a", "b"}:
        p.error("fixture requires schema_version 1 and exactly roles a/b")
    config = Path(f["ssh_config"]).resolve()
    if config.stat().st_mode & 0o077:
        p.error("private SSH config must have mode 0600")
    for r in f["roles"].values():
        for key in ("ssh_host", "wg_interface"):
            if not re.fullmatch(r"[a-zA-Z0-9_.-]{1,64}", r[key]):
                p.error("invalid fixture identifier")
        if len(r["units"]) != 2 or any(not re.fullmatch(r"remora-t037-[a-z0-9-]+", u) for u in r["units"]):
            p.error("two dedicated remora-t037-* units required per role")
    out = Path(a.output).resolve()
    out.mkdir(parents=True, exist_ok=False)
    owned = []
    handles = []
    observations = []
    started_units = []
    run = secrets.token_hex(16)
    result = {"schema_version": 1, "status": "invalid", "seconds": a.seconds,
              "duration_qualified": False, "run_id": run,
              "traffic_source_sha256": sha(ROOT / "scripts/validate/wireguard_traffic.py"),
              "runner_sha256": sha(__file__), "environment": f["public_environment"]}

    def ssh(role, command, **kwargs):
        return subprocess.run(["ssh", "-F", str(config), "-o", "StrictHostKeyChecking=yes",
                               f["roles"][role]["ssh_host"], command], stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, timeout=15, check=True, **kwargs).stdout

    def observe(role):
        r = f["roles"][role]
        interface = shlex.quote(r["wg_interface"])
        handshake = ssh(role, f"sudo -n wg show {interface} latest-handshakes")
        transfer = ssh(role, f"sudo -n wg show {interface} transfer")
        hs = [l.split() for l in handshake.splitlines()]
        tx = [l.split() for l in transfer.splitlines()]
        if len(hs) != 1 or len(tx) != 1 or hs[0][0] != tx[0][0]:
            raise QualificationFailure("dedicated fixture requires exactly one matching peer")
        life = {}
        for unit in r["units"]:
            fields = ssh(role, "systemctl show " + shlex.quote(unit) +
                         " -p ActiveState -p MainPID -p NRestarts -p ExecMainStartTimestampMonotonic -p InvocationID")
            state = dict(l.split("=", 1) for l in fields.splitlines() if "=" in l)
            if state.get("ActiveState") != "active" or int(state.get("MainPID", "0")) <= 0:
                raise QualificationFailure("Remora role is not active")
            life[unit] = state
        return {"role": role, "timestamp": time.time(), "handshake": int(hs[0][1]),
                "received_bytes": int(tx[0][1]), "sent_bytes": int(tx[0][2]), "units": life}

    try:
        for role in ("a", "b"):
            before = time.time()
            remote = float(ssh(role, "python3 -c 'import time; print(time.time())'"))
            after = time.time()
            if not before - 1 <= remote <= after + 1:
                raise QualificationFailure("guest clock offset exceeds one second")
            # Verify the installed test tool rather than uploading or provisioning peers.
            remote_sha = ssh(role, "sha256sum /opt/remora-t037/wireguard_traffic.py").split()[0]
            if remote_sha != result["traffic_source_sha256"]:
                raise QualificationFailure("installed traffic source hash mismatch")
            if ssh(role, "sha256sum /opt/remora-t037/remora").split()[0] != f["public_environment"]["remora_binary_sha256"]:
                raise QualificationFailure("installed Remora binary hash mismatch")
            r = f["roles"][role]
            route = json.loads(ssh(role, "ip -j route get " + shlex.quote(r["wg_peer"].rsplit(":", 1)[0]) +
                                   " from " + shlex.quote(r["wg_bind"].rsplit(":", 1)[0])))
            if len(route) != 1 or route[0].get("dev") != r["wg_interface"]:
                raise QualificationFailure("inner application route bypasses WireGuard")
            link = json.loads(ssh(role, "ip -j link show dev " + shlex.quote(r["wg_interface"])))
            if len(link) != 1 or link[0].get("mtu") != f["public_environment"]["wireguard_mtu"]:
                raise QualificationFailure("observed WireGuard MTU does not match fixture")
        if a.start_remora:
            # Server first, then clients immediately before warmup: Phase 3 has
            # no idle recovery/keepalive yet. Never restart a prepared live unit.
            for role in ("b", "a"):
                mode = "client" if role == "a" else "server"
                for channel, unit in zip(("wg", "generic"), f["roles"][role]["units"]):
                    if unit != f"remora-t037-{mode}-{channel}":
                        raise QualificationFailure("start mode requires dedicated role unit names")
                    state = ssh(role, "systemctl show " + shlex.quote(unit) + " -p LoadState --value").strip()
                    if state != "not-found":
                        raise QualificationFailure("refusing to replace an existing Remora unit")
                    cmd = ["sudo", "-n", "systemd-run", "--quiet", "--collect", "--unit", unit,
                           "--uid", "lab", "--property", "Restart=no", "--property", "TimeoutStopSec=5s",
                           "--property", f"RuntimeMaxSec={a.seconds + 100}", "/opt/remora-t037/remora", mode,
                           "--config", f"/opt/remora-t037/{channel}/{mode}.toml"]
                    ssh(role, shlex.join(cmd))
                    started_units.append((role, unit))
                    if role == "b":
                        ssh(role, "python3 -c 'import time; time.sleep(0.25)'")
        for role in ("a", "b"):
            observations.append(observe(role))
        baseline = {v["role"]: v for v in observations}
        start_at = time.time() + 20
        result["start_at"] = start_at
        for role in ("a", "b"):
            r = f["roles"][role]
            cmd = ["python3", "/opt/remora-t037/wireguard_traffic.py", "--role", role,
                   "--wg-bind", r["wg_bind"], "--wg-peer", r["wg_peer"],
                   "--generic-bind", r["generic_bind"], "--run-id", run,
                   "--start-at", str(start_at), "--seconds", str(a.seconds)]
            if r.get("generic_peer"):
                cmd += ["--generic-peer", r["generic_peer"]]
            stdout = (out / f"{role}-traffic.ndjson").open("xb")
            stderr = (out / f"{role}-traffic.stderr").open("xb")
            handles += [stdout, stderr]
            remote_cmd = ["sudo", "-n", "systemd-run", "--quiet", "--collect", "--wait", "--pipe",
                          "--unit", f"remora-t037-traffic-{run}-{role}", "--uid", "lab",
                          "--property", f"RuntimeMaxSec={a.seconds + 65}", *cmd]
            proc = subprocess.Popen(["ssh", "-F", str(config), "-o", "StrictHostKeyChecking=yes",
                                     r["ssh_host"], shlex.join(remote_cmd)], stdin=subprocess.DEVNULL,
                                    stdout=stdout, stderr=stderr, start_new_session=True)
            owned.append(proc)
        deadline = time.monotonic() + 20 + a.seconds + 15
        next_observe = time.monotonic() + 25
        while any(proc.poll() is None for proc in owned):
            now = time.monotonic()
            if now > deadline:
                raise TimeoutError("traffic deadline exceeded")
            if any(proc.returncode not in (None, 0) for proc in owned):
                raise QualificationFailure("traffic process failed; preserve raw evidence")
            if now >= next_observe:
                for role in ("a", "b"):
                    v = observe(role)
                    if v["units"] != baseline[role]["units"]:
                        raise QualificationFailure("Remora role restarted during qualification")
                    observations.append(v)
                (out / "observations.json").write_text(json.dumps(observations, indent=2) + "\n")
                print(json.dumps({"event": "qualification_progress", "elapsed_seconds":
                                  round(time.time() - start_at, 1)}), flush=True)
                next_observe = now + 30
            time.sleep(0.2)
        for h in handles:
            h.flush()
        traffic = []
        for role in ("a", "b"):
            values = [json.loads(l) for l in (out / f"{role}-traffic.ndjson").read_text().splitlines()]
            v = next(v for v in reversed(values) if v.get("event") == "result")
            if v["status"] != "pass":
                raise QualificationFailure("traffic integrity/delivery check failed")
            traffic.append(v)
            final = observe(role)
            if final["units"] != baseline[role]["units"]:
                raise QualificationFailure("Remora role restarted")
            if final["handshake"] < start_at - 30 or time.time() - final["handshake"] > 180:
                raise QualificationFailure("WireGuard handshake is not fresh")
            if any(final[k] <= baseline[role][k] for k in ("received_bytes", "sent_bytes")):
                raise QualificationFailure("WireGuard transfer counters did not grow both ways")
            observations.append(final)
            for unit in f["roles"][role]["units"]:
                invocation = baseline[role]["units"][unit]["InvocationID"]
                if not re.fullmatch(r"[a-f0-9]{32}", invocation):
                    raise QualificationFailure("invalid systemd invocation identity")
                raw = ssh(role, "sudo -n journalctl -u " + shlex.quote(unit) +
                          " _SYSTEMD_INVOCATION_ID=" + invocation + " --output=cat --no-pager")
                statuses = []
                for line in raw.splitlines():
                    try:
                        status = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if status.get("event") == "snapshot":
                        statuses.append(status)
                if not statuses:
                    raise QualificationFailure("missing structured Remora status")
                counters = statuses[-1]["counters"]
                if not all(counters["app_ingress_by_direction"][d] > 0 or
                           counters["app_egress_by_direction"][d] > 0 for d in ("to_client", "to_target")):
                    raise QualificationFailure("missing bidirectional Remora evidence")
                (out / f"{role}-{unit}-status.ndjson").write_text(
                    "".join(json.dumps(v, sort_keys=True) + "\n" for v in statuses))
        result.update(status="pass", duration_qualified=a.seconds == 1800, traffic=traffic)
    except Exception as error:
        result["failure_class"] = type(error).__name__
        if isinstance(error, QualificationFailure):
            result["failure_reason"] = str(error)
        # Fixed errors only: remote command output may contain private fixture data.
        print(json.dumps({"event": "qualification_failed", "failure_class": type(error).__name__}), flush=True)
    finally:
        # Only the random, run-owned traffic units are stopped; VPN and Remora
        # units remain operator-owned. RuntimeMaxSec bounds remote work even
        # when SSH or the controller disappears.
        for role in ("a", "b"):
            try:
                ssh(role, "sudo -n systemctl stop " + shlex.quote(f"remora-t037-traffic-{run}-{role}"))
            except subprocess.SubprocessError:
                pass
        for proc in owned:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)
        for role, unit in reversed(started_units):
            try:
                ssh(role, "sudo -n systemctl stop " + shlex.quote(unit))
            except subprocess.SubprocessError:
                pass
        for h in handles:
            h.close()
        (out / "observations.json").write_text(json.dumps(observations, indent=2) + "\n")
        (out / "summary.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"event": "wireguard_qualification", "status": result["status"],
                      "duration_qualified": result["duration_qualified"]}), flush=True)
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
