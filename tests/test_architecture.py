"""Guard package and component boundaries, including target/build dependencies.

Resolve Cargo dependency aliases and workspace inheritance without fetching
packages. This checks every declared target, rather than only the host platform.
The graph is local to this workspace; it is not a Rust semantic/import analyzer.
"""
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
KINDS = ('dependencies', 'build-dependencies', 'dev-dependencies')


def dependency_names(manifest, workspace):
    """Yield actual package names for all dependency kinds and target tables."""
    tables = [manifest, *manifest.get('target', {}).values()]
    for table in tables:
        for kind in KINDS:
            for alias, spec in table.get(kind, {}).items():
                spec = {'version': spec} if isinstance(spec, str) else spec.copy()
                if spec.get('workspace'):
                    inherited = workspace.get('dependencies', {})[alias]
                    inherited = {'version': inherited} if isinstance(inherited, str) else inherited
                    spec = {**inherited, **spec}
                yield spec.get('package', alias)


def workspace_manifests(root):
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    paths = {p / 'Cargo.toml' for pattern in workspace['members'] for p in root.glob(pattern)}
    excluded = {p / 'Cargo.toml' for pattern in workspace.get('exclude', []) for p in root.glob(pattern)}
    crates = {}
    for path in sorted(paths - excluded):
        data = tomllib.loads(path.read_text())
        crates[data['package']['name']] = set(dependency_names(data, workspace))
    return crates


def reachable(graph, name, seen=None):
    seen = set() if seen is None else seen
    for dependency in (graph[name] & graph.keys()) - seen:
        seen.add(dependency)
        reachable(graph, dependency, seen)
    return seen


def sources(directory, excluded=()):
    return [p for p in directory.rglob('*.rs') if p.relative_to(directory).as_posix() not in excluded]


class BoundaryGuardFixtures(unittest.TestCase):
    def test_resolves_renamed_workspace_target_build_and_dev_dependencies(self):
        workspace = tomllib.loads('''
[dependencies]
renamed = { package = "lucent-services", path = "crates/lucent-services" }
''')
        manifest = tomllib.loads('''
[dependencies]
ordinary = "1"
[build-dependencies]
build_alias = { package = "lucent-auth", path = "../lucent-auth" }
[target.'cfg(target_os = "other")'.dependencies]
renamed = { workspace = true }
[target.'cfg(unix)'.dev-dependencies]
test_alias = { package = "lucent-desktop", path = "../lucent-desktop" }
''')
        self.assertEqual(set(dependency_names(manifest, workspace)),
                         {'ordinary', 'lucent-auth', 'lucent-services', 'lucent-desktop'})
        graph = {'lucent-api': set(dependency_names(manifest, workspace)),
                 'lucent-services': {'lucent-domain'}, 'lucent-domain': set(),
                 'lucent-auth': set(), 'lucent-desktop': set()}
        self.assertIn('lucent-domain', reachable(graph, 'lucent-api'))

    def test_nested_source_files_are_included(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            nested = root / 'component' / 'hidden.rs'
            nested.parent.mkdir()
            nested.write_text('use std::process::Command;')
            (root / 'platform.rs').write_text('')
            self.assertEqual(sources(root, ('platform.rs',)), [nested])


class Boundaries(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.graph = workspace_manifests(ROOT / 'lucent')

    def assert_no_platform_operations(self, paths, forbidden=()):
        self.assert_no_imports(paths, ('std::process', 'std::fs', 'std::net', 'std::os', *forbidden))

    def assert_no_imports(self, paths, forbidden):
        for path in paths:
            text = path.read_text()
            for token in forbidden:
                # Match whitespace around Rust path separators too.
                pattern = r'\s*::\s*'.join(map(re.escape, token.split('::')))
                self.assertIsNone(re.search(pattern, text), f'{path.relative_to(ROOT)} imports {token}')

    def test_domain_and_usecases_do_not_depend_on_platform_or_rendering(self):
        self.assertLessEqual(self.graph['lucent-domain'], {'serde', 'zeroize'})
        self.assertEqual(self.graph['lucent-usecases'], {'lucent-domain'})
        for crate in ('lucent-domain', 'lucent-usecases'):
            self.assert_no_platform_operations(sources(ROOT / 'lucent/crates' / crate / 'src'))

    def test_framework_has_no_indirect_client_service_or_design_dependencies(self):
        framework = {'lucent-api', 'lucent-ui', 'lucent-wayland', 'lucent-render'}
        for name in framework:
            self.assertLessEqual(reachable(self.graph, name), framework, name)
        self.assertEqual(reachable(self.graph, 'lucent-usecases'), {'lucent-domain'})

    def test_component_policy_does_not_choose_production_adapters(self):
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-desktop/src',
            ('main.rs', 'platform.rs', 'test_adapters.rs', 'visual_tests.rs')),
            ('lucent_services', 'lucent_auth'))
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-session/src',
            ('platform.rs', 'bin/lucent-lock.rs', 'bin/lucent-greeter.rs')),
            ('lucent_services', 'lucent_auth'))
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-menu/src',
            ('main.rs',)), ('lucent_services', 'lucent_auth'))

    def test_menu_definition_model_has_no_platform_dependencies(self):
        import ast
        tree = ast.parse((ROOT / 'scripts/menu_model.py').read_text())
        modules = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                modules.update(alias.name for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                modules.add(node.module)
        self.assertLessEqual(modules, {'json', 're'})

    def test_launcher_does_not_import_the_root_component_or_adapter_bundle(self):
        root = ROOT / 'lucent/apps/lucent-desktop/src'
        paths = sources(root / 'launcher') + [root / 'artwork.rs', root / 'actions.rs', root / 'widgets/catalog.rs']
        self.assert_no_platform_operations(paths, ('crate::desktop', 'crate::adapters', 'crate::platform'))

    def test_widget_host_and_views_do_not_import_the_root_or_adapters(self):
        paths = sources(ROOT / 'lucent/apps/lucent-desktop/src/widgets')
        self.assert_no_platform_operations(paths,
            ('crate::desktop', 'crate::adapters', 'crate::platform', 'crate::ports'))
        for path in paths:
            self.assertNotRegex(path.read_text(), r'\b(?:Desktop|DesktopAdapters)\b', str(path))

    def test_audio_child_depends_on_ports_without_the_root_or_adapter_bundle(self):
        root = ROOT / 'lucent/apps/lucent-desktop/src'
        paths = sources(root / 'audio') + [root / 'audio.rs']
        self.assert_no_platform_operations(paths, ('crate::desktop', 'crate::adapters', 'crate::platform', 'crate::ports'))
        for path in paths:
            self.assertNotRegex(path.read_text(), r'\b(?:Desktop|DesktopAdapters)\b', str(path))

    def test_network_adapter_does_not_use_process_fallbacks_or_client_components(self):
        paths = sources(ROOT / 'lucent/crates/lucent-services/src/network', ('tests.rs',))
        self.assert_no_platform_operations(paths,
            ('crate::command', 'command_with_timeout', 'std::process', 'lucent_desktop', 'lucent_design', 'lucent_wayland'))

    def test_network_child_uses_the_domain_port_without_root_or_adapter_selection(self):
        paths = sources(ROOT / 'lucent/apps/lucent-desktop/src/network')
        self.assert_no_platform_operations(paths, ('crate::desktop', 'crate::adapters', 'crate::platform', 'crate::ports'))

    def test_bluetooth_adapter_and_child_preserve_transport_and_client_boundaries(self):
        self.assert_no_platform_operations(
            sources(ROOT / 'lucent/crates/lucent-services/src/bluetooth', ('tests.rs','pairing_tests.rs')),
            ('crate::command','command_with_timeout','lucent_desktop','lucent_design','lucent_wayland'))
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-desktop/src/bluetooth'),
            ('crate::desktop','crate::adapters','crate::platform','crate::ports'))
        self.assertNotIn('lucent-wayland', reachable(self.graph,'bluetooth-service'))
        self.assertNotIn('lucent-render', reachable(self.graph,'bluetooth-service'))

    def test_power_adapter_child_and_consumer_preserve_boundaries(self):
        self.assert_no_imports(
            sources(ROOT / 'lucent/crates/lucent-services/src/power', ('tests.rs',)),
            ('crate::command', 'command_with_timeout', 'lucent_desktop', 'lucent_design', 'lucent_wayland'))
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-desktop/src/power'),
            ('crate::desktop', 'crate::adapters', 'crate::platform', 'crate::ports'))
        self.assertNotIn('lucent-wayland', reachable(self.graph, 'power-service'))
        self.assertNotIn('lucent-render', reachable(self.graph, 'power-service'))

    def test_tray_adapter_child_and_consumer_preserve_boundaries(self):
        self.assert_no_imports(sources(ROOT / 'lucent/crates/lucent-services/src/tray', ('tests.rs',)),
                               ('crate::command', 'command_with_timeout', 'lucent_desktop', 'lucent_design', 'lucent_wayland'))
        self.assert_no_platform_operations(sources(ROOT / 'lucent/apps/lucent-desktop/src/tray'),
                                           ('crate::desktop', 'crate::adapters', 'crate::platform'))
        self.assertNotIn('lucent-wayland', reachable(self.graph, 'tray-service'))
        self.assertNotIn('lucent-render', reachable(self.graph, 'tray-service'))

    def test_compositor_consumer_and_adapter_preserve_monitor_policy_boundaries(self):
        self.assertNotIn('lucent-wayland', reachable(self.graph, 'compositor-service'))
        self.assertNotIn('lucent-render', reachable(self.graph, 'compositor-service'))
        self.assert_no_imports(sources(ROOT/'lucent/crates/lucent-services/src/compositor'),
                               ('lucent_desktop','lucent_design','lucent_wayland'))

    def test_application_catalog_and_dock_preserve_boundaries(self):
        self.assert_no_platform_operations([ROOT / 'lucent/apps/lucent-desktop/src/catalog.rs', ROOT / 'lucent/apps/lucent-desktop/src/dock.rs'],
                                           ('crate::desktop', 'crate::adapters', 'crate::platform'))
        self.assertNotIn('lucent-wayland', reachable(self.graph, 'application-service'))
        self.assertNotIn('lucent-render', reachable(self.graph, 'application-service'))

    def test_text_model_and_example_preserve_framework_boundaries(self):
        self.assert_no_platform_operations([ROOT/'lucent/crates/lucent-api/src/text_edit.rs', ROOT/'lucent/crates/lucent-api/src/clipboard.rs'],
                                           ('lucent_domain', 'lucent_desktop', 'lucent_services', 'lucent_wayland', 'lucent_render'))
        self.assertNotIn('lucent-desktop', reachable(self.graph, 'text-input'))
        self.assertNotIn('lucent-services', reachable(self.graph, 'text-input'))
        for name in ('lucent-usecases','lucent-services','lucent-desktop'):
            self.assertNotIn(name,reachable(self.graph,'rich-text'))
        self.assert_no_platform_operations([ROOT/'lucent/crates/lucent-api/src/rich_text.rs',ROOT/'lucent/crates/lucent-ui/src/rich_text.rs'],
                                           ('lucent_domain','lucent_services','lucent_desktop','std::process','std::fs'))

    def test_clipboard_transport_stays_in_native_backend_and_secure_clients_do_not_opt_in(self):
        self.assert_no_imports(sources(ROOT/'lucent/crates/lucent-wayland/src/clipboard'),
                               ('std::process', 'lucent_services', 'lucent_domain', 'lucent_desktop'))
        for name in ('lucent-session', 'lucent-polkit'):
            for path in sources(ROOT/'lucent/apps'/name/'src'):
                self.assertNotIn('fn clipboard_enabled',path.read_text())
        self.assertIn('fn clipboard_enabled', (ROOT/'lucent/examples/text-input/src/main.rs').read_text())

    def test_notification_adapters_and_child_keep_storage_out_of_views(self):
        self.assert_no_imports(sources(ROOT/'lucent/crates/lucent-services/src/notifications',('tests.rs',)),
                               ('lucent_desktop','lucent_design','lucent_wayland','lucent_render'))
        self.assert_no_platform_operations(sources(ROOT/'lucent/apps/lucent-desktop/src/notifications'),
                                           ('crate::desktop','crate::platform','crate::adapters'))
        self.assertNotIn('lucent-wayland',reachable(self.graph,'notification-service'))
        self.assertNotIn('lucent-render',reachable(self.graph,'notification-service'))

    def test_ordinary_desktop_clients_use_relative_declarative_input(self):
        for path in sources(ROOT/'lucent/apps/lucent-desktop/src'):
            self.assertNotIn('Element::input(',path.read_text(),str(path.relative_to(ROOT)))

    def test_all_desktop_effects_fit_the_runtime_worker_budget(self):
        root=ROOT / 'lucent/apps/lucent-desktop/src'
        lanes=re.findall(r'pub const \w+: &str = "([^"]+)";', (root/'work.rs').read_text())
        limit=int(re.search(r'max_workers: (\d+),', (ROOT/'lucent/crates/lucent-wayland/src/effects.rs').read_text())[1])
        self.assertEqual(len(lanes),len(set(lanes)))
        self.assertLessEqual(len(lanes),limit)
        # Queue selection is client policy. New features must reuse/name one
        # of these bounded lanes, rather than silently starting another worker.
        for path in sources(root,('visual_tests.rs','test_adapters.rs')):
            self.assertNotRegex(path.read_text(),r'\.task\(|\.task_(?:cancellable_)?on(?:_with_failure)?\(\s*"',str(path.relative_to(ROOT)))
