#!/usr/bin/env python3
"""Package and publish releases without storing GitHub Actions artifacts."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
)
STABLE_TAG = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


def package_version():
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        return tomllib.load(manifest)["package"]["version"]


def validate_tag(tag):
    if not STABLE_TAG.fullmatch(tag) or tag != "v" + package_version():
        raise ValueError("Release tag must be vMAJOR.MINOR.PATCH and match Cargo.toml")
    return tag


def archive_name(tag, target):
    return f"mdterm-{tag}-{target}.tar.gz"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_archive(path):
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        if sorted(member.name for member in members) != ["LICENSE", "README.md", "mdterm"]:
            raise ValueError(f"Unexpected files in {path.name}")
        for member in members:
            mode = 0o755 if member.name == "mdterm" else 0o644
            if not member.isfile() or member.mode != mode:
                raise ValueError(f"Invalid file type or permissions: {member.name}")
            if member.uid or member.gid or member.uname or member.gname:
                raise ValueError(f"Archive contains local ownership metadata: {member.name}")


def package(binary, target, directory):
    tag = validate_tag("v" + package_version())
    binary = binary.resolve(strict=True)
    actual = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if actual != f"mdterm {tag[1:]}":
        raise ValueError("Executable version does not match Cargo.toml")
    if target.endswith("linux-musl"):
        headers = subprocess.check_output(["readelf", "-l", str(binary)], text=True)
        if "INTERP" in headers:
            raise ValueError("Linux release executables must be statically linked")
    directory.mkdir(parents=True, exist_ok=True)
    destination = directory / archive_name(tag, target)
    # Fixed timestamps and empty ownership fields keep workstation data out.
    with destination.open("wb") as output:
        with gzip.GzipFile(fileobj=output, mode="wb", filename="", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for name, source in (("mdterm", binary), ("README.md", ROOT / "README.md"), ("LICENSE", ROOT / "LICENSE")):
                    data = source.read_bytes()
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    member.mode = 0o755 if name == "mdterm" else 0o644
                    archive.addfile(member, io.BytesIO(data))
    verify_archive(destination)
    print(destination)


def repository():
    return os.environ.get("GH_REPO", "mfontcada/mdterm")


def gh(*args, check=True):
    result = subprocess.run(["gh", *args], text=True, capture_output=True)
    if check and result.returncode:
        raise RuntimeError(result.stderr.strip() or "GitHub CLI failed")
    return result


def release(tag, missing_ok=False):
    result = gh("api", "--method", "GET", f"repos/{repository()}/releases/tags/{tag}", check=False)
    if result.returncode:
        if missing_ok and "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip())
    data = json.loads(result.stdout)
    if not data["draft"]:
        raise ValueError(f"{tag} is already published; published releases are never overwritten")
    return data


def prepare(tag):
    validate_tag(tag)
    info = json.loads(gh("api", "--method", "GET", f"repos/{repository()}").stdout)
    if info["private"]:
        raise ValueError("Release automation is restricted to public repositories")
    if release(tag, missing_ok=True) is None:
        gh("release", "create", tag, "--repo", repository(), "--draft", "--verify-tag",
           "--generate-notes", "--title", f"mdterm {tag}")
    print(f"Draft release ready: {tag}")


def upload(tag, target, directory):
    validate_tag(tag)
    release(tag)
    path = directory / archive_name(tag, target)
    verify_archive(path)
    gh("release", "upload", tag, str(path), "--repo", repository(), "--clobber")
    print(f"Uploaded {path.name} to the draft release")


def publish(tag):
    validate_tag(tag)
    data = release(tag)
    expected = {archive_name(tag, target) for target in TARGETS}
    allowed = expected | {"SHA256SUMS", "install.sh"}
    names = {asset["name"] for asset in data["assets"]}
    if not expected <= names or not names <= allowed:
        raise ValueError("Draft release must contain exactly the four expected platform archives")
    with tempfile.TemporaryDirectory(prefix="mdterm-release-") as scratch:
        directory = Path(scratch)
        gh("release", "download", tag, "--repo", repository(), "--dir", scratch,
           "--pattern", f"mdterm-{tag}-*.tar.gz")
        lines = []
        for name in sorted(expected):
            path = directory / name
            verify_archive(path)
            lines.append(f"{digest(path)}  {name}\n")
        installer = ROOT / "install.sh"
        lines.append(f"{digest(installer)}  install.sh\n")
        checksums = directory / "SHA256SUMS"
        checksums.write_text("".join(lines), encoding="ascii")
        gh("release", "upload", tag, str(checksums), str(installer),
           "--repo", repository(), "--clobber")
    names = {asset["name"] for asset in release(tag)["assets"]}
    if names != allowed:
        raise ValueError("Release asset inventory is incomplete")
    gh("release", "edit", tag, "--repo", repository(), "--draft=false", "--latest")
    print(f"Published https://github.com/{repository()}/releases/tag/{tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("metadata")
    for name in ("prepare", "publish", "upload"):
        command = commands.add_parser(name)
        command.add_argument("--tag", required=True)
        if name == "upload":
            command.add_argument("--target", required=True, choices=TARGETS)
            command.add_argument("--directory", required=True, type=Path)
    pack = commands.add_parser("package")
    pack.add_argument("--binary", required=True, type=Path)
    pack.add_argument("--target", required=True, choices=TARGETS)
    pack.add_argument("--directory", required=True, type=Path)
    args = parser.parse_args()
    if args.command == "metadata":
        tag = validate_tag("v" + package_version())
        if os.environ.get("GITHUB_EVENT_NAME") == "push" and os.environ.get("GITHUB_REF") != f"refs/tags/{tag}":
            raise ValueError("Pushed tag does not match Cargo.toml")
        print(f"tag={tag}")
    elif args.command == "package":
        package(args.binary, args.target, args.directory)
    elif args.command == "prepare":
        prepare(args.tag)
    elif args.command == "upload":
        upload(args.tag, args.target, args.directory)
    else:
        publish(args.tag)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"release: {error}") from error
