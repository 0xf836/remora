"""A newly spawned taskset child must install its CPU mask before sampling."""
import sys
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts/validate'))
import benchmark as b


class CpuReadiness(unittest.TestCase):
    def test_constructor_waits_through_inherited_mask_before_return(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(b.subprocess,'Popen',return_value=SimpleNamespace(pid=123)) as spawn, patch.object(b.os,'sched_getaffinity',side_effect=[{0,2,4,6},{2}]) as affinity, patch.object(b.Owned,'exited',return_value=False), patch.object(b.time,'sleep') as sleep:
            child=b.Owned(['role'],Path(temp)/'role',cpu=2)
            try:
                self.assertEqual(spawn.call_args.args[0],['taskset','-c','2','role'])
                self.assertEqual(affinity.call_count,2)
                sleep.assert_called_once_with(.001)
            finally:child.out.close();child.err.close()

    def test_failed_assignment_cleans_owned_child_with_bounded_deadline(self):
        child=object.__new__(b.Owned);child.p=SimpleNamespace(pid=123)
        with patch.object(b.os,'sched_getaffinity',return_value={0,2,4,6}), patch.object(b.Owned,'exited',return_value=False), patch.object(b.time,'monotonic',side_effect=[0,4]), patch.object(child,'stop') as stop:
            with self.assertRaises(RuntimeError):child._wait_for_cpu(2)
            stop.assert_called_once()


if __name__=='__main__':unittest.main()
