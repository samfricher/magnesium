"""Sentinel regressions for disposable native-command environment isolation."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from ci.isolated_environment import isolated_environment


PRIVATE_DIRECTORY_KEYS = (
    "HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME",
    "APPDATA", "LOCALAPPDATA", "TMPDIR", "TEMP", "TMP", "PATH",
)


class IsolatedEnvironmentTests(unittest.TestCase):
    def test_every_platform_directory_is_created_inside_the_private_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = (Path(directory) / "fixture").resolve()
            hostile = {key: "/original-profile/" + key for key in PRIVATE_DIRECTORY_KEYS}
            environment = isolated_environment(root, hostile)
            for key in PRIVATE_DIRECTORY_KEYS:
                path = Path(environment[key])
                self.assertTrue(path.is_dir(), key)
                self.assertTrue(path.is_relative_to(root), key)
                self.assertTrue(path.resolve().is_relative_to(root), key)
            self.assertEqual(environment["HOME"], environment["USERPROFILE"])
            self.assertEqual(environment["TMPDIR"], environment["TEMP"])
            self.assertEqual(environment["TEMP"], environment["TMP"])

    def test_provider_credentials_host_identity_and_runtime_overrides_are_not_inherited(self):
        with tempfile.TemporaryDirectory() as directory:
            hostile = {
                "CLAUDE_CODE_SESSION_ID": "claude-thread", "OPENAI_API_KEY": "synthetic-secret",
                "ANTHROPIC_API_KEY": "synthetic-secret", "NODE_OPTIONS": "--import=evil.js",
                "PYTHONPATH": "/original-profile/python", "MAGNESIUM_WORKSPACE_ROOT": "/original-work",
                "MAGNESIUM_DOCS_WORKER": "/original-profile/worker", "MAGNESIUM_DOCS_WORKER_OPT_IN": "1",
                "MAGNESIUM_HARNESS_EXECUTION_GRANT": "synthetic-grant", "MAGNESIUM_DATA_DIR": "/ignored",
                "hOmE": "/original-profile/home", "Path": "/original-profile/bin",
            }
            environment = isolated_environment(directory, hostile)
            for key in hostile:
                self.assertNotIn(key, environment)
            self.assertEqual(environment["TZ"], "UTC")

    def test_symlinked_temporary_root_keeps_every_private_path_inside_its_canonical_root(self):
        # macOS temporary paths may spell /private/var as /var. Exercise the
        # same alias on any platform with symlink support, rather than relying
        # on the machine's temporary-directory spelling.
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            canonical_parent = base / "private-var"
            canonical_parent.mkdir()
            alias = base / "var"
            try:
                alias.symlink_to(canonical_parent, target_is_directory=True)
            except (OSError, NotImplementedError) as error:
                self.skipTest(f"directory symlinks are unavailable: {error}")
            root = alias / "fixture"
            canonical_root = canonical_parent / "fixture"
            environment = isolated_environment(root, {})
            self.assertNotEqual(root, canonical_root)
            self.assertEqual(root.resolve(), canonical_root)
            for key in PRIVATE_DIRECTORY_KEYS:
                path = Path(environment[key])
                self.assertEqual(path, path.resolve(), key)
                self.assertTrue(path.is_relative_to(canonical_root), key)
                self.assertTrue(path.resolve().is_relative_to(root.resolve()), key)
                self.assertTrue(path.is_dir(), key)

    def test_windows_bootstrap_variables_are_case_normalized_without_copying_the_source(self):
        with tempfile.TemporaryDirectory() as directory:
            source = {"SYSTEMROOT": "C:\\Windows", "windir": "C:\\Windows", "TOKEN": "synthetic"}
            original = source.copy()
            environment = isolated_environment(directory, source)
            self.assertEqual(source, original)
            self.assertEqual(environment["SystemRoot"], "C:\\Windows")
            self.assertEqual(environment["WINDIR"], "C:\\Windows")
            self.assertNotIn("TOKEN", environment)

    def test_fake_initialization_cannot_discover_or_write_an_original_host_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            original = base / "original-profile"
            original.mkdir()
            sentinel = original / "sentinel.txt"
            sentinel.write_text("original profile is unchanged", encoding="utf-8")
            host = original / "claude"
            host.write_text("synthetic host, must never execute", encoding="utf-8")
            host.chmod(0o755)
            before = {p.name: p.read_bytes() for p in original.iterdir()}
            source = {key: str(original) for key in PRIVATE_DIRECTORY_KEYS}
            for key in ("SystemRoot", "WINDIR"):
                if key in os.environ:
                    source[key] = os.environ[key]
            environment = isolated_environment(base / "fixture", source)
            fake_magnesium = base / "fake-magnesium.py"
            fake_magnesium.write_text(
                "import json, os, pathlib, shutil\n"
                "assert shutil.which('claude') is None\n"
                "for key in ('HOME','USERPROFILE','XDG_CONFIG_HOME','XDG_DATA_HOME',"
                "'XDG_CACHE_HOME','APPDATA','LOCALAPPDATA','TMPDIR','TEMP','TMP'):\n"
                "    (pathlib.Path(os.environ[key]) / ('fake-init-' + key)).write_text('fixture')\n"
                "print(json.dumps(dict(os.environ)))\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [sys.executable, str(fake_magnesium), "init"], env=environment,
                cwd=base / "fixture", text=True, capture_output=True, timeout=10, check=True,
            )
            effective = json.loads(result.stdout)
            for key in PRIVATE_DIRECTORY_KEYS:
                self.assertEqual(effective[key], environment[key])
            self.assertEqual(before, {p.name: p.read_bytes() for p in original.iterdir()})


if __name__ == "__main__":
    unittest.main()
