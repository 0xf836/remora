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
    def test_socket_override_cannot_silently_mismatch_baseline_or_accept_invalid_size(self):
        class Network:
            def ensure_alive(self):
                pass

        for mode, value in [('baseline', 1048576), ('tunnel', True), ('tunnel', 0), ('tunnel', 2**31)]:
            with self.subTest(mode=mode, value=value), tempfile.TemporaryDirectory() as temp, patch.object(reference, 'check_budget'):
                out = Path(temp) / 'must-not-be-created'
                profile = dict(self.profile, outer_socket_buffer_bytes=value)
                with self.assertRaises(ValueError):
                    reference._run_once(Network(), out, profile, mode, 32, 1200, 'simultaneous', 91136, 3, 60, {})
                self.assertFalse(out.exists())

    def test_socket_request_metadata_tracks_changed_shared_defaults(self):
        source = """settings!(IoSettings {
            outer_socket_buffer_bytes: usize = 2_000_000,
            relay_socket_buffer_bytes: usize = 100_000,
        });"""
        self.assertEqual(
            reference.socket_buffer_requests(source),
            {"outer_bytes": 2000000, "application_bytes": 100000},
        )

    def test_socket_request_metadata_rejects_unrecognized_or_invalid_defaults(self):
        source = """settings!(IoSettings {
            outer_socket_buffer_bytes: usize = 1048576,
            relay_socket_buffer_bytes: usize = 131072,
        });"""
        for invalid in [
            source.replace("IoSettings", "OtherSettings"),
            source + source,
            source.replace("1048576", "1024 * 1024"),
            source.replace("1048576", "0"),
            source.replace("1048576", "2147483648"),
            source.replace("relay_socket_buffer_bytes: usize = 131072,", ""),
        ]:
            with self.subTest(source=invalid), self.assertRaises(ValueError):
                reference.socket_buffer_requests(invalid)

    def test_manifest_reports_shared_runtime_default_socket_requests(self):
        class ManifestCaptured(Exception):
            pass

        class Network:
            def snapshot(self):
                return {}

        captured = {}

        def record(path, value):
            if path.name == "manifest.json":
                captured.update(value)
                raise ManifestCaptured

        with tempfile.TemporaryDirectory() as temp, patch.object(
            reference.platform, "machine", return_value="x86_64"
        ), patch.object(
            reference.os, "sched_getaffinity", return_value=set(range(10))
        ), patch.object(reference, "sha", return_value="test-hash"), patch.object(
            reference, "input_hashes", return_value={}
        ), patch.object(
            reference.subprocess, "check_output", return_value="test-version"
        ), patch.object(reference, "write", side_effect=record):
            with self.assertRaises(ManifestCaptured):
                reference.execute(Network(), Path(temp), self.profile)

        self.assertEqual(
            captured["requested_socket_buffers"],
            {"outer_bytes": 1048576, "application_bytes": 131072},
        )

    def test_collector_placement_restored_after_success_and_failure(self):
        before = os.sched_getaffinity(0)
        cpus = sorted(before)
        if len(cpus) < 3:
            self.skipTest("requires three available CPUs")
        placement = {"client": cpus[:1], "server": cpus[1:2], "tools": cpus[2:]}
        for fail in [False, True]:
            with self.subTest(fail=fail):
                try:
                    with reference.collector_affinity(placement) as evidence:
                        self.assertEqual(os.sched_getaffinity(0), set(cpus[2:]))
                        self.assertEqual(evidence["before"], cpus)
                        self.assertEqual(evidence["actual"], cpus[2:])
                        if fail:
                            raise RuntimeError("injected run failure")
                except RuntimeError as error:
                    self.assertTrue(fail)
                    self.assertEqual(str(error), "injected run failure")
                self.assertEqual(os.sched_getaffinity(0), before)

    def test_collector_overlap_or_unavailable_cpu_rejected_before_mutation(self):
        for placement in [
            {"client": [0], "server": [1], "tools": [0, 2]},
            {"client": [0], "server": [1], "tools": [1, 2]},
            {"client": [0], "server": [1], "tools": []},
            {"client": [0], "server": [1], "tools": [3]},
        ]:
            with self.subTest(placement=placement), patch.object(
                reference.os, "sched_getaffinity", return_value={0, 1, 2}
            ), patch.object(reference.os, "sched_setaffinity") as mutate:
                with self.assertRaises(ValueError):
                    with reference.collector_affinity(placement):
                        self.fail("invalid placement admitted")
                mutate.assert_not_called()

    def setUp(self):
        self.profile = tomllib.loads(
            (ROOT / "benchmarks/reference-x86_64.toml").read_text()
        )

    def test_arm64_profile_uses_identical_matrix_and_requires_native_execution(self):
        arm = tomllib.loads((ROOT / "benchmarks/reference-arm64.toml").read_text())
        self.assertEqual(reference.plan(arm), reference.plan(self.profile))
        with patch.object(reference.platform, "machine", return_value="x86_64"):
            with self.assertRaisesRegex(RuntimeError, "native architecture/profile mismatch"):
                reference.execute(None, Path("/tmp/unused"), arm)
        unknown = dict(arm, architecture="emulated-arm64")
        with self.assertRaises(ValueError): reference.plan(unknown)

    def test_unrecorded_or_changed_pacing_is_rejected(self):
        for quantum in [None, 0, 50, 1000]:
            profile = dict(self.profile, generator_pacing_quantum_us=quantum)
            with self.assertRaises(ValueError): reference.plan(profile)

    def test_complete_unique_matrix_and_randomized_reproducible_order(self):
        method = reference.plan(self.profile)
        self.assertEqual(method, reference.plan(self.profile))
        self.assertEqual(len(method["searches"]), 18)
        pairs = method["pairs"]
        self.assertEqual(len(pairs), 75)
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
            75,
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
            search_policy="linear",
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

    def test_proc_teardown_before_waitable_exit_has_bounded_post_interval_wait(self):
        from types import SimpleNamespace
        owned = [SimpleNamespace(p=object(), exited=lambda: False) for _ in range(3)]
        owned[2].exited = unittest.mock.Mock(side_effect=[False, False, False, True])
        with patch.object(reference, "measured_resource", side_effect=[{}, {}, FileNotFoundError()]):
            sample = reference.resource_sample(["server", "client", "receiver"], owned, 2001, 2000)
        self.assertEqual(sample["completed_traffic"], ["receiver"])
        self.assertNotIn("receiver", sample["roles"])

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
                # Popen may return before exec populates /proc/cmdline.
                # Wait for the real holder, not a scheduler-dependent instant.
                deadline = time.monotonic() + 3
                while b"infinity" not in Path(f"/proc/{holder.p.pid}/cmdline").read_bytes():
                    self.assertFalse(holder.exited(), "holder exited before exec readiness")
                    self.assertLess(time.monotonic(), deadline, "holder exec readiness timeout")
                    time.sleep(0.001)
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


class BoundedBudget(unittest.TestCase):
    def test_planned_intervals_fit_and_deadline_cannot_pass_incomplete_run(self):
        profile = tomllib.loads((ROOT / "benchmarks/reference-x86_64.toml").read_text())
        method = reference.plan(profile)
        self.assertEqual(method["planned_interval_seconds"], 4965)
        self.assertLess(method["planned_interval_seconds"], 5400 - 30)
        self.assertEqual(method["wall_budget_seconds"], 5400)
        self.assertEqual(sum(p["phase"] == "capacity" for p in method["pairs"]), 18)
        class Network: reference_deadline = 20
        with patch.object(reference.time, "monotonic", return_value=20):
            with self.assertRaises(TimeoutError): reference.check_budget(Network())
        with patch.object(reference.time, "monotonic", return_value=19): reference.check_budget(Network())

    def test_capacity_extension_is_matched_and_limited_to_three_pairs(self):
        profile = tomllib.loads((ROOT / "benchmarks/reference-x86_64.toml").read_text())
        method = reference.plan(profile)
        extended = [key for key in method["pairs"] if key["phase"] == "capacity"
                    and key["sessions"] == 32 and key["direction"] == "simultaneous"]
        self.assertEqual(len(extended), 3)
        for key in method["pairs"]:
            expected = 60 if key in extended or key["phase"] == "resource" else 15
            self.assertEqual(reference.paired_seconds(profile, key), expected)
        historical = dict(profile)
        del historical["capacity_32_simultaneous_seconds"]
        self.assertEqual(reference.plan(historical)["planned_interval_seconds"], 4695)
        self.assertTrue(all(reference.paired_seconds(historical, key) == 15 for key in extended))
        for invalid in [0, 30, 61]:
            with self.assertRaisesRegex(ValueError, "unmatched full reference"):
                reference.plan(dict(profile, capacity_32_simultaneous_seconds=invalid))
    def test_bisection_uses_both_valid_and_rejected_brackets(self):
        self.assertEqual(reference.bisect_rate(0, None, 0), 125)
        self.assertEqual(reference.bisect_rate(125, None, 1), 250)
        self.assertEqual(reference.bisect_rate(500, 1000, 2), 750)
        self.assertEqual(reference.bisect_rate(500, 750, 3), 625)

    def test_churn_budget_exhaustion_cleans_owned_process_group(self):
        class Network: reference_deadline = 20
        class Child:
            stopped = False
            def __init__(self, *args): pass
            def exited(self): return False
            def stop(self): self.stopped = True
        child = Child()
        with patch.object(reference, "Owned", return_value=child), patch.object(reference.time, "monotonic", return_value=20):
            with self.assertRaises(TimeoutError): reference.run_churn(Network(), Path("/tmp/unused"))
        self.assertTrue(child.stopped)

    def test_live_progress_updates_without_overwriting_measurement_artifacts(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            reference.write_progress(out, {"stage": "calibration"})
            reference.write_progress(out, {"stage": "paired"})
            import json
            self.assertEqual(json.loads((out / "progress.json").read_text())["stage"], "paired")
            self.assertFalse((out / "progress.tmp").exists())

    def test_calibration_failure_names_exact_invalid_evidence(self):
        row = dict(generator_errors=174, integrity_errors=0, generator_cpu_max=.245,
                   clock_drift_ms=.000268, coverage_lost=0, counter_overflow=False)
        self.assertEqual(reference.calibration_invalid_reasons(row), ["generator_errors=174"])
        row["generator_errors"] = 0
        self.assertEqual(reference.calibration_invalid_reasons(row), [])
        row["clock_drift_ms"] = float("nan")
        self.assertEqual(reference.calibration_invalid_reasons(row), ["clock_drift_ms=nan"])

    def test_completion_guard_cannot_be_changed_or_omitted_from_new_method(self):
        profile = tomllib.loads((ROOT / "benchmarks/reference-x86_64.toml").read_text())
        for guard in [None, 0, 1, 21]:
            with self.assertRaises(ValueError): reference.plan(dict(profile, generator_completion_guard_ms=guard))

    def test_owned_setup_loopback_is_enabled_only_after_fresh_namespace_proof(self):
        import network
        from types import SimpleNamespace
        fds = [os.open(f"/proc/self/ns/{kind}", os.O_RDONLY) for kind in ["net", "user"]]
        try:
            with tempfile.TemporaryDirectory() as temp, patch.object(network.os, "stat", return_value=SimpleNamespace(st_ino=-1)), patch.object(network, "run", return_value='[{"ifname":"lo"}]') as command, patch.object(network, "namespace_holder", side_effect=RuntimeError("stop after loopback initialization")):
                with self.assertRaisesRegex(RuntimeError, "stop after loopback"):
                    network.Network(Path(temp), "reference", *fds)
                self.assertEqual(command.call_args_list[0].args, ("ip", "-j", "link", "show"))
                self.assertEqual(command.call_args_list[1].args, ("ip", "link", "set", "lo", "up"))
            with tempfile.TemporaryDirectory() as temp, patch.object(network.os, "stat", return_value=SimpleNamespace(st_ino=-1)), patch.object(network, "run", return_value='[{"ifname":"lo"},{"ifname":"eth0"}]') as command:
                with self.assertRaisesRegex(RuntimeError, "not fresh"):
                    network.Network(Path(temp), "reference", *fds)
                self.assertEqual(command.call_count, 1)
        finally:
            for fd in fds: os.close(fd)

    def test_ordinary_suppression_does_not_manufacture_status_coverage_loss(self):
        self.assertEqual(reference.diagnostic_coverage_lost(dict(coverage_lost=0,status_write_failures=0,diagnostics_dropped=10144)), 0)
        self.assertEqual(reference.diagnostic_coverage_lost(dict(coverage_lost=1,status_write_failures=2,diagnostics_dropped=10144)), 3)
