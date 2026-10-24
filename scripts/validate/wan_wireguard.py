"""Ephemeral WireGuard peers inside the WAN runner's verified namespaces.

Keys live only in the private temporary fixture. No host interfaces, routes,
configuration, service or existing peer is read or changed.
"""
import subprocess
import time
import json

from network import ns, run


def prepare(network, fixture, ports):
    public = {}
    for role in ['client', 'server']:
        key = subprocess.run(['wg', 'genkey'], capture_output=True, check=True, timeout=5).stdout
        path = fixture / f'wan-wg-{role}.key'
        path.write_bytes(key)
        path.chmod(0o600)
        public[role] = subprocess.run(['wg', 'pubkey'], input=key, capture_output=True,
                                      check=True, timeout=5).stdout.decode().strip()
    for role, peer, host, remote in [('client', 'server', '10.77.0.2', '10.77.0.1'),
                                    ('server', 'client', '10.77.0.1', '10.77.0.2')]:
        pid = getattr(network, role)
        run(*ns(pid, 'ip', 'link', 'add', 'wanwg0', 'type', 'wireguard'))
        command = ['wg', 'set', 'wanwg0', 'listen-port', str(ports['target']),
                   'private-key', str(fixture / f'wan-wg-{role}.key'),
                   'peer', public[peer], 'allowed-ips', remote + '/32']
        if role == 'client':
            command += ['endpoint', f'127.0.0.1:{ports["client"]}']
        run(*ns(pid, *command))
        run(*ns(pid, 'ip', 'address', 'add', host + '/32', 'dev', 'wanwg0'))
        run(*ns(pid, 'ip', 'link', 'set', 'wanwg0', 'mtu', '1280', 'up'))
        run(*ns(pid, 'ip', 'route', 'add', remote + '/32', 'dev', 'wanwg0'))
    return f'10.77.0.1:{ports["client"]}'


def observe(network):
    result = {}
    for role in ['client', 'server']:
        pid = getattr(network, role)
        # Discard the public identity; retain numerical evidence only. Never use
        # `wg show dump` (which would expose private keys).
        handshake = run(*ns(pid, 'wg', 'show', 'wanwg0', 'latest-handshakes')).split()
        transfer = run(*ns(pid, 'wg', 'show', 'wanwg0', 'transfer')).split()
        if len(handshake) != 2 or len(transfer) != 3:
            raise RuntimeError('expected exactly one owned WireGuard peer')
        result[role] = dict(latest_handshake=int(handshake[1]),
                            received_bytes=int(transfer[1]), sent_bytes=int(transfer[2]))
    return result


def ready(network):
    """Protocol readiness only: no offered-load warmup or bandwidth probes."""
    start = time.monotonic()
    for _ in range(12):
        subprocess.run(ns(network.client, 'ping', '-n', '-I', '10.77.0.2',
                          '-c', '1', '-W', '1', '10.77.0.1'),
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=2, check=False)
        result = observe(network)
        if all(v['latest_handshake'] and v['received_bytes'] and v['sent_bytes'] for v in result.values()):
            topology = {}
            for role, local, remote in [('client', '10.77.0.2', '10.77.0.1'),
                                        ('server', '10.77.0.1', '10.77.0.2')]:
                pid = getattr(network, role)
                links = json.loads(run(*ns(pid, 'ip', '-j', 'link', 'show', 'dev', 'wanwg0')))
                routes = json.loads(run(*ns(pid, 'ip', '-j', 'route', 'get', remote, 'from', local)))
                validate_topology(links, routes)
                topology[role] = dict(mtu=links[0]['mtu'], route_device=routes[0]['dev'])
            return dict(seconds=time.monotonic() - start, method='bounded single ICMP probes; no loaded warmup', peers=result, topology=topology)
    raise RuntimeError('owned WireGuard peers failed protocol readiness')


def validate_topology(links, routes):
    if len(links) != 1 or links[0].get('mtu') != 1280:
        raise RuntimeError('WireGuard MTU observation mismatch')
    if len(routes) != 1 or routes[0].get('dev') != 'wanwg0':
        raise RuntimeError('inner application route bypasses owned WireGuard peer')


def close(network):
    for role in ['client', 'server']:
        # Called only for an explicitly created peer pair. Namespace teardown
        # remains the final cleanup if creation or deletion failed partially.
        run(*ns(getattr(network, role), 'ip', 'link', 'delete', 'wanwg0'))
