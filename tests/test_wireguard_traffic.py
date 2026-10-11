import importlib.util
from pathlib import Path
import unittest

PATH = Path(__file__).resolve().parents[1] / "scripts/validate/wireguard_traffic.py"
SPEC = importlib.util.spec_from_file_location("wireguard_traffic", PATH)
traffic = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(traffic)


class TrafficIntegrity(unittest.TestCase):
    def test_mixed_payloads_and_warmup(self):
        run = bytes(range(16))
        for size in traffic.SIZES:
            for seq in (0, 17999, traffic.WARMUP):
                packet = traffic.packet(run, 0, 1, seq, size)
                self.assertEqual(len(packet), size)
                self.assertEqual(traffic.validate(packet, run, 0, 1, 18000), seq)

    def test_cross_delivery_corruption_and_out_of_range(self):
        run = bytes(range(16))
        packet = traffic.packet(run, 0, 1, 10, 512)
        for arguments in ((bytes(16), 0, 1, 18000), (run, 1, 1, 18000),
                          (run, 0, 0, 18000), (run, 0, 1, 10)):
            self.assertIsNone(traffic.validate(packet, *arguments))
        for corrupted in (packet[:-1], packet + b"x", packet[:50] + b"X" + packet[51:], b""):
            self.assertIsNone(traffic.validate(corrupted, run, 0, 1, 18000))


if __name__ == "__main__":
    unittest.main()
