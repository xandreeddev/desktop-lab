import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ci_ubuntu_sources", ROOT / "scripts/ci-ubuntu-sources.py")
sources = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sources)


def stanza(uri=sources.MIRROR, suites="resolute resolute-updates resolute-backports"):
    return ("# Ubuntu archive\nTypes: deb deb-src\n"
            f"URIs: {uri}\nSuites: {suites}\n"
            "Components: main restricted universe multiverse\n"
            "Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n"
            "Architectures: amd64\nCheck-Valid-Until: yes\n")


class UbuntuSources(unittest.TestCase):
    def test_runner_mirror_reference_preserves_each_stanzas_non_uri_fields(self):
        regular = stanza()
        security = stanza(suites="resolute-security")
        before = regular + "\n" + security
        expected = (regular.replace(sources.MIRROR, sources.ARCHIVE)
                    + "\n" + security.replace(sources.MIRROR, sources.SECURITY))
        self.assertEqual(sources.transform(before), expected)

    def test_all_supported_official_and_cloud_uris_preserve_suite_endpoint(self):
        for uri in sources.KNOWN_URIS:
            for suites, target in [("noble noble-updates", sources.ARCHIVE),
                                   ("noble-security", sources.SECURITY)]:
                with self.subTest(uri=uri, suites=suites):
                    self.assertEqual(sources.transform(stanza(uri, suites)), stanza(target, suites))

    def test_already_normalized_sources_are_idempotent(self):
        before = stanza(sources.ARCHIVE) + "\n" + stanza(sources.SECURITY, "resolute-security")
        self.assertEqual(sources.transform(before), before)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ubuntu.sources"
            path.write_text(before)
            stamp = path.stat().st_mtime_ns
            self.assertFalse(sources.normalize(path))
            self.assertEqual(path.stat().st_mtime_ns, stamp)

    def test_multiline_uris_and_suites_preserve_comments_and_embedded_signing_key(self):
        before = ("Types: deb\nURIs:\n " + sources.MIRROR
                  + "\n# Keep this comment\n " + sources.ARCHIVE
                  + "\nSuites: resolute\n resolute-updates\nComponents: main universe\n"
                  "Signed-By:\n -----BEGIN PGP PUBLIC KEY BLOCK-----\n .\n fixture-key\n"
                  " -----END PGP PUBLIC KEY BLOCK-----\n")
        expected = before.replace("URIs:\n " + sources.MIRROR, "URIs:" + sources.ARCHIVE)
        expected = expected.replace("\n " + sources.ARCHIVE + "\nSuites:", "\nSuites:")
        self.assertEqual(sources.transform(before), expected)

    def test_unrelated_disabled_stanzas_and_comments_are_unchanged(self):
        other = stanza("https://example.invalid/packages", "custom")
        disabled = "Enabled: No\n" + stanza(suites="resolute resolute-security")
        before = other + "\n\n" + disabled + "\n# " + sources.MIRROR + "\n\n" + stanza()
        self.assertEqual(sources.transform(before), before[:-len(stanza())] + stanza(sources.ARCHIVE))

    def test_whitespace_line_endings_and_absent_final_newline_are_preserved(self):
        before = stanza().replace("\n", "\r\n").rstrip("\r\n")
        self.assertEqual(sources.transform(before), before.replace(sources.MIRROR, sources.ARCHIVE))

    def test_apt_disabled_values_remain_unchanged_and_do_not_count_as_active(self):
        for value in ["no", "false", "without", "off", "disable", "0"]:
            with self.subTest(value=value):
                disabled = f"Enabled: {value.upper()}\n" + stanza(suites="resolute resolute-security")
                self.assertEqual(sources.transform(disabled + "\n" + stanza()),
                                 disabled + "\n" + stanza(sources.ARCHIVE))
                with self.assertRaisesRegex(ValueError, "No active recognized"):
                    sources.transform(disabled)

    def test_ambiguous_and_malformed_sources_fail_before_any_write(self):
        invalid = [
            stanza(suites="resolute resolute-security"),
            stanza(sources.MIRROR + " https://example.invalid/packages"),
            stanza().replace("Suites: resolute resolute-updates resolute-backports", "Suites:"),
            stanza(suites="./"),
            stanza().replace("Types: deb deb-src", "Types: unknown"),
            stanza() + "URIs: " + sources.MIRROR + "\n",
            stanza().replace("Components: main restricted universe multiverse\n", ""),
            " continuation\n" + stanza(),
            "Enabled: unsupported\n" + stanza(),
            "Enabled: 0x0\n" + stanza(),
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ubuntu.sources"
            for bad in invalid:
                with self.subTest(bad=bad):
                    before = (stanza() + "\n" + bad).encode()
                    path.write_bytes(before)
                    with self.assertRaises(ValueError):
                        sources.normalize(path)
                    self.assertEqual(path.read_bytes(), before)

    def test_owned_file_rewrite_preserves_mode_and_does_not_read_mirror_list(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ubuntu.sources"
            path.write_text(stanza())
            path.chmod(0o640)
            self.assertTrue(sources.normalize(path))
            self.assertEqual(path.read_text(), stanza(sources.ARCHIVE))
            self.assertEqual(path.stat().st_mode & 0o777, 0o640)

    def test_unknown_uri_spoofs_and_no_active_ubuntu_sources_fail_closed(self):
        for uri in ["https://archive.ubuntu.com.invalid/ubuntu/",
                    "https://archive.ubuntu.com@invalid/ubuntu/",
                    "mirror+file:/tmp/apt-mirrors.txt"]:
            with self.subTest(uri=uri), self.assertRaises(ValueError):
                sources.transform(stanza(uri))
        with self.assertRaises(ValueError):
            sources.transform("Enabled: no\n" + stanza())

    def test_symlink_input_is_rejected_without_changing_target(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "original"
            target.write_text(stanza())
            path = Path(directory) / "ubuntu.sources"
            path.symlink_to(target)
            with self.assertRaises(ValueError):
                sources.normalize(path)
            self.assertEqual(target.read_text(), stanza())

    def test_workflow_command_changes_only_the_explicit_owned_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ubuntu.sources"
            path.write_text(stanza() + "\n" + stanza(suites="resolute-security"))
            command = [sys.executable, str(ROOT / "scripts/ci-ubuntu-sources.py"),
                       "--sources", str(path)]
            first = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertIn("sources selected", first.stdout)
            self.assertEqual(path.read_text(), stanza(sources.ARCHIVE) + "\n"
                             + stanza(sources.SECURITY, "resolute-security"))
            again = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertIn("already selected", again.stdout)


if __name__ == "__main__":
    unittest.main()
