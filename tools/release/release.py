"""Validate exact release tags and prepare Windows assets. Python 3.11+."""
import argparse
import hashlib
import json
import os
import sys
import re
import shutil
import subprocess
import tomllib
from pathlib import Path


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True, stderr=subprocess.PIPE).strip()


def validate_version(root, tag):
    if not re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", tag):
        raise ValueError("Expected a version tag such as v0.1.0")
    version = tag[1:]
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    package = json.loads((root / "apps/desktop/package.json").read_text())
    lock = json.loads((root / "apps/desktop/package-lock.json").read_text())
    tauri = json.loads((root / "apps/desktop/src-tauri/tauri.conf.json").read_text())
    versions = [cargo["workspace"]["package"]["version"], package["version"],
                lock["version"], lock["packages"][""]["version"], tauri["version"]]
    cargo_lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    members = {tomllib.loads(path.read_text(encoding="utf-8"))["package"]["name"]
               for pattern in cargo["workspace"]["members"]
               for path in root.glob(pattern + "/Cargo.toml")}
    versions.extend(p["version"] for p in cargo_lock["package"] if p["name"] in members)
    if any(v != version for v in versions):
        raise ValueError(f"Tag {tag} does not match manifest/lockfile versions: {sorted(set(versions))}")
    return version


def calendar_clients(environ=None):
    """Only inspect presence. Desktop secrets are never logged or interpolated."""
    env = os.environ if environ is None else environ
    def present(name):
        return bool(env.get(name, "").strip())
    return {"google": present("KW_GOOGLE_CALENDAR_CLIENT_ID") and present("KW_GOOGLE_CALENDAR_CLIENT_SECRET"),
            "microsoft": present("KW_MICROSOFT_CALENDAR_CLIENT_ID")}


def calendar_report(clients):
    return "calendar: " + " ".join(f"{provider}={'included' if included else 'missing'}" for provider, included in clients.items())


def preflight(root, tag, main_ref, require_calendar_clients=False, environ=None):
    clients = calendar_clients(environ)
    print(calendar_report(clients), file=sys.stderr)
    if require_calendar_clients and not all(clients.values()):
        raise ValueError("Both Google and Microsoft calendar clients are required")
    validate_version(root, tag)
    sha = git(root, "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}")
    if git(root, "rev-parse", "HEAD") != sha:
        raise ValueError("The checkout must be the exact tagged commit")
    subprocess.run(["git", "-C", str(root), "merge-base", "--is-ancestor", sha, main_ref], check=True)
    return sha


def calendar_notes(clients):
    return "\n".join(f"- One-click {label} calendars: {'included' if clients[provider] else 'not included (custom client required under Advanced)'}."
                     for provider, label in [("google", "Google"), ("microsoft", "Outlook")])


def prepare(root, tag, bundle_root, out):
    validate_version(root, tag)
    installers = list(bundle_root.glob("nsis/*.exe"))
    if len(installers) != 1:
        raise ValueError(f"Expected exactly one NSIS installer; found {len(installers)}")
    if not installers[0].stat().st_size:
        raise ValueError("Installer is empty")
    if out.exists() and any(out.iterdir()):
        raise ValueError("Asset output must be empty; refusing stale release assets")
    out.mkdir(parents=True, exist_ok=True)
    name = f"KnegisWhisper_{tag[1:]}_windows_x64_setup.exe"
    target = out / name
    shutil.copyfile(installers[0], target)
    with target.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    (out / "SHA256SUMS.txt").write_text(f"{digest}  {name}\n", encoding="utf-8", newline="\n")
    template = (root / "tools/release/notes.md").read_text(encoding="utf-8")
    sha = git(root, "rev-parse", "HEAD")
    (out / "RELEASE_NOTES.md").write_text(template.replace("{{TAG}}", tag).replace("{{SHA}}", sha).replace("{{CALENDAR_CLIENTS}}", calendar_notes(calendar_clients())), encoding="utf-8")
    return target


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["preflight", "prepare"])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--main-ref", default="origin/main")
    parser.add_argument("--bundle-root", type=Path, default=Path("target/x86_64-pc-windows-msvc/release/bundle"))
    parser.add_argument("--out", type=Path, default=Path("target/release-assets"))
    parser.add_argument("--require-calendar-clients", action="store_true")
    args = parser.parse_args()
    if args.command == "preflight":
        print(preflight(args.root, args.tag, args.main_ref, args.require_calendar_clients))
    else:
        print(prepare(args.root, args.tag, args.bundle_root, args.out))


if __name__ == "__main__":
    main()
