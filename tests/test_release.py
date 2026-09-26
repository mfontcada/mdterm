import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_package_version(self):
        with patch.object(release, "package_version", return_value="1.2.3"):
            self.assertEqual(release.validate_tag("v1.2.3"), "v1.2.3")
            for tag in ("1.2.3", "v1.2.4", "v01.2.3", "v1.2.3-beta.1", "v1.2.3\n"):
                with self.subTest(tag=tag), self.assertRaises(ValueError):
                    release.validate_tag(tag)

    def test_mismatched_push_is_rejected_before_creating_release(self):
        env = dict(os.environ, GITHUB_EVENT_NAME="push", GITHUB_REF="refs/tags/v9.9.9")
        result = subprocess.run(["python3", str(ROOT / "scripts/release.py"), "metadata"],
                                env=env, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match", result.stderr)

    def test_package_has_only_public_files_and_no_local_owner_metadata(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            binary = directory / "binary"
            binary.write_bytes(b"example executable bytes")
            version = release.package_version()
            with patch.object(release.subprocess, "check_output", return_value=f"mdterm {version}\n"):
                release.package(binary, "aarch64-apple-darwin", directory)
            archive = directory / release.archive_name("v" + version, "aarch64-apple-darwin")
            release.verify_archive(archive)
            with tarfile.open(archive) as bundle:
                self.assertEqual(bundle.extractfile("mdterm").read(), binary.read_bytes())
                self.assertEqual(bundle.extractfile("LICENSE").read(), (ROOT / "LICENSE").read_bytes())

    def test_extra_file_cannot_be_published(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "bad.tar.gz"
            with tarfile.open(path, "w:gz") as archive:
                for name in ("mdterm", "README.md", "LICENSE", "personal.txt"):
                    item = tarfile.TarInfo(name)
                    item.size = 1
                    archive.addfile(item, io.BytesIO(b"x"))
            with self.assertRaises(ValueError):
                release.verify_archive(path)

    def test_published_release_cannot_be_overwritten(self):
        response = subprocess.CompletedProcess([], 0, json.dumps({"draft": False}), "")
        with patch.object(release, "gh", return_value=response), self.assertRaises(ValueError):
            release.release("v0.1.0")

    def test_incomplete_draft_cannot_be_published(self):
        with patch.object(release, "release", return_value={"assets": []}), patch.object(release, "gh") as cli:
            with self.assertRaises(ValueError):
                release.publish("v" + release.package_version())
            cli.assert_not_called()


if __name__ == "__main__":
    unittest.main()
