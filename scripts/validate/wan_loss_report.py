#!/usr/bin/env python3
"""Evaluate individual WAN screen gates without claiming full qualification.

Only complete original-sequence seconds measure steady loss. Recovery uses
complete arrival seconds after the actual injection, so a partially observed
second can never make recovery appear earlier. Delay comparison needs a paired
unimpaired baseline; missing evidence is reported explicitly.
"""
import argparse
import json
import math
import hashlib
from pathlib import Path



# Existing SC-007 process budget; a 200-Mbit/s WAN screen does not qualify
# the full 32-session/500-Mbit/s SC-007 workload.
WAN_ROLE_CPU_CORES_MAX = 2.0
RESOURCE_BOUND_SOURCE = 'existing SC-007; WAN screen only'


def resource_errors(row):
    values = row.get('resources') if isinstance(row, dict) else None
    if not isinstance(values, list) or len(values) != 2:
        return ['two client/server role resource records required']
    errors = []
    for role, value in zip(['client', 'server'], values):
        if not isinstance(value, dict):
            errors.append(role + ': missing resource record')
            continue
        for field in ['average_cpu_cores', 'owner_average_cpu_cores']:
            number = value.get(field)
            if type(number) not in [int, float] or not math.isfinite(number) or number < 0:
                errors.append(role + ': missing/nonfinite CPU evidence')
        rss = value.get('peak_rss_bytes')
        if type(rss) is not int or rss <= 0:
            errors.append(role + ': missing/invalid RSS evidence')
    return errors


def recovery_errors(row):
    values = row.get('recovery')
    if not isinstance(values, dict) or set(values) != {'client', 'server'}:
        return ['client/server recovery records required']
    errors = []
    for role, value in values.items():
        if not isinstance(value, dict):
            errors.append(role + ': missing recovery record')
            continue
        for field in ['connection_attempts', 'watchdog_timeouts']:
            number = value.get(field)
            if type(number) is not int or number < 0:
                errors.append(role + ': missing/invalid recovery counter')
    return errors


def recovery(arrivals, size, fault_ms, policy):
    first = math.ceil(fault_ms / 1000)
    last = math.floor((fault_ms + policy['recovery_deadline_seconds'] * 1000) / 1000)
    hold = policy['recovery_hold_seconds']
    for start in range(first, last + 1):
        window = arrivals[start:start + hold]
        if len(window) == hold and all(n * size * 8 / 1e6 >= policy['recovery_delivered_mbit_s_min'] for n in window):
            return (start * 1000 - fault_ms) / 1000
    return None


def evaluate(row, baseline, arguments, fault, policy, startup_policy=None):
    checks = {}
    missing = []
    not_applicable = []
    resources_invalid = resource_errors(row)
    checks['measurement_valid'] = not row['invalid_reasons'] and not resources_invalid
    missing += ['candidate resources: ' + reason for reason in resources_invalid]
    baseline_resources_invalid = resource_errors(baseline) if baseline is not None else []
    baseline_valid = baseline is not None and not baseline.get('invalid_reasons', ['missing validity evidence']) and not baseline_resources_invalid
    missing += ['baseline resources: ' + reason for reason in baseline_resources_invalid]
    if baseline is not None:
        checks['baseline_measurement_valid'] = baseline_valid
    size, seconds = int(arguments['size']), int(arguments['seconds'])
    direction = arguments['direction']
    cold_limit = (startup_policy['cold_loss_max_percent_by_rtt_ms'][str(arguments['rtt_ms'])]
                  if startup_policy else policy['cold_loss_max_percent'])
    for index, name in enumerate(['forward', 'reverse']):
        if direction not in [name, 'simultaneous']:
            continue
        cold, steady = row['loss_percent'][index], row['steady_loss_percent'][index]
        checks[name + '_cold_loss'] = cold is not None and cold <= cold_limit
        if startup_policy and int(arguments['rtt_ms']) == 100:
            expected = sum(row.get('offered_pps_intervals', [int(200_000_000/(8*size))]*seconds))
            checks[name + '_startup_loss_bytes'] = cold is not None and cold/100*expected*size <= startup_policy['rtt100_startup_loss_payload_bytes_max']
            if arguments.get('rate_steps', False):
                not_applicable.append(name + ': 190-Mbit/s recovery is checked at the final 200-Mbit/s step, not the initial 20-Mbit/s phase')
            else:
                arrivals = row.get('arrival_seconds', [None, None])[index]
                checks[name + '_startup_recovery'] = arrivals is not None and recovery(arrivals[:seconds], size, 0, policy) is not None
        # Derive rate from the declared offered packet slots, not rounded Mbps.
        nominal = int(200_000_000 / (8 * size)) * size * 8 / 1e6
        if arguments.get('rate_steps', False):
            final_start = seconds * 3 // 4
            intervals = row.get('sequence_seconds', [[], []])[index]
            final = intervals[final_start + 2:seconds]
            checks[name + '_final_steady_rate'] = len(final) == seconds // 4 - 2 and bool(final) and sum(final)*size*8/(len(final)*1e6) >= policy['steady_delivered_mbit_s_min']
            checks[name + '_step_recovery'] = recovery(row['arrival_seconds'][index][:seconds], size, final_start*1000, policy) is not None
        else:
            checks[name + '_steady_rate'] = steady is not None and nominal * (1 - steady / 100) >= policy['steady_delivered_mbit_s_min']
        if not baseline_valid:
            missing.append(name + ': valid paired baseline for added loss/delay')
        else:
            base_steady = baseline['steady_loss_percent'][index]
            checks[name + '_steady_added_loss'] = steady is not None and base_steady is not None and steady - base_steady <= policy['steady_added_loss_max_pp']
            # This screen has only maxima. Subtracting maxima is diagnostic and
            # cannot prove a per-packet added-delay bound; require baseline min.
            minimum = baseline.get('one_way_delay_min_us', [None, None])[index]
            maximum = row['one_way_delay_max_us'][index]
            if minimum is None or maximum is None:
                missing.append(name + ': minimum baseline one-way delay')
            else:
                checks[name + '_transient_delay'] = maximum - minimum <= policy['transient_added_one_way_delay_max_ms'] * 1000
        if fault['kind'] != 'none':
            checks['fault_triggered'] = fault.get('triggered', False)
            if fault['kind'] in ['drop5', 'ack5']:
                checks['fault_exact_count'] = fault.get('confirmed_drops') == policy['injected_outer_data_packet_drops']
            arrival = row['arrival_seconds'][index]
            checks[name + '_recovery'] = arrival is not None and recovery(arrival[:seconds], size, fault['offset_ms'], policy) is not None
    # Whole-run RTT percentiles are not a substitute for steady RTT samples.
    if baseline_valid and row.get('steady_rtt_p95_ms') is not None and baseline.get('steady_rtt_p95_ms') is not None:
        checks['steady_added_rtt'] = row['steady_rtt_p95_ms'] - baseline['steady_rtt_p95_ms'] <= policy['steady_added_rtt_p95_max_ms']
    else:
        missing.append('steady RTT percentile')
    if not resources_invalid:
        checks['rss'] = all(v['peak_rss_bytes'] <= policy['rss_max_bytes'] for v in row['resources'])
        checks['cpu'] = all(v['average_cpu_cores'] <= WAN_ROLE_CPU_CORES_MAX for v in row['resources'])
    if row.get('binary') != 'baseline':
        recovery_invalid = recovery_errors(row)
        checks['recovery_evidence'] = not recovery_invalid
        missing += ['candidate recovery: ' + reason for reason in recovery_invalid]
        if not recovery_invalid:
            checks['no_unexpected_recovery'] = all(
                data['connection_attempts'] <= 1 and data['watchdog_timeouts'] == 0
                for data in row['recovery'].values())
    return {'resource_bounds': {'role_cpu_cores_max': WAN_ROLE_CPU_CORES_MAX, 'rss_max_bytes': policy['rss_max_bytes'], 'source': RESOURCE_BOUND_SOURCE}, 'checks': checks, 'failed': [k for k, v in checks.items() if not v],
            'missing': missing, 'not_applicable': not_applicable, 'cold_loss_limit_percent': cold_limit, 'acceptance_complete': False}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('run', type=Path)
    a = p.parse_args()
    root = Path(__file__).resolve().parents[2]
    manifest = json.loads((a.run / 'manifest.json').read_text())
    if 'policy' in manifest:
        policy = manifest['policy']
    else:
        raw = (root / 'benchmarks/policies/wan-loss.json').read_bytes()
        if hashlib.sha256(raw).hexdigest() != manifest['policy_sha256']:
            raise ValueError('original policy changed after measurement; cannot reinterpret old evidence')
        policy = json.loads(raw)
    startup_policy = manifest.get('startup_policy')
    arguments = manifest['arguments']
    rows = json.loads((a.run / 'summary.json').read_text())['rows']
    reports = []
    for row in rows:
        if row['binary'] == 'baseline':
            continue
        baseline = next((v for v in rows if v['binary'] == 'baseline' and v['repeat'] == row['repeat']), None)
        fault = json.loads((Path(row['path']) / 'fault.json').read_text())
        reports.append(dict(path=row['path'], **evaluate(row, baseline, arguments, fault, policy, startup_policy)))
    print(json.dumps({'reports': reports, 'acceptance_complete': False}, indent=2))


if __name__ == '__main__':
    main()
