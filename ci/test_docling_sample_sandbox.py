from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from workers.docling.sample import generate as sample


class DoclingSampleSandboxTests(unittest.TestCase):
    def test_docling_sample_commands_keep_sandbox_and_private_profile(self):
        for function, option in ((sample.print_pdf, "--print-to-pdf="), (sample.screenshot_png, "--screenshot=")):
            with self.subTest(function=function.__name__), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "sample"

                def capture(command, **_kwargs):
                    self.assertEqual(command[:3], ["/fixture/chrome", "--headless=new", "--disable-gpu"])
                    profile = Path(command[3].split("=", 1)[1])
                    page = Path(command[-1])
                    self.assertEqual(command[3], f"--user-data-dir={page.parent / 'profile'}")
                    self.assertTrue(page.is_file())
                    self.assertNotEqual(profile, Path.home())
                    expected = ["--no-pdf-header-footer"] if function is sample.print_pdf else [
                        "--window-size=1000,1250", "--force-device-scale-factor=1",
                    ]
                    self.assertEqual(command[4:-1], [*expected, option + str(output)])
                    output.write_bytes(b"fixture")
                    return subprocess.CompletedProcess(command, 0, "", "")

                with patch.object(sample.subprocess, "run", side_effect=capture) as launch:
                    function("/fixture/chrome", "<p>fixture</p>", str(output))
                self.assertEqual(launch.call_count, 1)

    def test_docling_sample_reports_blocked_browser_without_retrying_unsandboxed(self):
        for function in (sample.print_pdf, sample.screenshot_png):
            with self.subTest(function=function.__name__), tempfile.TemporaryDirectory() as directory:
                with patch.object(sample.subprocess, "run", return_value=
                        subprocess.CompletedProcess([], 1, "", "sandbox unavailable")) as launch:
                    with self.assertRaisesRegex(SystemExit, "sandbox unavailable"):
                        function("/fixture/chrome", "<p>fixture</p>", str(Path(directory) / "sample"))
                self.assertEqual(launch.call_count, 1)
                self.assertNotIn("--no-sandbox", launch.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
