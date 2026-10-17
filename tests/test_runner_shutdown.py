import json
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RunnerShutdown(unittest.TestCase):
    def test_termination_reaps_owned_group_and_runs_finally(self):
        code = '''
import sys,time,json
from pathlib import Path
sys.path.insert(0,sys.argv[1])
from benchmark import Owned,install_termination_handler
install_termination_handler()
root=Path(sys.argv[2]);owned=Owned(["sleep","60"],root/"owned")
print(owned.p.pid,flush=True)
try:
    time.sleep(60)
finally:
    owned.stop()
    (root/"cleanup.json").write_text(json.dumps({"reaped":owned.reaped,"returncode":owned.p.returncode}))
'''
        for signum in [signal.SIGTERM, signal.SIGHUP]:
            with self.subTest(signum=signum), tempfile.TemporaryDirectory() as temp:
                proc = subprocess.Popen(
                    [sys.executable, "-c", code, str(ROOT / "scripts/validate"), temp],
                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
                )
                try:
                    self.assertTrue(select.select([proc.stdout], [], [], 3)[0])
                    child = int(proc.stdout.readline())
                    proc.send_signal(signum)
                    self.assertNotEqual(proc.wait(timeout=5), 0)
                    result = json.loads((Path(temp) / "cleanup.json").read_text())
                    self.assertTrue(result["reaped"])
                    self.assertIsNotNone(result["returncode"])
                    self.assertFalse(Path(f"/proc/{child}").exists())
                finally:
                    if proc.poll() is None:
                        proc.kill()
                        proc.wait(timeout=3)
                    proc.stdout.close()
