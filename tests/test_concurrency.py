import hashlib
import json
from pathlib import Path
import stat
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts/validate"))
from concurrency import Lab, packet, udp


class PacketFailureEvidence(unittest.TestCase):
    def test_unexpected_packet_is_rejected_with_private_bounded_first_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            lab = Lab.__new__(Lab)
            lab.logs = Path(directory)
            lab.scenario = "injected-test"
            lab.packet_failure = None
            lab.targets, lab.senders = [udp()], []
            lab.corrupt = 0
            sender = udp()
            try:
                payload = b"private-sentinel" * 4000
                sender.sendto(payload, lab.targets[0].getsockname())
                with self.assertRaisesRegex(AssertionError, "unexpected application payload"):
                    lab.exchange(time.monotonic() + 1)
                path = lab.logs / "failure.json"
                text = path.read_text()
                failure = json.loads(text)
                self.assertEqual(lab.corrupt, 1)
                self.assertEqual(failure["datagram_bytes"], len(payload))
                self.assertEqual(failure["datagram_sha256"], hashlib.sha256(payload).hexdigest())
                self.assertEqual(failure["local_address"], list(lab.targets[0].getsockname()))
                self.assertEqual(failure["peer_address"], list(sender.getsockname()))
                self.assertIsNone(failure["recognized_kind"])
                self.assertLess(len(text), 2048)
                self.assertNotIn("private-sentinel", text)
                self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
                with self.assertRaisesRegex(AssertionError, "payload integrity failed"):
                    lab.fail_packet("payload integrity failed", lab.targets[0], b"x", sender.getsockname())
                self.assertEqual(path.read_text(), text)
            finally:
                sender.close()
                lab.targets[0].close()

    def test_valid_sized_corruption_still_fails_integrity(self):
        with tempfile.TemporaryDirectory() as directory:
            lab = Lab.__new__(Lab)
            lab.logs = Path(directory)
            lab.scenario = "injected-test"
            lab.packet_failure = None
            lab.targets, lab.senders = [udp()], []
            lab.corrupt = 0
            sender = udp()
            try:
                payload = packet(b"P", 0, 1)[:-1] + b"x"
                sender.sendto(payload, lab.targets[0].getsockname())
                with self.assertRaisesRegex(AssertionError, "payload integrity failed"):
                    lab.exchange(time.monotonic() + 1)
                failure = json.loads((lab.logs / "failure.json").read_text())
                self.assertEqual(failure["recognized_kind"], "P")
                self.assertEqual(failure["datagram_bytes"], 128)
                self.assertEqual(lab.corrupt, 1)
            finally:
                sender.close()
                lab.targets[0].close()


if __name__ == "__main__":
    unittest.main()
