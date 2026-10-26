import copy
import json
from pathlib import Path
import sys
import unittest

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts/validate'))
from wan_regression import evaluate


class RegressionGates(unittest.TestCase):
    def setUp(self):
        self.policy=json.loads((ROOT/'benchmarks/policies/wan-loss-regression.json').read_text())
        resource=dict(average_cpu_cores=.3,owner_average_cpu_cores=.29,peak_rss_bytes=10_000_000)
        measurement=dict(mode="tunnel",generator_errors=0,integrity_errors=0,generator_cpu_max=.2,clock_drift_ms=0,
                         coverage_lost=0,counter_overflow=False,kernel_rx_overflow_complete=True,offered=[15000,15000],submitted=[15000,15000],
                         unique=[15000,15000],unique_per_session=[[15000,15000]],rtt_samples_per_session=[150],rtt_p95_ms=20,resources=[resource,resource])
        self.rows={label:copy.deepcopy(measurement) for label in ['direct','control','code','selected']}
        self.rows['direct'].update(mode='direct',kernel_rx_overflow_complete=None)
        self.workload=['small',1,128,1000,15]
    def test_tunnel_cannot_claim_baseline_kernel_coverage_exemption(self):
        self.rows['code'].update(mode='baseline', kernel_rx_overflow_complete=None)
        result = evaluate(self.rows, self.workload, self.policy)
        self.assertFalse(result['passed'])
        self.assertTrue(any('kernel socket drop telemetry' in reason for reason in result['invalid_reasons']))

    def test_identical_limits_compare_code_while_profile_keeps_absolute_resource_bounds(self):
        self.rows['selected']['resources'][0]['peak_rss_bytes']=22_000_000
        self.assertTrue(evaluate(self.rows,self.workload,self.policy)['passed'])
        self.rows['code']['resources'][0]['peak_rss_bytes']=22_000_000
        self.assertIn('code:client:paired RSS',evaluate(self.rows,self.workload,self.policy)['failed'])
    def test_invalid_direct_or_missing_reverse_load_cannot_establish_comparative_pass(self):
        self.rows['direct']['generator_cpu_max']=.9
        result=evaluate(self.rows,self.workload,self.policy)
        self.assertFalse(result['passed'])
        self.assertTrue(result['invalid_reasons'])
        self.rows['direct']['generator_cpu_max']=.2
        self.rows['selected']['submitted'][1]=0
        self.assertTrue(evaluate(self.rows,self.workload,self.policy)['invalid_reasons'])
    def test_code_cpu_and_selected_absolute_rss_are_independent_gates(self):
        self.rows['code']['resources'][0]['average_cpu_cores']=.4
        self.rows['selected']['resources'][1]['peak_rss_bytes']=135_000_000
        failed=evaluate(self.rows,self.workload,self.policy)['failed']
        self.assertIn('code:client:paired average_cpu_cores',failed)
        self.assertIn('selected:server:SC-007 resource bound',failed)
    def test_negative_clock_step_is_invalid_too(self):
        self.rows['direct']['clock_drift_ms']=-11
        self.assertTrue(evaluate(self.rows,self.workload,self.policy)['invalid_reasons'])
    def test_user_cpu_tradeoff_never_waives_absolute_resource_or_other_gates(self):
        self.rows['code']['resources'][0]['average_cpu_cores']=.4
        self.assertFalse(evaluate(self.rows,self.workload,self.policy)['passed'])
        self.policy['paired_cpu_informational']=True
        result=evaluate(self.rows,self.workload,self.policy)
        self.assertTrue(result['passed'])
        self.assertFalse(result['paired_cpu_gated'])
        self.assertTrue(any(row['delta_cores']>.09 for row in result['cpu_observations']))
        self.rows['code']['resources'][0]['average_cpu_cores']=2.001
        self.rows['code']['resources'][0]['peak_rss_bytes']=135_000_000
        failed=evaluate(self.rows,self.workload,self.policy)['failed']
        self.assertIn('code:client:SC-007 resource bound',failed)
        self.assertIn('code:client:paired RSS',failed)
    def test_per_session_and_both_role_resource_evidence_are_mandatory(self):
        valid=copy.deepcopy(self.rows)
        for key,value in [('unique_per_session',[]),('unique_per_session',[[14999,15000]]),
                          ('resources',[]),('resources',self.rows['code']['resources'][:1])]:
            self.rows=copy.deepcopy(valid)
            self.rows['code'][key]=value
            self.assertTrue(evaluate(self.rows,self.workload,self.policy)['invalid_reasons'])
        self.rows=copy.deepcopy(valid)
        del self.rows['code']['unique_per_session']
        self.assertTrue(evaluate(self.rows,self.workload,self.policy)['invalid_reasons'])
    def test_nonfinite_resource_cannot_skip_numeric_bounds(self):
        for value in [float('nan'),float('inf'),-1]:
            self.rows['code']['resources'][0]['average_cpu_cores']=value
            self.assertTrue(evaluate(self.rows,self.workload,self.policy)['invalid_reasons'])


if __name__=='__main__':unittest.main()
