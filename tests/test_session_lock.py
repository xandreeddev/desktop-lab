"""A failed native locker must leave time inside the sleep inhibitor for fallback."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]


class LockDeadline(unittest.TestCase):
    def test_failed_startup_reserves_time_for_stock_locker(self):
        spec=importlib.util.spec_from_file_location('lock_request',ROOT/'scripts/lucent-lock.py')
        module=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        now=[10.0]
        def sleep(seconds): now[0]+=seconds
        def call(*args,timeout=1):
            self.assertLessEqual(timeout,3.)
            sleep(min(timeout,.1))
            return 'is-active' in args
        class ReplacedProcess(Exception): pass
        with patch.object(module.sys,'argv',['lock.py','sleep','3000']), \
             patch.object(module.time,'monotonic',side_effect=lambda:now[0]), \
             patch.object(module.time,'sleep',side_effect=sleep), \
             patch.object(module,'call',side_effect=call), \
             patch.object(module.os,'execv',side_effect=ReplacedProcess) as execute:
            with self.assertRaises(ReplacedProcess): module.main()
        executable,argv=execute.call_args.args
        self.assertEqual(executable,'/usr/bin/omarchy-system-sleep-lock')
        self.assertEqual(argv[0],executable)
        self.assertGreaterEqual(int(argv[1]),900)
        self.assertLessEqual(now[0]-10+int(argv[1])/1000,3.)

    def test_native_failure_uses_independent_recovery_and_never_stock_ui(self):
        import tempfile
        spec=importlib.util.spec_from_file_location('native_lock_request',ROOT/'scripts/lucent-lock.py')
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        now=[10.0];emergency=[False]
        def sleep(seconds):now[0]+=seconds
        def call(*args,timeout=1):
            sleep(min(timeout,.1))
            return 'is-active' in args or (args[0]=='omarchy-hyprland-session-locked' and emergency[0])
        def launch(argv,**_):
            self.assertEqual(argv[:4],['hyprlock','--grace','0','--immediate-render'])
            emergency[0]=True
        with tempfile.TemporaryDirectory() as temporary:
            home=Path(temporary);state=home/'.local/state/lucent'
            (state/'integration-backup').mkdir(parents=True);(state/'integration-backup/native-shell-enabled').touch()
            (state/'theme').mkdir();(state/'theme/hyprlock.conf').write_text('fixture')
            with patch.object(module.Path,'home',return_value=home),patch.object(module.sys,'argv',['lock.py','sleep','3000']), \
                 patch.object(module.time,'monotonic',side_effect=lambda:now[0]),patch.object(module.time,'sleep',side_effect=sleep), \
                 patch.object(module,'call',side_effect=call),patch.object(module.subprocess,'Popen',side_effect=launch), \
                 patch.object(module.os,'execv') as stock:
                module.main()
                stock.assert_not_called()
            self.assertTrue(emergency[0])
            self.assertLessEqual(now[0]-10,3.)
