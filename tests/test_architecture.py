"""Guard domain/framework boundaries against accidental platform coupling."""
from pathlib import Path
import tomllib
import unittest
ROOT=Path(__file__).resolve().parents[1]

class Boundaries(unittest.TestCase):
    def test_domain_does_not_depend_on_platform_or_rendering(self):
        cargo=tomllib.loads((ROOT/'lucent/crates/lucent-domain/Cargo.toml').read_text())
        self.assertLessEqual(set(cargo['dependencies']),{'serde','zeroize'})
        for source in (ROOT/'lucent/crates/lucent-domain/src').glob('*.rs'):
            self.assertNotIn('std::process',source.read_text())
            self.assertNotIn('std::fs',source.read_text())

    def test_framework_does_not_depend_on_the_desktop_or_services(self):
        for crate in ['lucent-api','lucent-ui','lucent-render','lucent-wayland']:
            cargo=tomllib.loads((ROOT/'lucent/crates'/crate/'Cargo.toml').read_text())
            self.assertFalse(set(cargo.get('dependencies',{})) & {'lucent-desktop','lucent-services','lucent-design','lucent-auth'})

    def test_local_dependency_graph_has_no_indirect_boundary_leaks(self):
        manifests = list((ROOT/'lucent').glob('crates/*/Cargo.toml')) + list((ROOT/'lucent').glob('apps/*/Cargo.toml'))
        crates = {data['package']['name']: data for path in manifests
                  for data in [tomllib.loads(path.read_text())]}
        graph = {name: set(data.get('dependencies', {})) & crates.keys() for name, data in crates.items()}
        def reachable(name, seen=None):
            seen = set() if seen is None else seen
            for dependency in graph[name] - seen:
                seen.add(dependency)
                reachable(dependency, seen)
            return seen
        self.assertEqual(reachable('lucent-usecases'), {'lucent-domain'})
        framework = {'lucent-api', 'lucent-ui', 'lucent-wayland', 'lucent-render'}
        for name in framework:
            self.assertLessEqual(reachable(name), framework, name)

    def test_component_policy_does_not_choose_production_adapters(self):
        for name in ['desktop.rs','views.rs','widgets.rs','notifications.rs']:
            text=(ROOT/'lucent/apps/lucent-desktop/src'/name).read_text()
            self.assertNotIn('lucent_services',text)
            self.assertNotIn('std::process',text)
        session=(ROOT/'lucent/apps/lucent-session/src/lib.rs').read_text()
        self.assertNotIn('lucent_auth',session)
        self.assertNotIn('std::process',session)
        menu=(ROOT/'lucent/apps/lucent-menu/src/lib.rs').read_text()
        self.assertNotIn('std::process',menu)
        self.assertNotIn('std::fs',menu)

    def test_menu_definition_model_has_no_platform_dependencies(self):
        import ast
        tree = ast.parse((ROOT/'scripts/menu_model.py').read_text())
        modules = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                modules.update(alias.name for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                modules.add(node.module)
        self.assertLessEqual(modules, {'json', 're'})
