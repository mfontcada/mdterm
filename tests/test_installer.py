"""Exercise the installer without contacting GitHub or changing the user's home."""

import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
    ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
    ("Darwin", "x86_64", "x86_64-apple-darwin"),
    ("Darwin", "arm64", "aarch64-apple-darwin"),
)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mdterm installer ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.destination = self.root / "installed bin"
        self.downloads = self.root / "releases"
        self.log = self.root / "requests.txt"
        self.env = os.environ.copy()
        self.env.pop("MDTERM_VERSION", None)
        self.env.pop("MDTERM_INSTALL_DIR", None)
        self.env.update(
            PATH=f"{self.tools}{os.pathsep}{os.environ['PATH']}",
            HOME=str(self.root / "home"),
            TMPDIR=str(self.root),
            MDTERM_INSTALL_DIR=str(self.destination),
            FAKE_RELEASES=str(self.downloads),
            FAKE_REQUEST_LOG=str(self.log),
            FAKE_LATEST="v0.1.0",
            FAKE_OS="Linux",
            FAKE_ARCH="x86_64",
        )
        self.tool("uname", """
            #!/bin/sh
            case "$1" in
                -s) printf '%s\\n' "$FAKE_OS" ;;
                -m) printf '%s\\n' "$FAKE_ARCH" ;;
                *) exit 1 ;;
            esac
        """)
        self.tool("curl", """
            #!/usr/bin/env python3
            import os, sys
            from pathlib import Path
            import shutil
            args = sys.argv[1:]
            url = args[-1]
            with Path(os.environ['FAKE_REQUEST_LOG']).open('a') as log:
                log.write(url + '\\n')
            if os.environ.get('FAKE_CURL_FAILURE', 'NEVER_MATCH') in url:
                raise SystemExit(22)
            repo = 'https://github.com/mfontcada/mdterm'
            if '--write-out' in args:
                assert url == repo + '/releases/latest', url
                print(repo + '/releases/tag/' + os.environ['FAKE_LATEST'], end='')
            else:
                prefix = repo + '/releases/download/'
                assert url.startswith(prefix), url
                relative = url.removeprefix(prefix)
                source = Path(os.environ['FAKE_RELEASES']) / relative
                destination = args[args.index('--output') + 1]
                if not source.is_file():
                    raise SystemExit(22)
                shutil.copyfile(source, destination)
        """)
        self.make_release("v0.1.0")

    def tool(self, name, source):
        path = self.tools / name
        path.write_text(textwrap.dedent(source).lstrip(), encoding="utf8")
        path.chmod(0o755)

    def make_release(self, tag, binary=None, reported_version=None):
        directory = self.downloads / tag
        directory.mkdir(parents=True, exist_ok=True)
        content = binary or f"#!/bin/sh\nprintf 'mdterm {reported_version or tag[1:]}\\n'\n".encode()
        checksums = []
        for _, _, target in TARGETS:
            name = f"mdterm-{tag}-{target}.tar.gz"
            archive = directory / name
            with tarfile.open(archive, "w:gz") as output:
                member = tarfile.TarInfo("mdterm")
                member.size = len(content)
                member.mode = 0o755
                output.addfile(member, io.BytesIO(content))
            checksums.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {name}\n")
        (directory / "SHA256SUMS").write_text("".join(checksums))

    def install(self, expect_success=True):
        result = subprocess.run(["sh", str(ROOT / "install.sh")], env=self.env,
                                cwd=self.root, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=20)
        if expect_success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(list(self.root.glob("mdterm.*")), [], "temporary downloads were not removed")
        self.assertEqual(list(self.destination.glob(".mdterm.*")), [], "staged executable was not removed")
        return result

    def existing_installation(self):
        self.destination.mkdir(parents=True)
        (self.destination / "mdterm").write_bytes(b"previous installation")

    def assert_preserved(self):
        self.assertEqual((self.destination / "mdterm").read_bytes(), b"previous installation")

    def test_all_four_platforms_install_a_verified_executable(self):
        for operating_system, arch, target in TARGETS:
            with self.subTest(target=target):
                self.env.update(FAKE_OS=operating_system, FAKE_ARCH=arch)
                self.install()
                binary = self.destination / "mdterm"
                self.assertEqual(subprocess.check_output([str(binary), "--version"], text=True).strip(), "mdterm 0.1.0")
                self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
                self.assertIn(f"mdterm-v0.1.0-{target}.tar.gz", self.log.read_text())

    def test_update_and_pinned_version(self):
        self.install()
        self.make_release("v0.2.0")
        self.env["FAKE_LATEST"] = "v0.2.0"
        self.install()
        binary = self.destination / "mdterm"
        self.assertIn("0.2.0", subprocess.check_output([str(binary)], text=True))
        self.env["MDTERM_VERSION"] = "0.1.0"
        self.log.unlink()
        self.install()
        self.assertIn("0.1.0", subprocess.check_output([str(binary)], text=True))
        self.assertNotIn("/releases/latest", self.log.read_text())

    def test_default_destination_and_path_guidance(self):
        del self.env["MDTERM_INSTALL_DIR"]
        result = self.install()
        self.assertTrue((Path(self.env["HOME"]) / ".local/bin/mdterm").is_file())
        self.assertIn('export PATH="$HOME/.local/bin:$PATH"', result.stdout)

    def test_checksum_mismatch_preserves_existing_installation(self):
        self.existing_installation()
        archive = next((self.downloads / "v0.1.0").glob("*x86_64-unknown-linux-musl.tar.gz"))
        archive.write_bytes(b"corrupt download")
        result = self.install(expect_success=False)
        self.assertIn("Checksum mismatch", result.stderr)
        self.assert_preserved()

    def test_download_failure_preserves_existing_installation(self):
        self.existing_installation()
        self.env["FAKE_CURL_FAILURE"] = "/SHA256SUMS"
        self.install(expect_success=False)
        self.assert_preserved()

    def test_wrong_executable_version_preserves_existing_installation(self):
        self.existing_installation()
        self.make_release("v0.1.0", reported_version="9.9.9")
        result = self.install(expect_success=False)
        self.assertIn("version does not match", result.stderr)
        self.assert_preserved()

    def test_unsupported_platform_fails_before_downloading(self):
        for system, arch in (("FreeBSD", "x86_64"), ("Linux", "i686")):
            with self.subTest(system=system, arch=arch):
                self.env.update(FAKE_OS=system, FAKE_ARCH=arch)
                self.install(expect_success=False)
                self.assertFalse(self.log.exists())
                self.assertFalse(self.destination.exists())

    def test_invalid_version_cannot_change_the_download_path(self):
        self.env["MDTERM_VERSION"] = "../../elsewhere"
        self.install(expect_success=False)
        self.assertFalse(self.log.exists())

    def test_symlink_archive_is_rejected(self):
        self.existing_installation()
        directory = self.downloads / "v0.1.0"
        name = "mdterm-v0.1.0-x86_64-unknown-linux-musl.tar.gz"
        archive = directory / name
        with tarfile.open(archive, "w:gz") as output:
            member = tarfile.TarInfo("mdterm")
            member.type = tarfile.SYMTYPE
            member.linkname = "/bin/sh"
            output.addfile(member)
        (directory / "SHA256SUMS").write_text(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {name}\n")
        self.install(expect_success=False)
        self.assert_preserved()

    @unittest.skipUnless(os.environ.get("MDTERM_BINARY"), "set MDTERM_BINARY to verify a real release executable")
    def test_real_binary_installs_and_reports_version_without_a_terminal(self):
        binary = Path(os.environ["MDTERM_BINARY"]).resolve()
        result = subprocess.run([str(binary), "--version"], stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, check=True)
        version = result.stdout.strip().removeprefix("mdterm ")
        self.make_release("v" + version, binary=binary.read_bytes())
        self.env["MDTERM_VERSION"] = "v" + version
        self.install()
        installed = subprocess.check_output([str(self.destination / "mdterm"), "--version"], text=True)
        self.assertEqual(installed, result.stdout)


if __name__ == "__main__":
    unittest.main()
