"""Safety regressions for the operator tool; no libvirt, sudo or networking."""
import importlib.util
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location("libvirt_lab", Path(__file__).resolve().parents[1] /
                                               "scripts/validate/libvirt_lab.py")
lab = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lab)


class SafetyTests(unittest.TestCase):
    def test_destroyed_manifest_cannot_reclaim_names_reused_by_later_run(self):
        instance = object.__new__(lab.Lab)
        instance.m = {"phase": "destroyed"}
        instance.virsh = lambda *a, **kw: self.fail("old manifest must not query or mutate new resources")
        with mock.patch("builtins.print"):
            instance.destroy()
    def test_subnet_collision_even_with_host_route(self):
        for existing in ("192.168.253.0/24", "192.168.0.0/16", "192.168.253.20/32"):
            with self.assertRaises(lab.LabError):
                lab.check_subnet("192.168.253.0/24", [existing])
        self.assertEqual(str(lab.check_subnet("192.168.253.0/24", ["192.168.100.0/24"])),
                         "192.168.253.0/24")

    def test_reject_public_wrong_size_or_noncanonical_network(self):
        for cidr in ("8.8.8.0/24", "192.168.253.1/24", "192.168.0.0/16", "fd00::/64"):
            with self.assertRaises((lab.LabError, ValueError)):
                lab.check_subnet(cidr, [])

    def test_identity_mismatch_refuses_takeover(self):
        with self.assertRaises(lab.LabError):
            lab.check_identity("other-uuid", "owned-uuid", "test domain")
        lab.check_identity(" owned-uuid\n", "owned-uuid", "test domain")

    def test_public_state_and_symlinks_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            state = root / "state"
            state.mkdir(mode=0o755)
            with self.assertRaises(lab.LabError):
                lab.validate_state(state)
            state.chmod(0o700)
            (state / "unsafe").symlink_to(root / "outside")
            with self.assertRaises(lab.LabError):
                lab.validate_state(state)
            (state / "unsafe").unlink()
            (root / "alias").symlink_to(state)
            with self.assertRaises(lab.LabError):
                lab.validate_state(root / "alias")
            lab.validate_state(state)

    def test_repository_cannot_contain_private_state(self):
        with self.assertRaises(lab.LabError):
            lab.validate_state(lab.ROOT / "private-test-state")
        self.assertFalse((lab.ROOT / "private-test-state").exists())

    def test_destroy_checks_all_ownership_before_any_mutation(self):
        instance = object.__new__(lab.Lab)
        instance.m = {"domains": {"a": {"name": "a", "uuid": "u1"},
                                   "b": {"name": "b", "uuid": "u2"}}}
        instance.exists = lambda *args: True
        def check(kind, name, identity):
            if name == "b":
                raise lab.LabError("replacement domain")
        instance.ensure_owned = check
        mutations = []
        instance.stop = lambda: mutations.append("stop")
        instance.virsh = lambda *args: mutations.append(args)
        with self.assertRaises(lab.LabError):
            instance.destroy()
        self.assertEqual(mutations, [])

    def test_persistent_wg_script_uses_key_file_and_blocks_bypass(self):
        text = lab.Lab.guest_wg("b", {"a": "public-a", "b": "public-b"})
        self.assertIn("private-key /opt/remora-t037/wg.key", text)
        self.assertIn('iifname != "lo" udp dport { 51820, 41020 }', text)
        self.assertIn('test "$(wg show wg-remora public-key)" = "public-b"', text)
        self.assertNotIn("flush ruleset", text)
        a = lab.Lab.guest_wg("a", {"a": "public-a", "b": "public-b"})
        self.assertIn("endpoint 127.0.0.1:41000", a)
        self.assertNotIn("nft", a)

    def test_destroy_refuses_shared_volume_or_network(self):
        for shared in ("volume", "network"):
            instance = object.__new__(lab.Lab)
            instance.m = {"name": "remora-test", "network_uuid": "net-uuid", "pool": "default",
                          "domains": {"a": {"name": "own-a", "uuid": "u1"}},
                          "volumes": {"owned.qcow2": "/pool/owned.qcow2"}}
            instance.exists = lambda *args: True
            instance.ensure_owned = lambda *args: None
            instance.volume_names = lambda: ["owned.qcow2"]
            def query(*args):
                if args[0] == "vol-key":
                    return "/pool/owned.qcow2"
                if args[0] == "list":
                    return "own-a\nforeign"
                self.fail("unexpected mutation before safety preflight")
            instance.virsh = query
            instance.xml = lambda *args: ET.fromstring(
                '<domain><devices><disk><source file="/pool/owned.qcow2"/></disk></devices></domain>'
                if shared == "volume" else '<domain><devices><interface><source network="remora-test"/>'
                                          '</interface></devices></domain>')
            instance.domain_xmls = lambda name: [instance.xml("domain", name)]
            instance.stop = lambda: self.fail("shared resource must never be stopped")
            with self.assertRaises(lab.LabError):
                instance.destroy()

    def test_stop_disables_own_network_and_refuses_shared_network(self):
        for shared in (False, True):
            instance = object.__new__(lab.Lab)
            instance.m = {"name": "remora-test", "network_uuid": "net", "phase": "configured",
                          "domains": {"a": {"name": "own-a", "uuid": "u1"}}}
            instance.exists = lambda *args: True
            instance.ensure_owned = lambda *args: None
            mutations = []
            def query(*args):
                if args[0] == "domstate":
                    return "shut off"
                if args[0] == "list":
                    return "own-a\nforeign" if shared else "own-a"
                if args[0] == "net-list":
                    return "remora-test"
                if args[0] == "net-destroy":
                    mutations.append(args[1])
                    return ""
                self.fail("unexpected command")
            instance.virsh = query
            instance.xml = lambda *args: ET.fromstring(
                '<domain><devices><interface><source network="remora-test"/></interface></devices></domain>')
            instance.domain_xmls = lambda name: [instance.xml("domain", name)]
            if shared:
                with self.assertRaises(lab.LabError):
                    instance.stop()
                self.assertEqual(mutations, [])
            else:
                instance.stop()
                self.assertEqual(mutations, ["remora-test"])

    def test_live_only_attachments_and_transient_guests_block_cleanup(self):
        for persistent in (False, True):
            for resource in ("network", "volume"):
                with self.subTest(persistent=persistent, resource=resource):
                    instance = object.__new__(lab.Lab)
                    instance.m = {"name": "remora-test", "network_uuid": "n", "pool": "default",
                                  "domains": {"a": {"name": "own-a", "uuid": "u"}},
                                  "volumes": {"disk": "/pool/disk"}}
                    instance.exists = lambda *a: True
                    instance.ensure_owned = lambda *a: None
                    instance.volume_names = lambda: ["disk"]
                    commands = []
                    def query(*a, **kw):
                        commands.append(a)
                        if a[0] == "vol-key": return "/pool/disk"
                        if a[0] == "domstate": return "shut off"
                        if a[0] == "list":
                            if "--persistent" in a: return "own-a\nforeign" if persistent else "own-a"
                            return "own-a\nforeign"
                        if a[0] == "dumpxml":
                            if "--inactive" in a: return "<domain><devices/></domain>"
                            return ('<domain><devices><interface><source network="remora-test"/></interface></devices></domain>'
                                    if resource == "network" else
                                    '<domain><devices><disk><source file="/pool/disk"/></disk></devices></domain>')
                        self.fail("unexpected mutation " + str(a))
                    instance.virsh = query
                    with self.assertRaises(lab.LabError): instance.destroy()
                    if resource == "network":
                        with self.assertRaises(lab.LabError): instance.stop()
                    self.assertFalse(any(c[0] in ("shutdown", "destroy", "vol-delete", "net-destroy", "undefine") for c in commands))

    def test_unrecorded_volume_keeps_recovery_intent_and_refuses_completed_cleanup(self):
        for failure in (lab.LabError("vol-key failed"), KeyboardInterrupt()):
            with tempfile.TemporaryDirectory() as directory, self.subTest(failure=type(failure)):
                instance = object.__new__(lab.Lab)
                instance.state = Path(directory)
                instance.manifest = instance.state / "manifest.json"
                image = instance.state / "image.qcow2"
                image.write_bytes(b"image")
                instance.m = {"name": "remora-test", "pool": "default", "domains": {},
                              "volumes": {}, "phase": "creating"}
                created = []
                instance.volume_names = lambda: list(created)
                instance.exists = lambda *a: False
                def query(*a, **kw):
                    if a[0] == "vol-create-as": created.append(a[2]); return ""
                    if a[0] == "vol-key": raise failure
                    if a[0] == "list": return ""
                    self.fail("unsafe deletion or unexpected query " + str(a))
                instance.virsh = query
                with mock.patch.object(lab, "run", return_value=SimpleNamespace(stdout=b'{"virtual-size":1024}')):
                    with self.assertRaises(type(failure)): instance.volume("a.qcow2", image, "qcow2")
                recorded = lab.json.loads(instance.manifest.read_text())
                self.assertIn("remora-test-a.qcow2", recorded["volume_intents"])
                with self.assertRaises(lab.LabError): instance.destroy()
                self.assertNotEqual(instance.m["phase"], "destroyed")
                self.assertEqual(created, ["remora-test-a.qcow2"])
                # After the operator resolves/removes the ambiguous resource,
                # ordinary recovery can finish without deleting by name alone.
                created.clear()
                with mock.patch("builtins.print"): instance.destroy()
                self.assertEqual(instance.m["phase"], "destroyed")

    def test_domain_views_include_live_and_next_boot_but_not_inactive_transients(self):
        for persistent in (False, True):
            instance = object.__new__(lab.Lab)
            calls = []
            def query(*a):
                calls.append(a)
                if a[0] == "list": return "foreign" if persistent else ""
                return ('<domain><devices><disk><source file="/pool/next-boot"/></disk></devices></domain>'
                        if "--inactive" in a else '<domain><devices/></domain>')
            instance.virsh = query
            views = instance.domain_xmls("foreign")
            self.assertEqual(len(views), 2 if persistent else 1)
            self.assertEqual(any(x.findall("devices/disk/source") for x in views), persistent)
            self.assertEqual(any("--inactive" in a for a in calls), persistent)

    def test_volume_identity_is_durable_before_upload_interrupt(self):
        with tempfile.TemporaryDirectory() as directory:
            instance = object.__new__(lab.Lab)
            instance.state = Path(directory)
            instance.manifest = instance.state / "manifest.json"
            image = instance.state / "source.iso"
            image.write_bytes(b"iso")
            volume = instance.state / "volume.iso"
            names = []
            instance.m = {"name": "remora-test", "pool": "default", "domains": {},
                          "volumes": {}, "phase": "creating"}
            instance.exists = lambda *a: False
            instance.volume_names = lambda: list(names)
            def query(*a, **kw):
                if a[0] == "vol-create-as": names.append(a[2]); volume.write_bytes(b""); return ""
                if a[0] in ("vol-key", "vol-path"): return str(volume)
                if a[0] == "list": return ""
                if a[0] == "vol-upload":
                    recorded = lab.json.loads(instance.manifest.read_text())
                    identity = recorded["volumes"][names[0]]
                    self.assertEqual(identity["key"], str(volume))
                    self.assertEqual(identity["inode"], volume.stat().st_ino)
                    self.assertFalse(recorded["volume_intents"])
                    raise KeyboardInterrupt()
                if a[0] == "vol-delete": names.clear(); return ""
                self.fail("unexpected query " + str(a))
            instance.virsh = query
            with self.assertRaises(KeyboardInterrupt): instance.volume("seed.iso", image, "raw")
            with mock.patch("builtins.print"): instance.destroy()
            self.assertEqual(names, [])
            self.assertEqual(instance.m["phase"], "destroyed")


class CacheTests(unittest.TestCase):
    def setUp(self):
        silence = mock.patch("builtins.print")
        silence.start()
        self.addCleanup(silence.stop)
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.data = b"verified fixture image bytes"
        self.sha = lab.hashlib.sha512(self.data).hexdigest()
        self.calls = []

    def tearDown(self):
        self.directory.cleanup()

    def command(self, *args, **kw):
        self.calls.append(args[0])
        if args[0] == "curl":
            Path(args[-1]).write_bytes(self.data)
            return SimpleNamespace(stdout=b"")
        self.assertEqual(args[0], "qemu-img")
        return SimpleNamespace(stdout=b'{"format":"qcow2"}')

    def test_download_once_across_independent_lab_runs(self):
        with mock.patch.object(lab, "IMAGE_SHA512", self.sha), mock.patch.object(lab, "run", self.command):
            one = lab.cached_image(self.root)
            two = lab.cached_image(self.root)
        self.assertEqual(one, two)
        self.assertEqual(self.calls.count("curl"), 1)
        self.assertEqual(one.read_bytes(), self.data)
        self.assertFalse(list(self.root.glob("*.partial")))

    def test_import_and_future_cache_hit_use_no_network(self):
        source = self.root / "source.qcow2"
        source.write_bytes(self.data)
        with mock.patch.object(lab, "IMAGE_SHA512", self.sha), mock.patch.object(lab, "run", self.command):
            lab.cached_image(self.root / "shared", source)
            lab.cached_image(self.root / "shared")
        self.assertNotIn("curl", self.calls)

    def test_corruption_is_rejected_without_silent_redownload(self):
        with mock.patch.object(lab, "IMAGE_SHA512", self.sha), mock.patch.object(lab, "run", self.command):
            image = lab.cached_image(self.root)
            self.calls.clear()
            image.write_bytes(b"corrupted")
            with self.assertRaises(lab.LabError):
                lab.cached_image(self.root)
        self.assertEqual(self.calls, [])
        self.assertEqual(image.read_bytes(), b"corrupted")

    def test_failed_download_cannot_publish_partial_cache(self):
        def fail(*args, **kwargs):
            Path(args[-1]).write_bytes(b"partial")
            raise lab.LabError("network failed")
        with mock.patch.object(lab, "run", fail):
            with self.assertRaises(lab.LabError):
                lab.cached_image(self.root)
        self.assertEqual([p.name for p in self.root.iterdir()], ["lock"])


class DisposableTests(unittest.TestCase):
    def setUp(self):
        silence = mock.patch("builtins.print")
        silence.start()
        self.addCleanup(silence.stop)

    def test_creation_failure_and_interrupt_both_cleanup(self):
        for error in (lab.LabError("provisioning failed"), KeyboardInterrupt()):
            with tempfile.TemporaryDirectory() as directory:
                instance = object.__new__(lab.Lab)
                instance.state = Path(directory)
                instance.private = instance.state / "private"
                instance.cache = instance.state / "cache"
                instance.a = SimpleNamespace(output=instance.state / "results")
                instance.m = None
                cleanups = []
                def create():
                    instance.m = {"phase": "creating"}
                    raise error
                instance.create = create
                instance.destroy = lambda force=False: cleanups.append(force)
                with self.assertRaises(type(error)):
                    instance.test()
                self.assertEqual(cleanups, [True])

    def test_never_cleanup_an_existing_debug_lab(self):
        instance = object.__new__(lab.Lab)
        instance.m = {"phase": "configured"}
        instance.destroy = lambda **kw: self.fail("existing lab must be retained")
        with self.assertRaises(lab.LabError):
            instance.test()

    def test_cleanup_failure_preserves_private_recovery_state(self):
        with tempfile.TemporaryDirectory() as directory:
            instance = object.__new__(lab.Lab)
            instance.state = Path(directory)
            instance.private = instance.state / "private"
            instance.cache = instance.state / "cache"
            instance.a = SimpleNamespace(output=instance.state / "results")
            instance.m = None
            def create():
                instance.m = {"phase": "creating"}
                instance.private.mkdir()
                raise lab.LabError("provisioning failed")
            instance.create = create
            def destroy(**kwargs):
                raise lab.LabError("identity changed")
            instance.destroy = destroy
            with self.assertRaises(lab.LabError):
                instance.test()
            self.assertTrue(instance.private.is_dir())


if __name__ == "__main__":
    unittest.main()
