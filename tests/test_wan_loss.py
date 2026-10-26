import copy
import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'scripts/validate'))
from wan_loss import invalid_reasons, timeline_invalid_reasons, frozen_input_paths, validate_frozen_inputs
import tempfile

class WanValidity(unittest.TestCase):
    def setUp(self):
        self.result = dict(generator_errors=0, integrity_errors=0, coverage_lost=0,
                           counter_overflow=False, kernel_rx_overflow_complete=True, generator_cpu_max=.2, clock_drift_ms=0,
                           offered=[0,200], submitted=[0,200], unique=[0,200],
                           resources=[dict(average_cpu_cores=.2,owner_average_cpu_cores=.19,peak_rss_bytes=10000000) for _ in range(2)])
        self.rx = {'sessions':1}
        self.sequences = [[],[10]*20]
        self.collections = [{},{'latency_samples':200,'clock_errors':0}]
    def check(self):
        return invalid_reasons(self.result, {}, self.rx, self.sequences, self.collections, 'reverse',10,20)
    def test_valid_traffic_still_requires_two_role_resources(self):
        for values in [[],self.result['resources'][:1]]:
            self.result['resources']=values
            self.assertTrue(any(reason.startswith('resources: ') for reason in self.check()))
    def test_incomplete_kernel_telemetry_invalidates_loss_evidence(self):
        for value in [False, None]:
            self.result['kernel_rx_overflow_complete'] = value
            self.assertIn('kernel socket drop telemetry incomplete or unknown', self.check())
        del self.result['kernel_rx_overflow_complete']
        self.assertIn('kernel socket drop telemetry incomplete or unknown', self.check())
    def test_complete_reverse_measurement(self):
        self.assertEqual(self.check(), [])
    def test_missing_reverse_peer_and_load_is_invalid_even_without_generator_errors(self):
        self.rx['sessions']=0
        self.result.update(offered=[0,0], submitted=[0,0], unique=[0,0])
        self.assertIn('expected one discovered peer',self.check())
        self.assertIn('required offered/submitted workload missing',self.check())
    def test_clock_steps_and_saturated_generator_are_invalid(self):
        for step in [-11,11]:
            self.result['clock_drift_ms']=step
            self.assertIn('clock mapping unstable',self.check())
        self.result['clock_drift_ms']=0
        self.result['generator_cpu_max']=.9
        self.assertIn('generator lacks CPU headroom',self.check())
    def test_required_interval_and_age_fields_cannot_be_missing(self):
        self.sequences[1]=[]
        self.collections[1]={}
        self.assertIn('missing/inconsistent sequence intervals',self.check())
        self.assertIn('missing one-way samples',self.check())
    def test_rate_steps_use_declared_phase_totals(self):
        rates = [1]*5+[2]*5+[5]*5+[10]*5
        self.result.update(offered=[0,90],submitted=[0,90],unique=[0,90])
        self.sequences[1]=rates
        self.collections[1]['latency_samples']=90
        self.assertEqual(invalid_reasons(self.result,{},self.rx,self.sequences,self.collections,'reverse',10,20,rates), [])

class TimelineValidity(unittest.TestCase):
    def test_complete_short_timeline_reconciles_and_missing_data_cannot_pass(self):
        endpoint=dict(timeline_bin_us=10000,timeline_seconds=21,
                      offered_10ms=[1]*200+[0]*1900,arrival_10ms=[1]*200+[0]*1900)
        result=dict(offered=[200,200],unique=[200,200])
        self.assertEqual(timeline_invalid_reasons(endpoint,endpoint,result,20),[])
        self.assertTrue(timeline_invalid_reasons({},endpoint,result,20))
        endpoint['arrival_10ms'][0]=0
        self.assertIn('endpoint timeline does not reconcile with packet counts',timeline_invalid_reasons(endpoint,endpoint,result,20))

    def test_soak_timeline_is_explicitly_a_bounded_prefix(self):
        endpoint=dict(timeline_bin_us=10000,timeline_seconds=30,
                      offered_10ms=[1]*3000,arrival_10ms=[1]*3000)
        result=dict(offered=[180000,180000],unique=[180000,180000])
        self.assertEqual(timeline_invalid_reasons(endpoint,endpoint,result,1800),[])
        endpoint['offered_10ms']=[1]*180000
        self.assertTrue(timeline_invalid_reasons(endpoint,endpoint,result,1800))

class FrozenInputs(unittest.TestCase):
    def test_reexecuted_shell_fixture_is_frozen_and_mutation_or_removal_invalidates(self):
        self.assertIn(Path(__file__).resolve().parents[1]/'scripts/validate/fixtures.sh',frozen_input_paths())
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'fixtures.sh'
            path.write_text('original')
            from benchmark import sha
            frozen={str(path):sha(path)}
            validate_frozen_inputs(frozen)
            path.write_text('changed')
            with self.assertRaises(RuntimeError):validate_frozen_inputs(frozen)
            path.unlink()
            with self.assertRaises(RuntimeError):validate_frozen_inputs(frozen)

if __name__ == '__main__': unittest.main()
