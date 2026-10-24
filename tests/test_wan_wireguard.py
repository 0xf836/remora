import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts/validate'))
from wan_wireguard import validate_topology


class WireGuardTopology(unittest.TestCase):
    def test_inner_route_must_use_wireguard(self):
        validate_topology([{'mtu':1280}], [{'dev':'wanwg0'}])
        for routes in [[], [{'dev':'lo'}], [{'dev':'veth0'}], [{'dev':'wanwg0'}, {'dev':'wanwg0'}]]:
            with self.assertRaisesRegex(RuntimeError, 'route bypasses'):
                validate_topology([{'mtu':1280}], routes)

    def test_mtu_must_be_observed_not_assumed(self):
        for links in [[], [{'mtu':1120}], [{'mtu':1500}], [{}]]:
            with self.assertRaisesRegex(RuntimeError, 'MTU observation'):
                validate_topology(links, [{'dev':'wanwg0'}])


if __name__ == '__main__':
    unittest.main()
