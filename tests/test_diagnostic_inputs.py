"""Candidate construction refuses stale outputs and unsealed source proposals."""
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts/validate'))
import diagnostic_inputs as b
from benchmark import sha, write


class CandidateConstruction(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name); self.inputs = root / 'inputs'; self.inputs.mkdir()
        self.source = root / 'source'; self.target = root / 'target'; self.policy = root / 'policy.json'
        contents = {name: name.encode() for name in ['src/lib.rs', 'Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain.toml']}
        import hashlib
        sources = {name: hashlib.sha256(value).hexdigest() for name, value in contents.items()}
        for name in ['phase6.tar', 'phase7-proposed.tar']:
            with tarfile.open(self.inputs / name, 'w') as archive:
                for path, value in contents.items():
                    info = tarfile.TarInfo(path); info.size = len(value); archive.addfile(info, io.BytesIO(value))
        proposal = {'base_revision': 'a' * 40, 'phase6_archive_sha256': sha(self.inputs / 'phase6.tar'),
            'phase7_archive_sha256': sha(self.inputs / 'phase7-proposed.tar'), 'proposed_sources': sources}
        write(self.inputs / 'proposal.json', proposal)
        write(self.policy, {'frozen': True, 'proposal_sha256': sha(self.inputs / 'proposal.json'), 'proposal': proposal, 'created_utc_seconds': time.time() - 1})

    def test_build_is_after_seal_and_exact_sources_are_recorded(self):
        def compile(command, check):
            self.assertTrue(check); self.assertIn('--locked', command); self.assertIn('--offline', command)
            (self.target / 'release').mkdir(parents=True); (self.target / 'release/remora').write_bytes(b'executable')
        with patch.object(b.subprocess, 'run', side_effect=compile), patch.object(b.subprocess, 'check_output', return_value='pinned-version\n'):
            identity = b.build_candidate(self.inputs, self.policy, self.source, self.target)
        self.assertEqual(identity['policy_sha256'], sha(self.policy))
        self.assertEqual(identity['binary_sha256'], sha(self.inputs / 'phase7-remora'))
        self.assertLess(b.read(self.policy)['created_utc_seconds'], identity['materialized_utc_seconds'])
        self.assertLessEqual(identity['materialized_utc_seconds'], identity['built_utc_seconds'])
        self.assertEqual(identity['sources'], b.read(self.inputs / 'proposal.json')['proposed_sources'])

    def test_prepare_baseline_records_exact_revision_and_preserves_proposal(self):
        inputs = self.inputs.parent / 'fresh-inputs'
        source = self.inputs.parent / 'fresh-source'
        target = self.inputs.parent / 'fresh-target'
        traffic = self.inputs.parent / 'traffic'; traffic.write_bytes(b'generator')
        archive = (self.inputs / 'phase6.tar').read_bytes()
        def output(command, **kwargs):
            return archive if command[:2] == ['git', 'archive'] else 'pinned-version\n'
        def compile(command, check):
            (target / 'release').mkdir(parents=True); (target / 'release/remora').write_bytes(b'baseline')
        with patch.object(b.subprocess, 'run', side_effect=compile), patch.object(b.subprocess, 'check_output', side_effect=output):
            identity = b.prepare_baseline(inputs, 'a' * 40, source, target, traffic)
        self.assertEqual(identity['revision'], 'a' * 40)
        self.assertEqual(identity['features'], ['qlog'])
        self.assertEqual(b.read(inputs / 'proposal.json')['base_revision'], 'a' * 40)
        self.assertEqual(identity['archive_sha256'], sha(inputs / 'phase6.tar'))
        self.assertEqual(sha(inputs / 'traffic'), sha(traffic))
        with self.assertRaises(ValueError): b.prepare_baseline(inputs, 'a' * 40, source, target, traffic)

    def test_changed_archive_or_existing_source_cannot_be_built(self):
        self.source.mkdir()
        with self.assertRaises(ValueError): b.build_candidate(self.inputs, self.policy, self.source, self.target)
        self.source.rmdir()
        (self.inputs / 'phase7-proposed.tar').write_bytes(b'changed')
        with self.assertRaises(ValueError): b.build_candidate(self.inputs, self.policy, self.source, self.target)
        self.assertFalse(self.source.exists())


class SourceArchiveExtraction(unittest.TestCase):
    def test_regular_files_keep_content_and_executable_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode='w') as archive:
                member = tarfile.TarInfo('scripts/check.sh')
                member.mode = 0o4755
                member.size = 4
                archive.addfile(member, io.BytesIO(b'test'))
            data.seek(0)
            with tarfile.open(fileobj=data) as archive:
                b.extract_sources(archive, root)
            self.assertEqual((root / 'scripts/check.sh').read_bytes(), b'test')
            self.assertEqual((root / 'scripts/check.sh').stat().st_mode & 0o7777, 0o755)

    def test_traversal_absolute_links_and_devices_are_rejected_before_writing(self):
        for name, kind in [('..', tarfile.DIRTYPE), ('../escape', tarfile.REGTYPE),
                           ('/escape', tarfile.REGTYPE), ('link', tarfile.SYMTYPE),
                           ('link', tarfile.LNKTYPE), ('device', tarfile.CHRTYPE),
                           ('pipe', tarfile.FIFOTYPE)]:
            with self.subTest(name=name, kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory) / 'source'; root.mkdir()
                data = io.BytesIO()
                with tarfile.open(fileobj=data, mode='w') as archive:
                    safe = tarfile.TarInfo('safe.rs'); safe.size = 4
                    archive.addfile(safe, io.BytesIO(b'safe'))
                    unsafe = tarfile.TarInfo(name); unsafe.type = kind; unsafe.linkname = '../escape'
                    archive.addfile(unsafe)
                data.seek(0)
                with tarfile.open(fileobj=data) as archive, self.assertRaises(ValueError):
                    b.extract_sources(archive, root)
                self.assertEqual(list(root.iterdir()), [])

    def test_existing_symlink_cannot_redirect_an_overlay(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'source'; root.mkdir()
            outside = Path(directory) / 'outside'; outside.mkdir()
            (root / 'redirect').symlink_to(outside, target_is_directory=True)
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode='w') as archive:
                member = tarfile.TarInfo('redirect/escape'); member.size = 4
                archive.addfile(member, io.BytesIO(b'test'))
            data.seek(0)
            with tarfile.open(fileobj=data) as archive, self.assertRaises(ValueError):
                b.extract_sources(archive, root)
            self.assertEqual(list(outside.iterdir()), [])


if __name__ == '__main__': unittest.main()
