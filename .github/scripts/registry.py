#!/usr/bin/env python3
"""Registry checks and publish steps for Mumcord/plugins.

Used by the `check`, `publish` and `pr-comment` workflows. Format reference:
docs/plugin-registry-format.md in Mumcord/mumcord (section "Registry source").

Subcommands:
  changed DIFF_BASE              list "<id> <version>" for version files added or changed since DIFF_BASE
  verify ID VERSION              rebuild one version from its source and run every package check
  i18n                           require all 24 languages in every official plugin
  permission-diff DIFF_BASE      write the Markdown permission diff for a pull request to stdout
  build-missing OUT              rebuild and check every version without a GitHub release into OUT
  release-built OUT              create the GitHub releases for the packages in OUT
  sequence INDEX_URL             print the next index sequence (above the published one)
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
PLUGINS = REPO / "plugins"
CLI = os.environ.get("MUMCORD_PLUGIN", "mumcord-plugin")
DENY_CONFIG = REPO / ".github" / "deny.toml"
WASM_TARGET = "wasm32-unknown-unknown"
COMMENT_MARKER = "<!-- mumcord-permission-diff -->"

# OSI-approved licences accepted for plugins (FR-6.56). Extend after maintainer review.
OSI_LICENCES = {
    "0BSD", "AGPL-3.0-only", "AGPL-3.0-or-later", "Apache-2.0", "BSD-2-Clause",
    "BSD-3-Clause", "BSL-1.0", "EUPL-1.2", "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0-only",
    "GPL-3.0-or-later", "ISC", "LGPL-2.1-only", "LGPL-2.1-or-later", "LGPL-3.0-only",
    "LGPL-3.0-or-later", "MIT", "MPL-2.0", "Unlicense", "Zlib",
}
VERSION_FILE = re.compile(r"^plugins/([a-z][a-z0-9-]{2,31})/versions/([^/]+)\.toml$")
PLUGIN_FILE = re.compile(r"^plugins/([a-z][a-z0-9-]{2,31})/plugin\.toml$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")


class CheckError(Exception):
    pass


def run(*args: str | Path, cwd: Path | None = None, capture: bool = False) -> str:
    result = subprocess.run(
        [str(a) for a in args], cwd=cwd, check=False, text=True,
        stdout=subprocess.PIPE if capture else None,
    )
    if result.returncode != 0:
        raise CheckError(f"command failed ({result.returncode}): {' '.join(map(str, args))}")
    return result.stdout or ""


def git(*args: str, cwd: Path = REPO) -> str:
    return run("git", *args, cwd=cwd, capture=True)


def version_key(version: str) -> tuple:
    """Semver order: release > pre-release; numeric pre-release parts compare as numbers."""
    core, _, pre = version.partition("+")[0].partition("-")
    numbers = tuple(int(p) for p in core.split("."))
    if not pre:
        return numbers + (1,)
    parts = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return numbers + (0, parts)


def read_toml_at(rev: str | None, path: str) -> dict | None:
    """Reads a TOML file from the working tree (rev None) or a git revision; None if missing."""
    if rev is None:
        file = REPO / path
        return tomllib.loads(file.read_text(encoding="utf-8")) if file.is_file() else None
    probe = subprocess.run(
        ["git", "show", f"{rev}:{path}"], cwd=REPO, text=True, capture_output=True, check=False,
    )
    return tomllib.loads(probe.stdout) if probe.returncode == 0 else None


def versions_at(rev: str | None, plugin_id: str) -> list[str]:
    if rev is None:
        folder = PLUGINS / plugin_id / "versions"
        return sorted((p.stem for p in folder.glob("*.toml")), key=version_key)
    listing = subprocess.run(
        ["git", "ls-tree", "--name-only", f"{rev}:plugins/{plugin_id}/versions"],
        cwd=REPO, text=True, capture_output=True, check=False,
    )
    if listing.returncode != 0:
        return []
    names = [n[: -len(".toml")] for n in listing.stdout.split() if n.endswith(".toml")]
    return sorted(names, key=version_key)


def changed_files(base: str) -> list[str]:
    return git("diff", "--name-only", "--diff-filter=AM", f"{base}...HEAD", "--").split()


def changed_versions(base: str) -> list[tuple[str, str]]:
    found = []
    for name in changed_files(base):
        if match := VERSION_FILE.match(name):
            found.append((match[1], match[2]))
    return found


# --- verify -----------------------------------------------------------------------------------


def fetch_source(source: dict, into: Path) -> Path:
    repository, commit = source.get("repository", ""), source.get("commit", "")
    # file:// sources exist only for local dry runs of this script.
    schemes = ("https://", "file://") if os.environ.get("REGISTRY_ALLOW_FILE_SOURCES") else ("https://",)
    if not repository.startswith(schemes):
        raise CheckError(f"source.repository must be an https URL, got {repository!r}")
    if not COMMIT.match(commit):
        raise CheckError("source.commit must be a full 40-digit commit hash")
    path = source.get("path", "")
    if path.startswith("/") or ".." in Path(path).parts:
        raise CheckError(f"source.path {path!r} must stay inside the repository")
    git("init", "-q", str(into))
    git("fetch", "-q", "--depth", "1", repository, commit, cwd=into)
    git("checkout", "-q", "FETCH_HEAD", cwd=into)
    folder = into / path
    if not (folder / "manifest.json").is_file():
        raise CheckError(f"no manifest.json in source folder {path or '.'}")
    return folder


def prepare_toolchain(folder: Path) -> None:
    # A rust-toolchain.toml in the source pins the compiler; rustup picks it up from `folder`.
    run("rustup", "show", "active-toolchain", cwd=folder)
    run("rustup", "target", "add", WASM_TARGET, cwd=folder)


def build(folder: Path, out: Path) -> tuple[Path, str]:
    output = run(CLI, "build", folder, "--reproducible", "--out", out, capture=True)
    lines = output.splitlines()
    package = Path(lines[0])
    digest = next(line.split()[1] for line in lines if line.startswith("sha256 "))
    return package, digest


def check_licence(folder: Path) -> None:
    manifest = json.loads((folder / "manifest.json").read_text(encoding="utf-8"))
    licence = manifest.get("license", "")
    terms = [t for t in re.split(r"\s+(?:OR|AND|WITH)\s+|[()]", licence) if t.strip()]
    unknown = [t for t in terms if t.strip() not in OSI_LICENCES]
    if not terms or unknown:
        raise CheckError(f"licence {licence!r} is not on the OSI allow-list ({', '.join(unknown)})")
    if not (folder / "LICENSE").is_file():
        raise CheckError("the source folder has no LICENSE file")
    # cargo-deny 0.20 takes --config as a global option, before the subcommand.
    run("cargo", "deny", "--manifest-path", folder / "Cargo.toml", "--config", DENY_CONFIG,
        "check", "licenses", "--hide-inclusion-graph")


def check_signature(package: Path, version: dict, author_keys: list[str]) -> None:
    signature = package.with_name(package.name + ".minisig")
    signature.write_text(version["signature"], encoding="utf-8")
    errors = []
    for key in author_keys:
        probe = subprocess.run([CLI, "check", str(package), "--official", "--key", key],
                               check=False, capture_output=True, text=True)
        if probe.returncode == 0:
            return
        errors.append(probe.stderr.strip())
    raise CheckError("the package fails `mumcord-plugin check --official` with every author key: "
                     + " | ".join(errors or ["plugin.toml lists no author_keys"]))


def load_test(package: Path, work: Path) -> None:
    scenario = work / "load.yaml"
    scenario.write_text("name: Loads in the mock host\nsteps: []\n", encoding="utf-8")
    run(CLI, "test", package, scenario)


def verify(plugin_id: str, version_name: str, out: Path) -> Path:
    """Rebuilds one version and runs every check; returns the package (signature next to it)."""
    plugin = read_toml_at(None, f"plugins/{plugin_id}/plugin.toml")
    version = read_toml_at(None, f"plugins/{plugin_id}/versions/{version_name}.toml")
    if plugin is None or version is None:
        raise CheckError(f"{plugin_id} {version_name}: plugin.toml or version file missing")
    if version.get("version") != version_name:
        raise CheckError(f"{plugin_id}: the file name {version_name}.toml does not match "
                         f"version {version.get('version')!r}")
    work = Path(tempfile.mkdtemp(prefix=f"{plugin_id}-{version_name}-"))
    try:
        step("source", plugin_id, version_name)
        folder = fetch_source(version["source"], work / "src")
        prepare_toolchain(folder)
        step("reproducible rebuild", plugin_id, version_name)
        package, digest = build(folder, out)
        size = package.stat().st_size
        if digest != version["sha256"] or size != version["size"]:
            raise CheckError(f"rebuild gives sha256 {digest} ({size} bytes), the version file "
                             f"says {version['sha256']} ({version['size']} bytes)")
        expected = out / f"{plugin_id}-{version_name}.mcplugin"
        if package != expected:
            package = package.rename(expected)
        step("author signature", plugin_id, version_name)
        check_signature(package, version, plugin["author_keys"])
        step("licence scan", plugin_id, version_name)
        check_licence(folder)
        step("mock-host load test", plugin_id, version_name)
        load_test(package, work)
        return package
    finally:
        shutil.rmtree(work, ignore_errors=True)


def step(name: str, plugin_id: str, version: str) -> None:
    print(f"==> {plugin_id} {version}: {name}", flush=True)


# --- permission diff --------------------------------------------------------------------------


def code(text: str) -> str:
    """Inline code for untrusted PR text: no backticks or line breaks to escape the span."""
    return "`" + re.sub(r"[`\r\n]", "", str(text)) + "`"


def permission_diff(base: str) -> str:
    changed = changed_files(base)
    ids = sorted({m[1] for name in changed for m in [VERSION_FILE.match(name) or
                                                    PLUGIN_FILE.match(name)] if m})
    if not ids:
        return ""
    lines = [COMMENT_MARKER, "## Permission diff", ""]
    review = []
    for plugin_id in ids:
        before = read_toml_at(base, f"plugins/{plugin_id}/plugin.toml")
        after = read_toml_at(None, f"plugins/{plugin_id}/plugin.toml") or {}
        lines.append(f"### `{plugin_id}`")
        if before is None:
            lines.append("- **New plugin**")
            review.append(f"`{plugin_id}` is a new plugin")
        new_keys = sorted(set(after.get("author_keys", [])) - set((before or {}).get("author_keys", [])))
        for key in new_keys:
            lines.append(f"- **New author key** {code(key)}")
            if before is not None:
                review.append(f"`{plugin_id}` has a new author key")
        published = versions_at(base, plugin_id)
        for version in versions_at(None, plugin_id):
            if f"plugins/{plugin_id}/versions/{version}.toml" not in changed:
                continue
            file = read_toml_at(None, f"plugins/{plugin_id}/versions/{version}.toml") or {}
            older = [v for v in published if version_key(v) < version_key(version)]
            previous = read_toml_at(base, f"plugins/{plugin_id}/versions/{older[-1]}.toml") if older else None
            now = set(file.get("permissions", []))
            was = set((previous or {}).get("permissions", []))
            against = f"against {code(older[-1])}" if older else "first version"
            lines.append(f"- Version {code(version)} ({against})")
            added, removed = sorted(now - was), sorted(was - now)
            for permission in added:
                lines.append(f"  - added {code(permission)}")
            for permission in removed:
                lines.append(f"  - removed {code(permission)}")
            if not added and not removed:
                lines.append("  - no permission changes")
            if added and before is not None:
                review.append(f"`{plugin_id}` {version} asks for new permissions")
        lines.append("")
    if review:
        lines.append("**Needs one maintainer approval** (policy/REVIEW.md): " + "; ".join(review) + ".")
    else:
        lines.append("No new plugin, author key or permission.")
    return "\n".join(lines) + "\n"


# --- publish ----------------------------------------------------------------------------------


def release_exists(tag: str) -> bool:
    probe = subprocess.run(["gh", "release", "view", tag], cwd=REPO, check=False,
                           capture_output=True, text=True)
    return probe.returncode == 0


RELEASES_FILE = "releases.txt"


def build_missing(out: Path) -> None:
    """Rebuilds and checks every version without a release; packages and a list land in `out`."""
    out.mkdir(parents=True, exist_ok=True)
    built = []
    for folder in sorted(p for p in PLUGINS.iterdir() if p.is_dir()):
        for version in versions_at(None, folder.name):
            if not release_exists(f"{folder.name}-v{version}"):
                verify(folder.name, version, out)
                built.append(f"{folder.name} {version}\n")
    (out / RELEASES_FILE).write_text("".join(built), encoding="utf-8")


def release_built(out: Path) -> None:
    """Creates the release `<id>-v<ver>` for each package `build-missing` left in `out`."""
    commit = git("rev-parse", "HEAD").strip()
    for line in (out / RELEASES_FILE).read_text(encoding="utf-8").splitlines():
        plugin_id, version = line.split()
        package = out / f"{plugin_id}-{version}.mcplugin"
        signature = package.with_name(package.name + ".minisig")
        tag = f"{plugin_id}-v{version}"
        run("gh", "release", "create", tag, package, signature, "--target", commit,
            "--title", f"{plugin_id} {version}",
            "--notes", f"Package of {plugin_id} {version}, rebuilt from its source by CI.",
            cwd=REPO)
        print(f"released {tag}", flush=True)


def next_sequence(index_url: str) -> int:
    published = 0
    try:
        request = urllib.request.Request(index_url, headers={"User-Agent": "Mumcord"})
        with urllib.request.urlopen(request, timeout=30) as response:
            published = int(json.load(response)["sequence"])
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        print("no published index yet", file=sys.stderr)
    return max(int(time.time()), published + 1)


OFFICIAL_FOLDERS = ("poll", "dice", "echolings", "mumbot-music", "eve-fit")
OFFICIAL_REPOSITORY = "https://github.com/Mumcord/plugins"


def official_plugin_folders(work: Path) -> list[Path]:
    """Source folders of every official plugin: the folders of this repository that already hold a
    plugin, plus the latest version of each registry plugin whose source is this repository."""
    folders = [REPO / name for name in OFFICIAL_FOLDERS if (REPO / name / "manifest.json").is_file()]
    for plugin_dir in sorted(PLUGINS.glob("*/")):
        versions = sorted((plugin_dir / "versions").glob("*.toml"), key=lambda f: version_key(f.stem))
        if not versions:
            continue
        source = tomllib.loads(versions[-1].read_text(encoding="utf-8")).get("source", {})
        repository = source.get("repository", "").removesuffix(".git").rstrip("/")
        if repository.lower() == OFFICIAL_REPOSITORY.lower():
            folders.append(fetch_source(source, work / plugin_dir.name))
    return folders


def check_i18n() -> None:
    from i18n_check import check_folder

    failures = []
    with tempfile.TemporaryDirectory(prefix="i18n-") as work:
        for folder in official_plugin_folders(Path(work)):
            for problem in check_folder(folder):
                failures.append(f"{folder.name}/i18n/{problem}")
    if failures:
        raise CheckError("official plugins must ship all 24 languages:\n" + "\n".join(failures))


def main(argv: list[str]) -> int:
    command, args = (argv[0], argv[1:]) if argv else ("", [])
    try:
        if command == "changed" and len(args) == 1:
            for plugin_id, version in changed_versions(args[0]):
                print(plugin_id, version)
        elif command == "verify" and len(args) == 2:
            out = Path(tempfile.mkdtemp(prefix="packages-"))
            print(f"verified {verify(args[0], args[1], out)}")
        elif command == "permission-diff" and len(args) == 1:
            sys.stdout.write(permission_diff(args[0]))
        elif command == "i18n" and not args:
            check_i18n()
            print("official plugins: all 24 languages complete")
        elif command == "build-missing" and len(args) == 1:
            build_missing(Path(args[0]))
        elif command == "release-built" and len(args) == 1:
            release_built(Path(args[0]))
        elif command == "sequence" and len(args) == 1:
            print(next_sequence(args[0]))
        else:
            print(__doc__, file=sys.stderr)
            return 2
    except KeyError as error:
        print(f"::error::missing field {error}", file=sys.stderr)
        return 1
    except (CheckError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
