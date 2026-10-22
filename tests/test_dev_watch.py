"""Local filesystem notifications and serial latest-demand behavior; no services."""
import importlib.util
from contextlib import nullcontext
import io
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from lucent_dev.model import DevError
from lucent_dev.watch import Debounce, Watcher, included, source_digest

spec = importlib.util.spec_from_file_location('dev_cli_for_watch', ROOT / 'scripts/lucent-dev.py')
cli = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cli)


class WatchTests(unittest.TestCase):
    def test_scope_excludes_build_output_docs_and_goldens(self):
        for path in ('lucent/crates/a/src/lib.rs', 'design/tokens.json', 'configs/lucent/palettes.json',
                     'scripts/generate-app-icons.py'):
            self.assertTrue(included(path), path)
        for path in ('lucent/target/a.rs', 'docs/a.md', 'site/src/a.astro', 'reports/a.json',
                     '.cache/a', '.git/index', 'lucent/tests/visual/baselines/a.png', 'scripts/other.py'):
            self.assertFalse(included(path), path)

    def test_atomic_save_and_new_directory_are_observed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'lucent/src'
            source.mkdir(parents=True)
            (root / 'scripts').mkdir()
            path = source / 'lib.rs'
            path.write_text('old')
            with Watcher(root) as watcher:
                temporary = source / 'save.tmp'
                temporary.write_text('new')
                temporary.replace(path)
                self.assertIn('lucent/src/lib.rs', watcher.read())
                nested = source / 'new'
                nested.mkdir()
                (nested / 'first.rs').write_text('first')
                self.assertIn('lucent/src/new', watcher.read())
                (nested / 'first.rs').write_text('second')
                self.assertIn('lucent/src/new/first.rs', watcher.read())

    def test_target_and_document_edits_do_not_wake_source_demand(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for path in ('lucent/src', 'lucent/target', 'docs', 'scripts'):
                (root / path).mkdir(parents=True)
            with Watcher(root) as watcher:
                (root / 'lucent/target/output').write_text('binary')
                (root / 'docs/a.md').write_text('documentation')
                self.assertEqual(watcher.read(), set())

    def test_content_hash_detects_same_size_edit_with_preserved_mtime(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lucent').mkdir()
            path = root / 'lucent/a.rs'
            path.write_text('one')
            before = path.stat()
            original = source_digest(root)
            path.write_text('two')
            os.utime(path, ns=(before.st_atime_ns, before.st_mtime_ns))
            self.assertNotEqual(source_digest(root), original)

    def test_symlinked_source_directories_and_fifo_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lucent').mkdir()
            (root / 'external').mkdir()
            link = root / 'lucent/link'
            link.symlink_to(root / 'external', target_is_directory=True)
            with self.assertRaises(DevError):
                source_digest(root)
            with self.assertRaises(DevError):
                Watcher(root)
            link.unlink()
            os.mkfifo(root / 'lucent/fifo')
            with self.assertRaises(DevError):
                source_digest(root)

    def test_edits_during_build_form_one_debounced_latest_request(self):
        demand = Debounce(.35)
        self.assertTrue(demand.ready(0.))
        demand.begin()  # The sole active build has consumed its request.
        self.assertFalse(demand.ready(1.))
        demand.changed(1.)
        demand.changed(1.2)
        demand.changed(1.3)
        self.assertFalse(demand.ready(1.64))
        self.assertTrue(demand.ready(1.66))
        demand.begin()
        self.assertFalse(demand.ready(2.))

    def test_symlinked_ancestors_of_inputs_and_generator_are_rejected(self):
        for ancestor, child in (('configs', 'lucent/palettes.json'), ('scripts', 'generate-app-icons.py')):
            with self.subTest(ancestor=ancestor), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                external = root / 'external'
                path = external / child
                path.parent.mkdir(parents=True)
                path.write_text('input')
                (root / ancestor).symlink_to(external, target_is_directory=True)
                with self.assertRaises(DevError):
                    source_digest(root)
                with self.assertRaises(DevError):
                    Watcher(root)


class WatchControlTests(unittest.TestCase):
    def run_watch(self, actions, results, application):
        clock = [0.]
        version = [0]
        jobs = []
        active = [0]
        maximum = [0]
        class FakeWatcher:
            changed = False
            def __init__(self, *_): pass
            def __enter__(self): return self
            def __exit__(self, *_): pass
            def read(self):
                changed, self.changed = self.changed, False
                return {'changed.rs'} if changed else set()
        watcher = FakeWatcher()
        class FakeSelector:
            def __enter__(self): return self
            def __exit__(self, *_): pass
            def register(self, *_): pass
            def unregister(self, *_): pass
            def select(self, timeout):
                clock[0] += 1.
                if not actions:
                    raise KeyboardInterrupt
                action = actions.pop(0)
                if action == 'source':
                    version[0] += 1
                    watcher.changed = True
                return [(SimpleNamespace(data=action), 1)] if action else []
        class FakeJob:
            def __init__(self, *_):
                self.unit = 'owned-build'
                self.process = Mock()
                self.cancelled = False
                self.result = results.pop(0)
                jobs.append(self)
                active[0] += 1
                maximum[0] = max(maximum[0], active[0])
            def finish(self):
                if isinstance(self.result, Exception): raise self.result
                return self.result
            def close(self): active[0] -= 1
            def cancel(self): self.cancelled = True
        args = SimpleNamespace(debounce=.35, profile='release', ready_timeout=45.)
        with patch.object(cli, 'paths', return_value=(Path('/source'), Path('/target'), Path('/bundles'))), \
                patch.object(cli, 'Watcher', return_value=watcher), \
                patch.object(cli.selectors, 'DefaultSelector', FakeSelector), \
                patch.object(cli.build, 'BuildJob', FakeJob), \
                patch.object(cli, 'fresh', side_effect=lambda result, *_: result['inputs'] == version[0]), \
                patch.object(cli.time, 'monotonic', side_effect=lambda: clock[0]), \
                patch.object(cli.deploy, 'preflight', return_value={}), \
                patch.object(cli.deploy, 'apply_bundle', side_effect=application) as apply, \
                patch('sys.stdout', new_callable=io.StringIO), patch('sys.stderr', new_callable=io.StringIO):
            try:
                cli.watch(args)
            except (KeyboardInterrupt, cli.deploy.Interrupted) as error:
                return jobs, maximum[0], apply.call_args_list, error
        self.fail('watch should run until an explicit interruption')

    def test_edit_during_blocked_build_discards_stale_result_and_builds_once_more(self):
        jobs, maximum, calls, _ = self.run_watch(
            [None, 'source', 'build', None, 'build'],
            [{'bundle': '/stale', 'inputs': 0}, {'bundle': '/latest', 'inputs': 1}],
            lambda *_args, **_kwargs: {'success': True})
        self.assertEqual(maximum, 1)
        self.assertEqual(len(jobs), 2)
        self.assertEqual([call.args[0] for call in calls], [Path('/latest')])

    def test_compiler_failure_does_not_call_apply_or_retry_without_an_edit(self):
        jobs, maximum, calls, _ = self.run_watch([None, 'build', None], [DevError('compile failed')], lambda *_: None)
        self.assertEqual(len(jobs), 1)
        self.assertEqual(maximum, 1)
        self.assertEqual(calls, [])

    def test_deferred_bundle_is_retried_without_a_new_build(self):
        attempts = [cli.deploy.Deferred('locked'), {'success': True}]
        def application(*_args, **_kwargs):
            value = attempts.pop(0)
            if isinstance(value, Exception): raise value
            return value
        jobs, _, calls, _ = self.run_watch([None, 'build', None, None], [{'bundle': '/latest', 'inputs': 0}], application)
        self.assertEqual(len(jobs), 1)
        self.assertEqual([call.args[0] for call in calls], [Path('/latest'), Path('/latest')])

    def test_new_edit_supersedes_a_deferred_bundle(self):
        attempts = [cli.deploy.Deferred('locked'), {'success': True}]
        def application(*_args, **_kwargs):
            value = attempts.pop(0)
            if isinstance(value, Exception): raise value
            return value
        jobs, maximum, calls, _ = self.run_watch(
            [None, 'build', 'source', None, 'build'],
            [{'bundle': '/older', 'inputs': 0}, {'bundle': '/newest', 'inputs': 1}], application)
        self.assertEqual(maximum, 1)
        self.assertEqual(len(jobs), 2)
        self.assertEqual([call.args[0] for call in calls], [Path('/older'), Path('/newest')])

    def test_apply_interrupt_escapes_watch_and_main_returns_signal_status(self):
        error = cli.deploy.Interrupted(2)
        def application(*_args, **_kwargs): raise error
        _, _, calls, observed = self.run_watch([None, 'build', None], [{'bundle': '/latest', 'inputs': 0}], application)
        self.assertIs(observed, error)
        self.assertEqual(len(calls), 1)
        with patch.object(cli, 'mutation_lease', return_value=nullcontext()), \
                patch.object(cli, 'watch', side_effect=error), patch('sys.stderr', new_callable=io.StringIO):
            self.assertEqual(cli.main(['watch']), 130)

    def test_ctrl_c_during_build_cancels_only_that_active_job(self):
        jobs, maximum, calls, observed = self.run_watch([None], [{'bundle': '/unfinished', 'inputs': 0}], lambda *_: None)
        self.assertIsInstance(observed, KeyboardInterrupt)
        self.assertEqual(maximum, 1)
        self.assertTrue(jobs[0].cancelled)
        self.assertEqual(calls, [])

    def test_doctor_and_apply_do_not_require_source_checkout_or_cargo(self):
        with patch.object(cli.deploy, 'preflight', return_value={'ready': True}), \
                patch.object(cli.deploy, 'apply_bundle', return_value={'success': True}), \
                patch.object(cli, 'load_bundle'), patch.object(cli, 'paths') as paths, \
                patch.object(cli, 'mutation_lease', return_value=nullcontext()), patch('sys.stdout', new_callable=io.StringIO):
            self.assertEqual(cli.main(['doctor', '--root', '/not-a-checkout']), 0)
            self.assertEqual(cli.main(['apply', '/bundle', '--root', '/not-a-checkout']), 0)
            paths.assert_not_called()


if __name__ == '__main__':
    unittest.main()
