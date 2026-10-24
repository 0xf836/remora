#!/usr/bin/env python3
"""Prospectively frozen focused code/profile comparison in owned namespaces."""
import argparse
import copy
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time
import tomllib

from benchmark import ROOT, install_termination_handler, sha, write
from network import Network
from reference import run_once, calibration_invalid_reasons, write_progress
from wan_loss import snapshot_sources, frozen_input_paths, validate_frozen_inputs


def shape_errors(measurement,sessions,expected):
    if not isinstance(measurement,dict):return ['missing measurement']
    required={'generator_errors','integrity_errors','generator_cpu_max','clock_drift_ms',
              'coverage_lost','counter_overflow','offered','submitted','unique','unique_per_session',
              'rtt_samples_per_session','rtt_p95_ms','resources'}
    if not required <= measurement.keys():return ['missing mandatory measurement fields']
    errors=[]
    def finite(value):return type(value) in [int,float] and math.isfinite(value)
    for key in ['generator_errors','integrity_errors','coverage_lost']:
        if type(measurement[key]) is not int or measurement[key] < 0:errors.append('invalid counters')
    for key in ['generator_cpu_max','clock_drift_ms','rtt_p95_ms']:
        if not finite(measurement[key]):errors.append('nonfinite measurement')
    if finite(measurement['generator_cpu_max']) and measurement['generator_cpu_max']<0:
        errors.append('negative generator CPU')
    if finite(measurement['rtt_p95_ms']) and measurement['rtt_p95_ms']<=0:
        errors.append('missing positive RTT measurement')
    if type(measurement['counter_overflow']) is not bool:errors.append('missing overflow evidence')
    for key in ['offered','submitted','unique']:
        values=measurement[key]
        if not isinstance(values,list) or len(values)!=2 or any(type(n) is not int or n<0 or n>expected for n in values):
            errors.append('invalid directional counts')
    flows=measurement['unique_per_session']
    if not isinstance(flows,list) or len(flows)!=sessions or any(not isinstance(flow,list) or len(flow)!=2 or any(type(n) is not int or not 0<n<=expected//sessions for n in flow) for flow in flows):
        errors.append('missing/invalid per-session delivery')
    elif 'invalid directional counts' not in errors and any(sum(flow[d] for flow in flows)!=measurement['unique'][d] for d in range(2)):
        errors.append('per-session delivery does not reconcile')
    samples=measurement['rtt_samples_per_session']
    if not isinstance(samples,list) or len(samples)!=sessions or any(type(n) is not int or n<100 for n in samples):
        errors.append('missing per-session RTT evidence')
    resources=measurement['resources']
    if not isinstance(resources,list) or len(resources)!=2:
        errors.append('missing role resource evidence')
    else:
        for resource in resources:
            keys={'average_cpu_cores','owner_average_cpu_cores','peak_rss_bytes'}
            if not isinstance(resource,dict) or not keys<=resource.keys() or any(not finite(resource[key]) or resource[key]<0 for key in keys):
                errors.append('invalid/nonfinite role resources')
            elif type(resource['peak_rss_bytes']) is not int or resource['peak_rss_bytes']==0:
                errors.append('missing role RSS evidence')
    return errors


def loss(measurement, direction):
    return 100*(measurement['submitted'][direction]-measurement['unique'][direction])/measurement['submitted'][direction]


def evaluate(current, workload, policy):
    name, sessions, size, pps, seconds = workload
    reasons = []
    expected = (pps//sessions)*sessions*seconds
    if set(current) != {'direct','control','code','selected'}:
        return dict(passed=False,invalid_reasons=['missing comparison arms'],failed=[],scope='focused only')
    for label, measurement in current.items():
        malformed=shape_errors(measurement,sessions,expected)
        if malformed:
            reasons += [label+':'+reason for reason in malformed]
            continue
        reasons += [label+':'+reason for reason in calibration_invalid_reasons(measurement)]
        if abs(measurement['clock_drift_ms']) >= 10:
            reasons.append(label+':unstable clock mapping')
        if measurement['offered'] != [expected,expected] or measurement['submitted'] != [expected,expected]:
            reasons.append(label+':independent offered/submitted workload mismatch')
        if len(measurement['rtt_samples_per_session']) != sessions or min(measurement['rtt_samples_per_session'],default=0) < 100:
            reasons.append(label+':missing per-session RTT evidence')
    if reasons:
        return dict(passed=False, invalid_reasons=reasons, failed=[], scope='focused only')
    direct, control = current['direct'], current['control']
    failed = []
    for direction in range(2):
        if loss(direct,direction) > policy['baseline_loss_max_percent']:
            failed.append(f'unsustainable direct:{direction}')
    for label in ['code','selected']:
        candidate = current[label]
        for direction in range(2):
            if name != 'capacity' and loss(candidate,direction)-loss(direct,direction) > policy['added_loss_max_pp']:
                failed.append(f'{label}:SC-004:{direction}')
            if loss(candidate,direction)-loss(control,direction) > policy['paired_added_loss_max_pp']:
                failed.append(f'{label}:paired loss:{direction}')
            if candidate['unique'][direction] < control['unique'][direction]*policy['paired_delivery_min_ratio']:
                failed.append(f'{label}:paired delivery:{direction}')
            if name == 'capacity' and candidate['unique'][direction] < direct['unique'][direction]*policy['capacity_ratio_min']:
                failed.append(f'{label}:SC-003:{direction}')
            if name == 'resource' and candidate['unique'][direction]*size*8/seconds/1e6 < policy['resource_delivery_mbit_s_each_min']:
                failed.append(f'{label}:SC-007 delivery:{direction}')
        if name != 'capacity' and candidate['rtt_p95_ms']-direct['rtt_p95_ms'] > policy['subcapacity_added_rtt_p95_max_ms']:
            failed.append(label+':SC-005')
        for role, resource in zip(['client','server'],candidate['resources']):
            if resource['average_cpu_cores'] > policy['role_cpu_cores_max'] or resource['peak_rss_bytes'] > policy['role_rss_bytes_max']:
                failed.append(label+':'+role+':SC-007 resource bound')
    code = current['code']
    cpu_observations = []
    if code['rtt_p95_ms']-control['rtt_p95_ms'] > policy['paired_added_rtt_p95_max_ms']:
        failed.append('code:paired RTT')
    for role, previous, candidate in zip(['client','server'],control['resources'],code['resources']):
        for metric in ['average_cpu_cores','owner_average_cpu_cores']:
            limit = previous[metric]*policy['paired_cpu_max_ratio']+policy['paired_cpu_absolute_margin']
            cpu_observations.append(dict(role=role,metric=metric,previous=previous[metric],current=candidate[metric],delta_cores=candidate[metric]-previous[metric],original_limit=limit))
            if policy.get('paired_cpu_informational') is not True and candidate[metric] > limit:
                failed.append('code:'+role+':paired '+metric)
        if candidate['peak_rss_bytes'] > max(previous['peak_rss_bytes']*policy['paired_rss_max_ratio'],previous['peak_rss_bytes']+policy['paired_rss_margin_bytes']):
            failed.append('code:'+role+':paired RSS')
    return dict(passed=not failed,invalid_reasons=[],failed=failed,cpu_observations=cpu_observations,paired_cpu_gated=policy.get('paired_cpu_informational') is not True,scope='focused only')


def main():
    install_termination_handler()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--control',type=Path,required=True)
    p.add_argument('--candidate',type=Path,required=True)
    p.add_argument('--policy',type=Path,default=ROOT/'benchmarks/policies/wan-loss-regression.json')
    p.add_argument('--worker-host-net-fd',type=int)
    p.add_argument('--worker-host-user-fd',type=int)
    a = p.parse_args()
    if a.worker_host_net_fd is None:
        fds=[os.open('/proc/self/ns/'+kind,os.O_RDONLY) for kind in ['net','user']]
        try:
            raise SystemExit(subprocess.run(['unshare','--user','--map-root-user','--net','--',sys.executable,str(Path(__file__).resolve()),*sys.argv[1:],
                                            '--worker-host-net-fd',str(fds[0]),'--worker-host-user-fd',str(fds[1])],pass_fds=fds).returncode)
        finally:
            for fd in fds:os.close(fd)
    a.output.mkdir(parents=True,exist_ok=False)
    policy_path=a.policy
    policy=json.loads(policy_path.read_text())
    policy_hash=sha(policy_path)
    profile=tomllib.loads((ROOT/'benchmarks/reference-x86_64.toml').read_text())
    profile.update(cc='cubic',drain_ms=policy['drain_ms'])
    cpus=sorted(os.sched_getaffinity(0))
    if len(cpus)<10:raise RuntimeError('existing reference methodology requires >=10 logical CPUs')
    affinity=dict(client=cpus[:4],server=cpus[4:8],tools=cpus[8:])
    binaries={label:path.resolve() for label,path in [('control',a.control),('candidate',a.candidate)]}
    frozen={str(path):sha(path) for path in binaries.values()}
    frozen['tools/traffic/target/release/remora-traffic']=sha(ROOT/'tools/traffic/target/release/remora-traffic')
    archive=snapshot_sources(a.output)
    source_hashes={str(path.relative_to(ROOT)):sha(path) for path in frozen_input_paths()}
    started=time.monotonic()
    n=Network(a.output,'reference',a.worker_host_net_fd,a.worker_host_user_fd)
    rows,checks=[],[]
    def validate_inputs():
        if sha(policy_path)!=policy_hash:raise RuntimeError('policy drift after freeze')
        validate_frozen_inputs({**source_hashes,**frozen})
    try:
        n.reference_deadline=started+policy['wall_budget_seconds']-20
        n.expected_binaries=frozen
        write(a.output/'manifest.json',dict(policy=policy,policy_sha256=policy_hash,binaries=frozen,source_sha256=source_hashes,
              source_archive_sha256=archive,affinity=affinity,profile=profile,topology=n.snapshot(),release_qualification=False,
              rustc=subprocess.check_output(['rustc','--version'],text=True).strip()))
        for repeat in range(policy['repetitions']):
            for workload in policy['workloads']:
                name,sessions,size,pps,seconds=workload
                current={}
                labels=['direct']+(['control','code','selected'] if repeat%2==0 else ['selected','code','control'])
                for label in labels:
                    validate_inputs()
                    configuration=copy.deepcopy(profile)
                    configuration.update(policy['selected_profile'] if label=='selected' else policy['core_profile'])
                    path=a.output/f'{repeat}-{name}-{label}'
                    measurement,_=run_once(n,path,configuration,'baseline' if label=='direct' else 'tunnel',sessions,size,'simultaneous',pps,
                                           policy['warmup_seconds'],seconds,affinity,binaries['control' if label=='control' else 'candidate'])
                    validate_inputs()
                    current[label]=measurement
                    rows.append(dict(repeat=repeat,workload=name,label=label,path=str(path),measurement=measurement))
                    write_progress(a.output,dict(rows=rows,checks=checks,complete=False,release_qualification=False))
                    print(json.dumps(dict(repeat=repeat,workload=name,label=label,loss_percent=[loss(measurement,d) if measurement['submitted'][d] else None for d in range(2)])),flush=True)
                check=dict(repeat=repeat,workload=name,**evaluate(current,workload,policy))
                checks.append(check)
                write_progress(a.output,dict(rows=rows,checks=checks,complete=False,release_qualification=False))
                if not check['passed']:
                    raise RuntimeError('focused frozen criteria failed; preserve original evidence')
        validate_inputs()
        write(a.output/'summary.json',dict(rows=rows,checks=checks,complete=True,passed=True,release_qualification=False))
    except BaseException as error:
        write(a.output/'failure.json',dict(reason=type(error).__name__,rows=len(rows),checks=checks,release_qualification=False))
        raise
    finally:
        n.close()


if __name__=='__main__':main()
