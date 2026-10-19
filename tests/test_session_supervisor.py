"""Supervisor shutdown uses only children owned by each isolated test."""
import importlib.util
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
POPEN = subprocess.Popen
SELECT = select.select
CHILD = "import time; print('ready', flush=True); time.sleep(30)"


class SessionSupervisor(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        with patch.dict(os.environ, {'XDG_RUNTIME_DIR': str(self.home)}), \
             patch.object(sys, 'path', [str(ROOT / 'scripts'), *sys.path]):
            spec = importlib.util.spec_from_file_location('session_supervisor', ROOT / 'scripts/lucent-session.py')
            self.session = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(self.session)
        # Module constants, never the test runner's HOME or real desktop.
        self.session.HOME = self.home
        self.session.STOP_TIMEOUT = .1
        self.handlers = {}
        self.children = []

    def tearDown(self):
        for child in self.children:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=1)
            child.stdout.close()
            child.stderr.close()

    def register(self, number, handler):
        # Capture callbacks without changing handlers or signalling this process.
        if callable(handler):
            self.handlers[number] = handler
        return signal.SIG_DFL

    def launch(self, code, after_launch=None):
        def start(argv):
            self.assertEqual(argv, [str(self.home / '.local/bin/lucent-desktop')])
            child = POPEN([sys.executable, '-c', code], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.children.append(child)
            if after_launch:
                after_launch(child)
            return child
        return start

    def child_ready(self, child):
        self.assertTrue(SELECT([child.stdout], [], [], 3)[0], 'owned child did not start')
        self.assertEqual(child.stdout.readline(), b'ready\n')

    def run_with(self, code=CHILD, ready=None, after_launch=None):
        if ready is None:
            def ready(child, stopped):
                self.child_ready(child)
                return True
        with patch.object(self.session.signal, 'signal', side_effect=self.register), \
             patch.object(self.session.subprocess, 'Popen', side_effect=self.launch(code, after_launch)), \
             patch.object(self.session, 'ready', side_effect=ready):
            return self.session.run()

    def test_intentional_signals_wake_idle_wait_and_exit_successfully(self):
        for number in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signal=number):
                entered = threading.Event()
                errors = []
                def request():
                    try:
                        if not entered.wait(3):
                            raise AssertionError('supervisor did not enter its blocking wait')
                        self.handlers[number](number, None)
                    except BaseException as error:
                        errors.append(error)
                worker = threading.Thread(target=request)
                def ready(child, stopped):
                    self.child_ready(child)
                    worker.start()
                    return True
                def wait(*args):
                    entered.set()
                    return SELECT(*args)
                try:
                    with patch.object(self.session.select, 'select', side_effect=wait):
                        self.assertEqual(self.run_with(ready=ready), 0)
                finally:
                    worker.join(timeout=3)
                self.assertFalse(worker.is_alive())
                self.assertEqual(errors, [])
                self.assertEqual(self.children[-1].returncode, -signal.SIGTERM)

    def test_stop_during_readiness_is_distinct_from_readiness_failure(self):
        original = self.session.ready
        def ready(child, stopped):
            self.child_ready(child)
            def inspect():
                self.handlers[signal.SIGTERM](signal.SIGTERM, None)
                return {}
            with patch.object(self.session, 'inspect', side_effect=inspect), \
                 patch.object(self.session, 'desktop_rendered', return_value=False):
                return original(child, stopped)
        with patch.object(self.session.subprocess, 'run') as helpers:
            self.assertEqual(self.run_with(ready=ready), 0)
            helpers.assert_not_called()

    def test_repeated_stop_does_not_extend_bounded_kill_escalation(self):
        code = "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); print('ready', flush=True); time.sleep(30)"
        def ready(child, stopped):
            self.child_ready(child)
            for number in (signal.SIGTERM, signal.SIGINT, signal.SIGTERM):
                self.handlers[number](number, None)
            return not stopped()
        self.assertEqual(self.run_with(code, ready), 0)
        self.assertEqual(self.children[-1].returncode, -signal.SIGKILL)

    def test_later_stop_preserves_already_observed_failure_status(self):
        for status in (7, -signal.SIGTERM):
            with self.subTest(status=status):
                def ready(child, stopped):
                    self.child_ready(child)
                    if status < 0:
                        child.terminate()
                    self.assertEqual(child.wait(timeout=3), status)
                    self.handlers[signal.SIGTERM](signal.SIGTERM, None)
                    self.assertFalse(stopped())
                    return True
                code = CHILD if status < 0 else "print('ready', flush=True); raise SystemExit(7)"
                self.assertEqual(self.run_with(code, ready), status)

    def test_real_nonzero_exit_after_accepted_stop_is_not_normalized(self):
        code = "import signal,sys,time; signal.signal(signal.SIGTERM, lambda *_: sys.exit(9)); print('ready', flush=True); time.sleep(30)"
        def ready(child, stopped):
            self.child_ready(child)
            self.handlers[signal.SIGTERM](signal.SIGTERM, None)
            child.wait(timeout=3)
            return not stopped()
        self.assertEqual(self.run_with(code, ready), 9)

    def stop_during_cleanup(self, child):
        terminate = child.terminate
        def late_stop():
            self.handlers[signal.SIGTERM](signal.SIGTERM, None)
            terminate()
        child.terminate = late_stop

    def test_readiness_failure_survives_a_later_stop_during_cleanup(self):
        original = self.session.ready
        def ready(child, stopped):
            self.child_ready(child)
            with patch.object(self.session.time, 'monotonic', side_effect=[0., 31.]):
                return original(child, stopped)
        with self.assertRaisesRegex(RuntimeError, 'did not render'):
            self.run_with(ready=ready, after_launch=self.stop_during_cleanup)
        self.assertIsNotNone(self.children[-1].returncode)

    def test_helper_failure_survives_a_later_stop_during_cleanup(self):
        marker = self.home / '.local/state/lucent/bar-hidden'
        marker.parent.mkdir(parents=True)
        marker.touch()
        failure = subprocess.CalledProcessError(7, ['owned-fixture-helper'])
        with patch.object(self.session.subprocess, 'run', side_effect=failure), \
             self.assertRaises(subprocess.CalledProcessError) as raised:
            self.run_with(after_launch=self.stop_during_cleanup)
        self.assertIs(raised.exception, failure)
        self.assertIsNotNone(self.children[-1].returncode)

    def test_pidfd_setup_failure_still_reaps_owned_child(self):
        with patch.object(self.session.os, 'pidfd_open', side_effect=OSError('fixture pidfd failure')), \
             self.assertRaisesRegex(OSError, 'fixture pidfd failure'):
            self.run_with()
        self.assertIsNotNone(self.children[-1].returncode)
        self.assertEqual(self.handlers, {})


if __name__ == '__main__':
    unittest.main()
