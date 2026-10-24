import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts/validate'))
from wan_loss_report import recovery, evaluate


POLICY = dict(recovery_deadline_seconds=2, recovery_hold_seconds=3,
              recovery_delivered_mbit_s_min=190, cold_loss_max_percent=1,
              steady_delivered_mbit_s_min=198, steady_added_loss_max_pp=.1,
              transient_added_one_way_delay_max_ms=200,
              steady_added_rtt_p95_max_ms=5, rss_max_bytes=134217728,
              injected_outer_data_packet_drops=5)


def role_resources():
    return [dict(average_cpu_cores=.2, owner_average_cpu_cores=.19, peak_rss_bytes=10000000) for _ in range(2)]


def role_recovery():
    return {name: dict(connection_attempts=1, watchdog_timeouts=0) for name in ['client','server']}


class WanReport(unittest.TestCase):
    def test_rtt100_rate_steps_recover_at_high_rate_transition_not_low_rate_start(self):
        row,baseline,args=self.complete_measurements()
        counts=[2083]*5+[5208]*5+[10416]*5+[20833]*5
        row.update(offered_pps_intervals=counts,sequence_seconds=[counts.copy(),[]],arrival_seconds=[counts.copy(),[]])
        args.update(rtt_ms=100,rate_steps=True)
        startup=dict(cold_loss_max_percent_by_rtt_ms={'55':1,'60':1,'100':4},rtt100_startup_loss_payload_bytes_max=20_000_000)
        report=evaluate(row,baseline,args,dict(kind='none'),POLICY,startup)
        self.assertEqual(report['failed'],[])
        self.assertNotIn('forward_startup_recovery',report['checks'])
        self.assertTrue(report['not_applicable'])
        self.assertTrue(report['checks']['forward_step_recovery'])
        row['arrival_seconds'][0][15:]=[0]*5
        self.assertIn('forward_step_recovery',evaluate(row,baseline,args,dict(kind='none'),POLICY,startup)['failed'])
        row['arrival_seconds'][0]=[2083]*20
        args['rate_steps']=False
        self.assertIn('forward_startup_recovery',evaluate(row,baseline,args,dict(kind='none'),POLICY,startup)['failed'])

    def complete_measurements(self):
        row = dict(binary='candidate', invalid_reasons=[], loss_percent=[0,None],
                   steady_loss_percent=[0,None], one_way_delay_max_us=[28000,None],
                   steady_rtt_p95_ms=55, resources=role_resources(), recovery=role_recovery())
        baseline = dict(invalid_reasons=[], steady_loss_percent=[0,None],
                        one_way_delay_min_us=[27000,None], steady_rtt_p95_ms=55,
                        resources=role_resources())
        return row, baseline, dict(size=1200, seconds=20, direction='forward')

    def test_missing_or_nonfinite_resources_cannot_establish_pass(self):
        invalid = [None, [], role_resources()[:1], [None, {}]]
        for field, value in [('average_cpu_cores', True), ('average_cpu_cores', -.1),
                             ('average_cpu_cores', float('nan')), ('owner_average_cpu_cores', float('inf')),
                             ('peak_rss_bytes', True), ('peak_rss_bytes', 0), ('peak_rss_bytes', -1)]:
            values = role_resources(); values[0][field] = value; invalid.append(values)
        values = role_resources(); del values[1]['owner_average_cpu_cores']; invalid.append(values)
        for values in invalid:
            with self.subTest(values=values):
                row, baseline, args = self.complete_measurements(); row['resources'] = values
                report = evaluate(row,baseline,args,dict(kind='none'),POLICY)
                self.assertFalse(report['checks']['measurement_valid'])
                self.assertNotIn('rss',report['checks']); self.assertNotIn('cpu',report['checks'])
                self.assertTrue(any('candidate resources' in message for message in report['missing']))
                row, baseline, args = self.complete_measurements(); baseline['resources'] = values
                report = evaluate(row,baseline,args,dict(kind='none'),POLICY)
                self.assertFalse(report['checks']['baseline_measurement_valid'])
                self.assertNotIn('forward_transient_delay',report['checks'])
                self.assertNotIn('steady_added_rtt',report['checks'])

    def test_exact_resource_bounds_and_candidate_cpu_only(self):
        row,baseline,args=self.complete_measurements()
        for resource in row['resources']:
            resource.update(average_cpu_cores=2.,peak_rss_bytes=POLICY['rss_max_bytes'])
        baseline['resources'][0]['average_cpu_cores']=3.
        report=evaluate(row,baseline,args,dict(kind='none'),POLICY)
        self.assertEqual(report['failed'],[]);self.assertEqual(report['missing'],[])
        self.assertEqual(report['resource_bounds']['role_cpu_cores_max'],2.)
        row['resources'][1]['average_cpu_cores']=2.001
        row['resources'][1]['peak_rss_bytes']+=1
        report=evaluate(row,baseline,args,dict(kind='none'),POLICY)
        self.assertEqual(set(report['failed']),{'cpu','rss'})

    def test_missing_or_incomplete_recovery_is_explicit_not_vacuous(self):
        invalid=[None,{}, {'client':dict(connection_attempts=1,watchdog_timeouts=0)}]
        for value in [True,-1,float('nan'),float('inf')]:
            values=role_recovery();values['server']['connection_attempts']=value;invalid.append(values)
        values=role_recovery();del values['client']['watchdog_timeouts'];invalid.append(values)
        for values in invalid:
            with self.subTest(values=values):
                row,baseline,args=self.complete_measurements();row['recovery']=values
                report=evaluate(row,baseline,args,dict(kind='none'),POLICY)
                self.assertIn('recovery_evidence',report['failed'])
                self.assertNotIn('no_unexpected_recovery',report['checks'])
                self.assertTrue(any('candidate recovery' in message for message in report['missing']))

    def test_recovery_requires_three_complete_seconds_within_deadline(self):
        arrivals = [20833] * 20
        arrivals[6] = 0
        self.assertEqual(recovery(arrivals, 1200, 6005, POLICY), .995)
        arrivals[8] = 0
        self.assertIsNone(recovery(arrivals, 1200, 6005, POLICY))
        self.assertIsNone(recovery([20833] * 9, 1200, 6005, POLICY))

    def test_total_loss_does_not_hide_failed_steady_or_delay_gate(self):
        row = dict(recovery=role_recovery(), invalid_reasons=[], loss_percent=[.5, None],
                   steady_loss_percent=[1, None], one_way_delay_max_us=[240000, None],
                   steady_rtt_p95_ms=56, resources=role_resources())
        baseline = dict(resources=role_resources(), invalid_reasons=[], steady_loss_percent=[0, None],
                        one_way_delay_min_us=[27000, None], steady_rtt_p95_ms=55)
        result = evaluate(row, baseline, dict(size=1200, seconds=20, direction='forward'),
                          dict(kind='none'), POLICY)
        self.assertEqual(set(result['failed']), {'forward_steady_rate', 'forward_steady_added_loss', 'forward_transient_delay'})
        self.assertEqual(result['missing'], [])
        self.assertFalse(result['acceptance_complete'])

    def test_invalid_baseline_cannot_establish_comparative_passes(self):
        row = dict(recovery=role_recovery(), invalid_reasons=[], loss_percent=[0, None],
                   steady_loss_percent=[0, None], one_way_delay_max_us=[28000, None],
                   steady_rtt_p95_ms=55, resources=role_resources())
        for reasons in [['clock mapping unstable'], ['generator lacks CPU headroom']]:
            baseline = dict(resources=role_resources(), invalid_reasons=reasons, steady_loss_percent=[0, None],
                            one_way_delay_min_us=[27000, None], steady_rtt_p95_ms=55)
            result = evaluate(row, baseline, dict(size=1200, seconds=20, direction='forward'),
                              dict(kind='none'), POLICY)
            self.assertIn('baseline_measurement_valid', result['failed'])
            self.assertTrue(result['missing'])
            self.assertNotIn('steady_added_rtt', result['checks'])
            self.assertNotIn('forward_transient_delay', result['checks'])
            self.assertNotIn('forward_steady_added_loss', result['checks'])

    def test_low_rate_phases_cannot_hide_a_failed_final_high_rate_phase(self):
        counts = [2083]*5+[5208]*5+[10416]*5+[20833]*2+[0]*3
        row = dict(recovery=role_recovery(), invalid_reasons=[], loss_percent=[.5,None], steady_loss_percent=[0,None],
                   resources=role_resources(), sequence_seconds=[counts,[]], arrival_seconds=[counts,[]])
        result = evaluate(row,None,dict(size=1200,seconds=20,direction='forward',rate_steps=True),dict(kind='none'),POLICY)
        self.assertIn('forward_final_steady_rate',result['failed'])
        self.assertIn('forward_step_recovery',result['failed'])

    def test_revised_startup_policy_is_explicit_and_cannot_dilute_loss_with_a_long_run(self):
        startup = dict(cold_loss_max_percent_by_rtt_ms={'55':1,'60':1,'100':4},
                       rtt100_startup_loss_payload_bytes_max=20_000_000)
        row = dict(recovery=role_recovery(), invalid_reasons=[],loss_percent=[3,None],steady_loss_percent=[0,None],
                   resources=role_resources(),arrival_seconds=[[0]+[20833]*20,[]])
        args = dict(size=1200,seconds=20,direction='forward',rtt_ms=100)
        self.assertIn('forward_cold_loss',evaluate(row,None,args,dict(kind='none'),POLICY)['failed'])
        revised = evaluate(row,None,args,dict(kind='none'),POLICY,startup)
        self.assertTrue(revised['checks']['forward_cold_loss'])
        self.assertTrue(revised['checks']['forward_startup_loss_bytes'])
        self.assertTrue(revised['checks']['forward_startup_recovery'])
        args['seconds']=60
        self.assertFalse(evaluate(row,None,args,dict(kind='none'),POLICY,startup)['checks']['forward_startup_loss_bytes'])
        args.update(seconds=20,rtt_ms=55)
        self.assertIn('forward_cold_loss',evaluate(row,None,args,dict(kind='none'),POLICY,startup)['failed'])

    def test_missing_baseline_does_not_become_pass(self):
        row = dict(recovery=role_recovery(), invalid_reasons=[], loss_percent=[0, None],
                   steady_loss_percent=[0, None], resources=role_resources())
        result = evaluate(row, None, dict(size=1200, seconds=20, direction='forward'),
                          dict(kind='none'), POLICY)
        self.assertTrue(result['missing'])
        self.assertFalse(result['acceptance_complete'])

    def test_low_total_loss_does_not_hide_an_unexpected_reconnection(self):
        row = dict(binary='candidate',invalid_reasons=[],loss_percent=[0,None],
                   steady_loss_percent=[0,None],resources=role_resources(),
                   recovery={'client':dict(connection_attempts=2,watchdog_timeouts=0),'server':dict(connection_attempts=0,watchdog_timeouts=0)})
        result = evaluate(row,None,dict(size=1200,seconds=20,direction='forward'),dict(kind='none'),POLICY)
        self.assertIn('no_unexpected_recovery',result['failed'])


if __name__ == '__main__':
    unittest.main()
