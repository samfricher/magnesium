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
    "HOME", "USERPROFILE", "CODEX_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
    "XDG_CACHE_HOME", "APPDATA", "LOCALAPPDATA", "TMPDIR", "TEMP", "TMP", "PATH",
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
                "CODEX_THREAD_ID": "host-thread", "CLAUDE_CODE_SESSION_ID": "claude-thread",
                "OPENCODE_SESSION_ID": "opencode-thread", "OPENAI_API_KEY": "synthetic-secret",
                "ANTHROPIC_API_KEY": "synthetic-secret", "NODE_OPTIONS": "--import=evil.js",
                "PYTHONPATH": "/original-profile/python", "MAGNESIUM_WORKSPACE_ROOT": "/original-work",
                "MAGNESIUM_DOCS_WORKER": "/original-profile/worker", "MAGNESIUM_DOCS_WORKER_OPT_IN": "1",
                "MAGNESIUM_HARNESS_EXECUTION_GRANT": "synthetic-grant", "MAGNESIUM_DATA_DIR": "/ignored",
                "CODEX_TEST_BINARY": "/original-profile/codex",
                "OPENCODE_TEST_BINARY": "/original-profile/opencode",
                "hOmE": "/original-profile/home", "Path": "/original-profile/bin",
            }
            environment = isolated_environment(directory, hostile)
            for key in hostile:
                if key in {"CODEX_TEST_BINARY", "OPENCODE_TEST_BINARY"}:
                    self.assertFalse(Path(environment[key]).exists())
                    self.assertTrue(Path(environment[key]).is_relative_to(Path(directory).resolve()))
                else:
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
            for key in (*PRIVATE_DIRECTORY_KEYS, "CODEX_TEST_BINARY", "OPENCODE_TEST_BINARY"):
                path = Path(environment[key])
                self.assertEqual(path, path.resolve(), key)
                self.assertTrue(path.is_relative_to(canonical_root), key)
                self.assertTrue(path.resolve().is_relative_to(root.resolve()), key)
                if key in PRIVATE_DIRECTORY_KEYS:
                    self.assertTrue(path.is_dir(), key)
                else:
                    self.assertFalse(path.exists(), key)

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
            host = original / "codex"
            host.write_text("synthetic host, must never execute", encoding="utf-8")
            host.chmod(0o755)
            before = {p.name: p.read_bytes() for p in original.iterdir()}
            source = {key: str(original) for key in PRIVATE_DIRECTORY_KEYS}
            source.update(CODEX_TEST_BINARY=str(host), OPENCODE_TEST_BINARY=str(host))
            for key in ("SystemRoot", "WINDIR"):
                if key in os.environ:
                    source[key] = os.environ[key]
            environment = isolated_environment(base / "fixture", source)
            fake_magnesium = base / "fake-magnesium.py"
            fake_magnesium.write_text(
                "import json, os, pathlib, shutil, subprocess\n"
                "assert shutil.which('codex') is None\n"
                "assert shutil.which('opencode') is None\n"
                "for key in ('CODEX_TEST_BINARY', 'OPENCODE_TEST_BINARY'):\n"
                "    try: subprocess.run([os.environ[key], '--version'], check=True)\n"
                "    except FileNotFoundError: pass\n"
                "    else: raise AssertionError('real host executable was available')\n"
                "for key in ('HOME','USERPROFILE','CODEX_HOME','XDG_CONFIG_HOME','XDG_DATA_HOME',"
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
