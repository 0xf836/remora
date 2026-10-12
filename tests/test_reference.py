import copy
import importlib.util
from pathlib import Path
import sys
import os
import tempfile
import time
from unittest.mock import patch
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/validate"))
import reference
from benchmark import Owned


class ReferenceMethod(unittest.TestCase):
    def setUp(self):
        self.profile = tomllib.loads(
            (ROOT / "benchmarks/reference-x86_64.toml").read_text()
        )

    def test_complete_unique_matrix_and_randomized_reproducible_order(self):
        method = reference.plan(self.profile)
        self.assertEqual(method, reference.plan(self.profile))
        self.assertEqual(len(method["searches"]), 18)
        pairs = method["pairs"]
        self.assertEqual(len(pairs), 185)
        self.assertEqual(
            len(
                {
                    (
                        p["pair"],
                        p["phase"],
                        p["sessions"],
                        p["payload_bytes"],
                        p["direction"],
                    )
                    for p in pairs
                }
            ),
            185,
        )
        self.assertEqual({p["baseline_first"] for p in pairs}, {True, False})
        changed = copy.deepcopy(self.profile)
        changed["pair_order_seed"] += 1
        self.assertNotEqual(method["pairs"], reference.plan(changed)["pairs"])
        self.assertFalse(method["release_acceptance_claim"])

    def test_repeat_spread_keeps_failed_row_instead_of_averaging_it_away(self):
        paired = []
        for index, received in enumerate([1000, 500]):
            row = {
                "offered": [1000, 0],
                "unique": [received, 0],
                "rtt_p95_ms": 20 + index * 10,
                "sample_count": 6000,
                "resources": [
                    {
                        "average_cpu_cores": 1,
                        "owner_average_cpu_cores": 0.8,
                        "peak_rss_bytes": 1000000,
                    }
                ]
                * 2,
            }
            paired.append(
                {
                    "phase": "capacity",
                    "sessions": 1,
                    "payload_bytes": 1200,
                    "direction": "forward",
                    "pair": index,
                    "measurement_seconds": 300,
                    "baseline": row,
                    "tunnel": row,
                }
            )
        report = reference.spread_report(paired)
        metric = report[0]["metrics"]["forward_delivered_mbit_s"]
        self.assertEqual(metric["values"], [0.032, 0.016])
        self.assertEqual(metric["spread"], 0.016)
        self.assertEqual(metric["sample_count"], 2)

    def test_unchanged_namespace_descriptors_cannot_authorize_host_mutation(self):
        import network

        fds = [os.open(f"/proc/self/ns/{k}", os.O_RDONLY) for k in ["net", "user"]]
        try:
            with tempfile.TemporaryDirectory() as temp, patch.object(
                network,
                "run",
                side_effect=AssertionError("no capability command allowed"),
            ):
                with self.assertRaises(RuntimeError):
                    network.Network(Path(temp), "reference", *fds)
        finally:
            for fd in fds:
                os.close(fd)

    def test_contract_units_coverage_and_sampling_cannot_be_silently_relaxed(self):
        changes = dict(
            topology="direct",
            link_mbit_s_each_direction=100,
            reference_rtt_ms=100,
            sessions=[1],
            payload_bytes=[1200],
            directions=["forward"],
            paired_repetitions=4,
            warmup_seconds=1,
            measurement_seconds=3,
            search_step_percent=10,
            search_max_steps=10,
            sustainable_loss_percent_max=1,
            subcapacity_fraction=0.7,
            resource_mbit_s_each_direction=50,
            churn_cycles=10,
            generator_cpu_core_max_exclusive=1.0,
            diagnostics="error",
            role_cores=1,
            io="invalid",
            cc="bbr",
            gso_requested="auto",
            gro_requested="auto",
        )
        for key, value in changes.items():
            with self.subTest(key=key):
                p = copy.deepcopy(self.profile)
                p[key] = value
                with self.assertRaises(ValueError):
                    reference.plan(p)

    def test_staggered_traffic_completion_does_not_fabricate_zero_resource_samples(
        self,
    ):
        with tempfile.TemporaryDirectory() as temp:
            done = Owned([sys.executable, "-c", "pass"], Path(temp) / "done")
            live = Owned(
                [sys.executable, "-c", "import time; time.sleep(2)"],
                Path(temp) / "live",
            )
            try:
                deadline = time.monotonic() + 1
                while not done.exited():
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.005)
                self.assertFalse(live.exited())
                sample = reference.resource_sample(
                    ["server", "client", "receiver", "generator-0"],
                    [live, live, done, live],
                    2001,
                    2000,
                )
                self.assertEqual(sample["completed_traffic"], ["receiver"])
                self.assertNotIn("receiver", sample["roles"])
                self.assertGreater(sample["roles"]["generator-0"]["rss_bytes"], 0)
                with self.assertRaisesRegex(RuntimeError, "measurement"):
                    reference.resource_sample(
                        ["server", "client", "receiver"], [live, live, done], 1999, 2000
                    )
            finally:
                done.stop()
                live.stop()

    def test_sampler_handles_exit_race_but_does_not_hide_missing_live_proc_evidence(
        self,
    ):
        from types import SimpleNamespace

        roles = ["server", "client", "receiver"]
        owned = [SimpleNamespace(p=object()) for _ in roles]
        for proc in owned:
            proc.exited = lambda: False
        with patch.object(
            reference, "measured_resource", return_value={"rss_bytes": 42}
        ) as sample:
            # Child was alive at the check, then /proc vanished during sampling.
            owned[2].exited = unittest.mock.Mock(side_effect=[False, True])
            sample.side_effect = [
                {"rss_bytes": 42},
                {"rss_bytes": 42},
                FileNotFoundError(),
            ]
            row = reference.resource_sample(roles, owned, 2001, 2000)
            self.assertEqual(row["completed_traffic"], ["receiver"])
            self.assertNotIn("receiver", row["roles"])
        owned[2].exited = lambda: False
        with patch.object(
            reference, "measured_resource", side_effect=FileNotFoundError()
        ):
            with self.assertRaises(FileNotFoundError):
                reference.resource_sample(roles, owned, 2001, 2000)
        owned[0].exited = lambda: True
        with self.assertRaisesRegex(RuntimeError, "measurement"):
            reference.resource_sample(roles, owned, 2001, 2000)

    def test_namespace_holder_lives_until_owned_teardown_and_exit_is_detected(self):
        import network

        with tempfile.TemporaryDirectory() as temp:
            # Exercise the exact holder process without requiring namespace
            # privileges here; native smoke separately tests unshare/nsenter.
            with patch.object(
                network, "Owned", side_effect=lambda cmd, path: Owned(cmd[3:], path)
            ):
                holder = network.namespace_holder(Path(temp) / "namespace")
            owned_network = network.Network.__new__(network.Network)
            owned_network.children = [holder]
            try:
                # A virtual clock jump beyond the old lifetime has no effect
                # on a holder which has no expiry. This is not a 37-hour soak.
                with patch.object(
                    network.time, "monotonic", return_value=time.monotonic() + 133200
                ):
                    owned_network.ensure_alive()
                self.assertIn(
                    b"infinity", Path(f"/proc/{holder.p.pid}/cmdline").read_bytes()
                )
                owned_network.close()
                self.assertTrue(holder.reaped)
            finally:
                owned_network.close()

            dead = Owned([sys.executable, "-c", "pass"], Path(temp) / "dead")
            owned_network.children = [dead]
            try:
                deadline = time.monotonic() + 1
                while not dead.exited():
                    self.assertLess(time.monotonic(), deadline)
                    time.sleep(0.005)
                with self.assertRaisesRegex(RuntimeError, "holder exited"):
                    owned_network.ensure_alive()
            finally:
                owned_network.close()


if __name__ == "__main__":
    unittest.main()
