#!/usr/bin/env python3
"""Bounded WAN experiments in fresh runner-owned user/network namespaces.

Reuses the reference relay, generators, integrity and resource collector. Results
are diagnostics, not full WAN acceptance: interval/one-way and WireGuard gates
are reported separately and must not be inferred from aggregate loss.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import tarfile

from benchmark import ROOT, install_termination_handler, sha, write
from network import Network, SCENARIOS, ns, run
from reference import run_once, write_progress
from wan_loss_report import resource_errors, WAN_ROLE_CPU_CORES_MAX, RESOURCE_BOUND_SOURCE


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--binary', type=Path, default=ROOT / 'target/release/remora')
    p.add_argument('--control', type=Path)
    p.add_argument('--rtt-ms', type=int, choices=[55, 60, 100], default=55)
    p.add_argument('--seconds', type=int, choices=[6, 20, 60, 1800], default=20)
    p.add_argument('--size', type=int, choices=[1000, 1200], default=1200)
    p.add_argument('--direction', choices=['forward', 'reverse', 'simultaneous'], default='forward')
    p.add_argument('--controllers', nargs='+', choices=['cubic', 'bbr2_gcongestion'], default=['cubic', 'bbr2_gcongestion'])
    p.add_argument('--profiles', nargs='+', choices=['original', 'medium', 'large', 'maximum', 'bounded', 'bounded194', 'bounded195'], default=['original'])
    p.add_argument('--rate-steps', action='store_true', help='20/50/100/200 Mbit/s, equal phases, no loaded warmup')
    p.add_argument('--pacing-us', type=int, choices=[100, 1000], default=1000)
    p.add_argument('--baseline', action='store_true')
    p.add_argument('--wireguard', action='store_true', help='ephemeral WG1280 peers inside owned namespaces')
    p.add_argument('--repetitions', type=int, choices=[1, 3], default=3)
    p.add_argument('--fault', choices=['none', 'drop5', 'ack5', 'pause5', 'pause10'], default='none')
    p.add_argument('--worker-host-net-fd', type=int)
    p.add_argument('--worker-host-user-fd', type=int)
    return p


PROFILES = {'original': (32, 20), 'medium': (512, 100), 'large': (2048, 200), 'maximum': (8192, 200), 'bounded': (8192, 190), 'bounded194': (8192, 194), 'bounded195': (8192, 195)}


def timeline_invalid_reasons(tx, rx, result, seconds):
    reasons = []
    for endpoint, offered, received in [(tx,result['offered'][0],result['unique'][1]),
                                        (rx,result['offered'][1],result['unique'][0])]:
        window = min(seconds + 1, 30)  # this runner declares a 1000-ms drain
        if endpoint.get('timeline_bin_us') != 10000 or endpoint.get('timeline_seconds') != window:
            reasons.append('missing endpoint timeline declaration')
        for key, total in [('offered_10ms',offered),('arrival_10ms',received)]:
            bins = endpoint.get(key)
            if not isinstance(bins,list) or len(bins) != window*100 or any(type(n) is not int or n < 0 for n in bins):
                reasons.append('missing/invalid bounded endpoint timeline')
            elif sum(bins) > total or (seconds + 1 <= 30 and sum(bins) != total):
                reasons.append('endpoint timeline does not reconcile with packet counts')
    return reasons


def frozen_input_paths():
    inputs = set()
    for directory, pattern in [('src','*.rs'),('tools/traffic/src','*.rs'),
                               ('scripts/validate','*.py'),('scripts/validate','*.sh'),
                               ('benchmarks/policies','wan-loss*.json')]:
        inputs.update((ROOT/directory).rglob(pattern))
    inputs.update(ROOT/path for path in ['Cargo.toml','Cargo.lock','rust-toolchain.toml',
                                        'build.rs','dependencies/quiche.json',
                                        'tools/traffic/Cargo.toml','tools/traffic/Cargo.lock'])
    return sorted(inputs)


def validate_frozen_inputs(hashes):
    for name,digest in hashes.items():
        try:
            actual=sha(ROOT/name)
        except OSError:
            raise RuntimeError('frozen input disappeared after manifest') from None
        if actual != digest:
            raise RuntimeError('frozen input changed after manifest')


def snapshot_sources(output):
    """Retain public build/runner inputs even across later uncommitted edits."""
    inputs=frozen_input_paths()
    total = sum(p.stat().st_size for p in inputs)
    if total > 16*1024*1024:
        raise RuntimeError('public source snapshot exceeds retention bound')
    archive = output/'source-inputs.tar'
    with tarfile.open(archive,'x') as tar:
        for path in sorted(inputs):
            tar.add(path,arcname=str(path.relative_to(ROOT)),recursive=False)
    return sha(archive)


def invalid_reasons(result, tx, rx, sequences, collections, direction, rate, seconds, intervals=None):
    reasons = ['resources: ' + reason for reason in resource_errors(result)]
    if result['generator_errors'] or result['integrity_errors'] or result['coverage_lost'] or result['counter_overflow']:
        reasons.append('generator/integrity/diagnostic failure')
    if result.get('kernel_rx_overflow_complete') is not True:
        reasons.append('kernel socket drop telemetry incomplete or unknown')
    if result['generator_cpu_max'] >= 0.9:
        reasons.append('generator lacks CPU headroom')
    if abs(result['clock_drift_ms']) >= 10:
        reasons.append('clock mapping unstable')
    if rx.get('sessions') != 1:
        reasons.append('expected one discovered peer')
    for i in range(2):
        active = direction == 'simultaneous' or direction == ['forward','reverse'][i]
        expected = (sum(intervals) if intervals else rate * seconds) if active else 0
        if result['offered'][i] != expected or result['submitted'][i] != expected:
            reasons.append('required offered/submitted workload missing')
        if active:
            if len(sequences[i]) != seconds or sum(sequences[i]) != result['unique'][i]:
                reasons.append('missing/inconsistent sequence intervals')
            if collections[i].get('latency_samples') != result['unique'][i] or not result['unique'][i]:
                reasons.append('missing one-way samples')
            if collections[i].get('clock_errors', 0):
                reasons.append('negative one-way clock interval')
    return reasons


class Fault:
    def __init__(self, network, kind, direction):
        self.network, self.kind, self.direction = network, kind, direction
        self.fired = False
        self.metadata = {'kind': kind, 'triggered': False}

    def __call__(self, sample, start, owned):
        if self.fired or self.kind == 'none' or sample['time_ms'] < start + 6000:
            return
        self.fired = True
        self.metadata.update(triggered=True, offset_ms=sample['time_ms'] - start)
        if self.kind.startswith('pause'):
            # Only the owned receiving Remora process; generators keep running.
            proc = owned[0 if self.direction == 'forward' else 1].p
            t = time.monotonic_ns()
            os.kill(proc.pid, signal.SIGSTOP)
            try:
                time.sleep(int(self.kind[5:]) / 1000)
            finally:
                os.kill(proc.pid, signal.SIGCONT)
            self.metadata['observed_pause_ns'] = time.monotonic_ns() - t
        else:
            config = Path(owned[0].p.args[-1])
            import tomllib
            port = int(tomllib.loads(config.read_text())['server']['listen'][0].rsplit(':', 1)[1])
            forward_leg = self.direction != 'reverse'
            if self.kind == 'ack5':
                forward_leg = not forward_leg
                self.metadata['carrier'] = 'small encrypted reverse-path datagrams; ACK/control/data contents not inspected'
            pid = self.network.server if forward_leg else self.network.client
            port_field = 'dport' if forward_leg else 'sport'
            run(*ns(pid, 'nft', 'add', 'table', 'inet', 'remora_wan'))
            run(*ns(pid, 'nft', 'add', 'chain', 'inet', 'remora_wan', 'input',
                    '{ type filter hook input priority -10; policy accept; }'))
            run(*ns(pid, 'nft', 'add', 'rule', 'inet', 'remora_wan', 'input',
                    'udp', port_field, str(port), 'ip', 'length', '<' if self.kind == 'ack5' else '>', '400',
                    'numgen', 'inc', 'mod', '2147483647', '<', '5', 'counter', 'drop'))
            self.pid = pid

    def finish(self):
        if self.kind in ['drop5', 'ack5'] and self.fired:
            data = json.loads(run(*ns(self.pid, 'nft', '-j', 'list', 'table', 'inet', 'remora_wan')))
            counts = [x['counter']['packets'] for item in data['nftables']
                      for x in item.get('rule', {}).get('expr', []) if 'counter' in x]
            self.metadata['confirmed_drops'] = sum(counts)
            run(*ns(self.pid, 'nft', 'delete', 'table', 'inet', 'remora_wan'))
            if counts != [5]:
                raise RuntimeError('injection did not confirm exactly five drops')
        if self.kind != 'none' and not self.fired:
            raise RuntimeError('fault did not fire within measurement')
        return self.metadata


def worker(a):
    a.output.mkdir(parents=True, exist_ok=False)
    binaries = {'candidate': a.binary.resolve()}
    if a.control:
        binaries['control'] = a.control.resolve()
    tool = ROOT / 'tools/traffic/target/release/remora-traffic'
    cpus = sorted(os.sched_getaffinity(0))
    if len(cpus) < 4:
        raise RuntimeError('four logical CPUs required for isolated roles and collectors')
    distinct = []
    seen_cores = set()
    for cpu in cpus:
        topology = Path(f'/sys/devices/system/cpu/cpu{cpu}/topology')
        core = ((topology/'physical_package_id').read_text().strip(), (topology/'core_id').read_text().strip())
        if core not in seen_cores:
            distinct.append(cpu)
            seen_cores.add(core)
    if len(distinct) < 4:
        raise RuntimeError('four distinct physical cores required')
    affinity = {'client': [distinct[0]], 'server': [distinct[1]], 'tools': distinct[2:4]}
    policy = ROOT / 'benchmarks/policies/wan-loss.json'
    source_archive = snapshot_sources(a.output)
    frozen_inputs={str(p.relative_to(ROOT)):sha(p) for p in frozen_input_paths()}
    write(a.output / 'manifest.json', {
        'schema_version': 1, 'policy_sha256': sha(policy),
        'resource_bounds': dict(role_cpu_cores_max=WAN_ROLE_CPU_CORES_MAX, rss_max_bytes=json.loads(policy.read_text())['rss_max_bytes'], source=RESOURCE_BOUND_SOURCE),
        'source_archive_sha256': source_archive,
        'frozen_inputs_sha256': frozen_inputs,
        'environment': dict(kernel=os.uname().release,machine=os.uname().machine,
                            rustc=subprocess.check_output(['rustc','--version'],text=True).strip()),
        'policy': json.loads(policy.read_text()),
        'startup_policy': json.loads((ROOT/'benchmarks/policies/wan-loss-startup-v2.json').read_text()),
        'startup_policy_sha256': sha(ROOT/'benchmarks/policies/wan-loss-startup-v2.json'),
        'candidate_expansion_sha256': sha(ROOT/'benchmarks/policies/wan-loss-candidates-v2.json'),
        'candidate_refinement': json.loads((ROOT/'benchmarks/policies/wan-loss-candidates-v3.json').read_text()),
        'candidate_refinement_sha256': sha(ROOT/'benchmarks/policies/wan-loss-candidates-v3.json'),
        'binaries': {k: {'path': str(v), 'sha256': sha(v)} for k, v in binaries.items()},
        'traffic_sha256': sha(tool), 'runner_sha256': sha(Path(__file__)),
        'reference_runner_sha256': sha(ROOT / 'scripts/validate/reference.py'),
        'wireguard_runner_sha256': sha(ROOT / 'scripts/validate/wan_wireguard.py'),
        'traffic_source_sha256': {str(p.relative_to(ROOT)): sha(p) for p in sorted((ROOT/'tools/traffic/src').rglob('*.rs'))},
        'source_sha256': {str(p.relative_to(ROOT)): sha(p) for p in sorted((ROOT/'src').rglob('*.rs'))},
        'affinity': affinity, 'arguments': {k: str(v) if isinstance(v, Path) else v for k, v in vars(a).items() if not k.startswith('worker')},
        'acceptance_complete': False, 'limits': ['individual screen; full policy matrix required', 'not hardware-equivalent to the VPS', 'no deployment'],
    })
    SCENARIOS['wan'] = dict(rtt_ms=a.rtt_ms, loss_percent=0, mtu=1500, queue_packets=8192)
    n = Network(a.output, 'wan', a.worker_host_net_fd, a.worker_host_user_fd)
    rows = []
    try:
        n.reference_deadline = time.monotonic() + 5400
        n.expected_binaries = {str(v): sha(v) for v in binaries.values()}
        n.expected_binaries['tools/traffic/target/release/remora-traffic'] = sha(tool)
        write(a.output/'topology.json', n.snapshot())
        for repeat in range(a.repetitions):
            variants = ([('baseline', next(iter(binaries.values())))] if a.baseline else []) + list(binaries.items())
            if repeat % 2:
                variants.reverse()
            for label, binary in variants:
                for cc in (a.controllers[:1] if label == 'baseline' else a.controllers):
                    for name in (a.profiles[:1] if label == 'baseline' else a.profiles):
                        packets, residence = PROFILES[name]
                        profile = dict(io='mmsg', batch_max=32, cc=cc, diagnostics='info',
                                       generator_completion_guard_ms=0, generator_pacing_quantum_us=a.pacing_us,
                                       drain_ms=1000, session_queue_packets=packets,
                                       session_queue_bytes=min(8388608, packets*1400),
                                       application_residence_ms=residence, wireguard=a.wireguard)
                        steps = [int(value*1_000_000/(8*a.size)) for value in [20,50,100,200]] if a.rate_steps else None
                        intervals = [rate for rate in steps for _ in range(a.seconds//4)] if steps else None
                        if steps:
                            profile['generator_rate_steps_pps'] = steps
                        fault = Fault(n, 'none' if label == 'baseline' else a.fault, a.direction)
                        n.measurement_hook = fault
                        path = a.output / f'{repeat}-{label}-{cc}-{name}'
                        validate_frozen_inputs({**frozen_inputs,**n.expected_binaries})
                        result, actual_pps = run_once(n, path, profile, 'baseline' if label == 'baseline' else 'tunnel', 1, a.size,
                                                     a.direction, int(200_000_000/(8*a.size)), 0,
                                                     a.seconds, affinity, binary)
                        validate_frozen_inputs({**frozen_inputs,**n.expected_binaries})
                        write(path/'fault.json', fault.finish())
                        tx = json.loads((path/'generator-0.stdout.ndjson').read_text())
                        rx = json.loads((path/'receiver.stdout.ndjson').read_text())
                        write(path/'timeline.json',dict(
                            bin_us=10000,window_seconds=min(a.seconds+1,30),
                            scope='bounded endpoint prefix; QUIC state remains sampled once/second',
                            forward=dict(offered=tx.get('offered_10ms'),received=rx.get('arrival_10ms')),
                            reverse=dict(offered=rx.get('offered_10ms'),received=tx.get('arrival_10ms')),
                            same_sender_rtt_max_us=tx.get('rtt_max_10ms_us'),
                        ))
                        sequences = [rx.get('forward_sequence_seconds', {}).get('1', []), tx.get('reverse_sequence_seconds', [])]
                        collections = [rx['forward_received'].get('1', {}), tx['reverse_received']]
                        steady_loss = []
                        one_way = []
                        for index in range(2):
                            if not result['submitted'][index]:
                                steady_loss.append(None); one_way.append(None); continue
                            expected = sum(intervals[2:]) if intervals else actual_pps * (a.seconds-2)
                            steady_loss.append(100*(expected-sum(sequences[index][2:]))/expected if sequences[index] else None)
                            one_way.append(collections[index].get('one_way_delay_max_us'))
                        loss = [(100*(s-u)/s if s else None) for s,u in zip(result['submitted'],result['unique'])]
                        row = dict(repeat=repeat, binary=label, cc=cc, profile=name, loss_percent=loss, steady_loss_percent=steady_loss, one_way_delay_max_us=one_way, arrival_seconds=[rx.get('arrival_seconds'), tx.get('arrival_seconds')],
                                   recovery={role: data.get('recovery') for role,data in json.loads((path/'diagnostics.json').read_text()).items()},
                                   offered_pps_intervals=intervals or [actual_pps]*a.seconds, sequence_seconds=sequences,
                                   one_way_delay_min_us=[c.get('one_way_delay_min_us') for c in collections],
                                   steady_rtt_p95_ms=tx['steady_rtt_p95_us']/1000 if tx.get('steady_rtt_p95_us') is not None else None,
                                   steady_rtt_samples=tx.get('steady_rtt_samples', 0),
                                   delivered_mbit_s=[u*a.size*8/a.seconds/1e6 for u in result['unique']],
                                   generator_errors=result['generator_errors'], generator_cpu_max=result['generator_cpu_max'], clock_drift_ms=result['clock_drift_ms'], integrity_errors=result['integrity_errors'],
                                   coverage_lost=result['coverage_lost'], kernel_rx_overflow_complete=result['kernel_rx_overflow_complete'], resources=result['resources'],
                                   rtt_p95_ms=result['rtt_p95_ms'], path=str(path))
                        reasons = invalid_reasons(result, tx, rx, sequences, collections, a.direction, actual_pps, a.seconds, intervals)
                        reasons += timeline_invalid_reasons(tx,rx,result,a.seconds)
                        if a.wireguard:
                            peers = json.loads((path/'wireguard.json').read_text())
                            if any(not v['latest_handshake'] or not v['received_bytes'] or not v['sent_bytes'] for v in peers.values()):
                                reasons.append('missing WireGuard handshake or bidirectional transfer')
                        row['invalid_reasons'] = reasons
                        row['status'] = 'invalid' if reasons else 'diagnostic_only'
                        rows.append(row)
                        write_progress(a.output, {'rows': rows, 'acceptance_complete': False})
                        print(json.dumps(row), flush=True)
        validate_frozen_inputs({**frozen_inputs,**n.expected_binaries})
        write(a.output/'summary.json', {'rows': rows, 'acceptance_complete': False})
    finally:
        n.close()


def main():
    install_termination_handler()
    a = parser().parse_args()
    if a.fault != 'none' and a.seconds < 20:
        raise ValueError('fault scenarios require at least 20 seconds')
    if a.rate_steps and (a.seconds == 6 or a.fault != 'none'):
        raise ValueError('rate-step screen requires complete equal phases and no separate fault')
    if a.seconds == 1800 and a.repetitions != 1:
        raise ValueError('bounded soak is one full 1800-second interval')
    if a.worker_host_net_fd is None:
        fds = [os.open('/proc/self/ns/'+k, os.O_RDONLY) for k in ['net','user']]
        try:
            args = [*sys.argv[1:], '--worker-host-net-fd', str(fds[0]), '--worker-host-user-fd', str(fds[1])]
            r = subprocess.run(['unshare','--user','--map-root-user','--net','--',sys.executable,str(Path(__file__).resolve()),*args],pass_fds=fds)
            raise SystemExit(r.returncode)
        finally:
            for fd in fds:
                os.close(fd)
    worker(a)


if __name__ == '__main__':
    main()
