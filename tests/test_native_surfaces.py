import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('native_surfaces', Path(__file__).resolve().parents[1]/'vm/native_surfaces.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


class NativeSurfaces(unittest.TestCase):
    def test_active_selection_preserves_actual_native_ids_and_shared_panels(self):
        state = {'client': {'named_monitor_hosts': True, 'active_monitor': 'logical-B',
                            'monitor_hosts': [{'id':'logical-A','instance':'instance-A'},
                                              {'id':'logical-B','instance':'instance-B'}]},
                 'surfaces':[{'id':'bar [instance-A]','role':'bar','instance':'instance-A'},
                             {'id':'bar [instance-B]','role':'bar','instance':'instance-B'},
                             {'id':'dock','role':'dock','instance':None}]}
        self.assertEqual(native.find_surface(state,'bar')['id'],'bar [instance-B]')
        self.assertEqual(len(native.active_surfaces(state)),2)
        self.assertEqual(len(state['surfaces']),3)
        self.assertEqual(native.find_surface(state,'dock')['id'],'dock')

    def test_legacy_id_is_a_role_without_parsing_a_diagnostic_instance(self):
        state={'surfaces':[{'id':'bar'},{'id':'dock'}]}
        self.assertEqual(native.find_surface(state,'bar'),{'id':'bar'})
        state['surfaces']=[{'id':'bar [DP-1]','role':'bar','instance':'DP-1'}]
        with self.assertRaises(LookupError): native.find_surface(state,'bar')

    def test_ambiguous_or_absent_active_host_is_not_guessed(self):
        for hosts in ([],[{'id':'logical','instance':None}],
                      [{'id':'logical','instance':'A'},{'id':'logical','instance':'B'}]):
            state={'client':{'named_monitor_hosts':True,'active_monitor':'logical','monitor_hosts':hosts},
                   'surfaces':[{'id':'bar','role':'bar'}]}
            self.assertEqual(native.active_surfaces(state),[])
