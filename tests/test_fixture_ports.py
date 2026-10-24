import concurrent.futures
import json
from pathlib import Path
import socket
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts/validate'))
import fixture_ports


class FixturePorts(unittest.TestCase):
    def test_private_network_namespaces_have_independent_registries(self):
        with patch.object(fixture_ports.os, 'stat', return_value=SimpleNamespace(st_ino=100)):
            first = fixture_ports.registry_directory()
        with patch.object(fixture_ports.os, 'stat', return_value=SimpleNamespace(st_ino=101)):
            second = fixture_ports.registry_directory()
        self.assertNotEqual(first, second)

    def close(self, sockets):
        for sock in sockets:
            sock.close()

    def test_configured_ports_are_outside_kernel_ephemeral_range_and_both_families_reserved(self):
        with tempfile.TemporaryDirectory() as directory:
            ports, sockets = fixture_ports.reserve_ports('ipv4', directory)
            try:
                first, last = map(int, Path('/proc/sys/net/ipv4/ip_local_port_range').read_text().split())
                self.assertEqual(len(set(ports)), 3)
                self.assertTrue(all(not first <= p <= last for p in ports))
                for af, host in [(socket.AF_INET, '127.0.0.1'), (socket.AF_INET6, '::1')]:
                    for port in ports:
                        with socket.socket(af, socket.SOCK_DGRAM) as other:
                            if af == socket.AF_INET6:
                                other.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                            with self.assertRaises(OSError):
                                other.bind((host, port))
            finally:
                self.close(sockets)

    def test_released_socket_and_deleted_fixture_cannot_reuse_live_callers_ports(self):
        with tempfile.TemporaryDirectory() as directory:
            ports, sockets = fixture_ports.reserve_ports('ipv4', directory)
            self.close(sockets)
            # Reproduce the alias: a late sender still addresses a stopped
            # server, and a new unleased target can bind that released tuple.
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as late:
                late.bind(('127.0.0.1', 0))
                old = ('127.0.0.1', ports[0])
                with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as alias:
                    alias.bind(old)
                    alias.settimeout(0.2)
                    late.sendto(b'old-connection-packet', old)
                    self.assertEqual(alias.recv(128), b'old-connection-packet')
            # Force the old proxy/target choices first, as ephemeral reuse can.
            choose = fixture_ports.secrets.choice
            offered = iter(ports)
            def choices(values):
                return next(offered, None) or choose(values)
            with patch.object(fixture_ports.secrets, 'choice', side_effect=choices):
                fresh, sockets = fixture_ports.reserve_ports('ipv6', directory)
            try:
                self.assertTrue(set(ports).isdisjoint(fresh))
            finally:
                self.close(sockets)

    def test_parallel_callers_have_disjoint_leases(self):
        with tempfile.TemporaryDirectory() as directory:
            def reserve(_):
                ports, sockets = fixture_ports.reserve_ports('ipv4', directory)
                self.close(sockets)
                return ports
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                ports = sum(pool.map(reserve, range(8)), [])
            self.assertEqual(len(set(ports)), 24)

    def test_dead_owner_is_reclaimed_and_pid_reuse_is_not_a_live_lease(self):
        with tempfile.TemporaryDirectory() as directory:
            ports, sockets = fixture_ports.reserve_ports('ipv4', directory)
            self.close(sockets)
            state = Path(directory) / 'leases.json'
            leases = json.loads(state.read_text())
            for record in leases.values():
                record['identity'] = 'different-process-start'
            state.write_text(json.dumps(leases))
            with patch.object(fixture_ports.secrets, 'choice', side_effect=ports):
                reused, sockets = fixture_ports.reserve_ports('ipv4', directory)
            self.close(sockets)
            self.assertEqual(reused, ports)

    def test_unsafe_registry_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            root.chmod(0o755)
            with self.assertRaisesRegex(RuntimeError, 'unsafe'):
                fixture_ports.reserve_ports('ipv4', root)
            root.chmod(0o700)
            link = root / 'symlink'
            link.symlink_to(root, target_is_directory=True)
            with self.assertRaisesRegex(RuntimeError, 'unsafe'):
                fixture_ports.reserve_ports('ipv4', link)


if __name__ == '__main__':
    unittest.main()
