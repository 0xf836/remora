#!/usr/bin/env python3
import argparse, json, subprocess, tempfile, time, tomllib
from pathlib import Path
from benchmark import Owned, ROOT, wait_udp_bound

p = argparse.ArgumentParser()
p.add_argument("--fixtures", required=True)
p.add_argument(
    "--scenario",
    choices=["smoke", "security", "shutdown", "concurrency", "recovery"],
    default="smoke",
)
a = p.parse_args()
f = Path(a.fixtures).resolve()
client = tomllib.loads((f / "client.toml").read_text())["client"]
server = tomllib.loads((f / "server.toml").read_text())["server"]
if a.scenario in ["concurrency", "recovery"]:
    raise SystemExit(
        "full concurrency/recovery qualification belongs to later phases; no pass recorded"
    )
if a.scenario == "security":
    subprocess.run(
        ["cargo", "test", "--locked", "--test", "e2e", "security"], cwd=ROOT, check=True
    )
else:
    target = client["target_host"]
    target = f"[{target}]" if ":" in target else target
    tool = str(ROOT / "tools/traffic/target/release/remora-traffic")
    binary = str(ROOT / "target/release/remora")
    owned = []
    with tempfile.TemporaryDirectory(prefix="remora-e2e-") as directory:
        root = Path(directory)
        try:
            deadline = int(time.time() * 1000) + 60000
            owned.append(
                Owned(
                    [
                        tool,
                        "responder",
                        "--listen",
                        f"{target}:{client['target_port']}",
                        "--start-ms",
                        str(deadline),
                        "--seconds",
                        "5",
                    ],
                    root / "responder",
                )
            )
            for role in ["server", "client"]:
                if role == "client":
                    wait_udp_bound(owned[1], server["listen"][0])
                owned.append(
                    Owned(
                        [binary, role, "--config", str(f / f"{role}.toml")], root / role
                    )
                )
            result = subprocess.run(
                [tool, "exercise", "--to", client["listen"], "--sessions", "2"],
                capture_output=True,
                text=True,
                timeout=20,
            )
            if result.returncode != 0:
                raise RuntimeError("forwarding exercise failed")
            print(result.stdout.strip())
        finally:
            for process in owned:
                process.stop()
        if a.scenario == "shutdown":
            assert all(
                proc.p.returncode == 0 for proc in owned[1:]
            ), "graceful role shutdown failed"
            print(
                json.dumps(
                    {"event": "shutdown", "roles_released": True, "seconds_bound": 5}
                )
            )
