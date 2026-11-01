"""MkDocs hooks that keep docs/ the single source of the documentation site.

The pages are written to read well on GitHub, so their links point at real repository paths
(for example ../policy/REVIEW.md or ../poll/assets/icon-128.png). The site only contains docs/,
so this hook publishes the few outside files the pages need and rewrites every relative link
that leaves docs/: to the published copy when there is one, otherwise to the file on GitHub.
"""

from __future__ import annotations

import posixpath
import re
from pathlib import Path

from mkdocs.config.defaults import MkDocsConfig
from mkdocs.structure.files import File, Files
from mkdocs.structure.pages import Page

REPO_ROOT = Path(__file__).resolve().parent.parent
GITHUB = "https://github.com/Mumcord/plugins"

# Repository Markdown files published as site pages: repository path -> page path in the site.
PAGES = {
    "docs/README.md": "developers.md",
    "policy/REVIEW.md": "review-policy.md",
}
# Images published under _repo/<repository path>.
ASSET_GLOBS = ("assets/brand/*.svg", "assets/brand/*.png", "*/assets/icon-128.png")
ASSET_PREFIX = "_repo/"

# Markdown links and images, and src/href attributes in inline HTML.
LINK = re.compile(r"(\]\()([^)\s]+)(\))|((?:src|href)=\")([^\"]+)(\")")


def _assets() -> dict[str, str]:
    found = {}
    for pattern in ASSET_GLOBS:
        for path in REPO_ROOT.glob(pattern):
            rel = path.relative_to(REPO_ROOT).as_posix()
            found[rel] = ASSET_PREFIX + rel
    return found


def on_files(files: Files, config: MkDocsConfig) -> Files:
    for repo_path, uri in PAGES.items():
        files.append(File.generated(config, uri, abs_src_path=str(REPO_ROOT / repo_path)))
    for repo_path, uri in _assets().items():
        files.append(File.generated(config, uri, abs_src_path=str(REPO_ROOT / repo_path)))
    return files


def _rewrite(target: str, repo_dir: str, page_dir: str, published: dict[str, str]) -> str:
    if re.match(r"^[a-z][a-z0-9+.-]*:|^#|^/", target):
        return target
    path, sep, anchor = target.partition("#")
    resolved = posixpath.normpath(posixpath.join(repo_dir, path))
    if resolved.startswith("docs/"):
        uri = resolved[len("docs/") :]
    elif resolved in published:
        uri = published[resolved]
    else:
        # Not part of the site (sources, template, schemas): link to it on GitHub.
        kind = "tree" if path.endswith("/") or not (REPO_ROOT / resolved).is_file() else "blob"
        return f"{GITHUB}/{kind}/main/{resolved}{sep}{anchor}"
    return posixpath.relpath(uri, page_dir or ".") + sep + anchor


def on_page_markdown(markdown: str, page: Page, config: MkDocsConfig, files: Files) -> str:
    source = Path(page.file.abs_src_path).resolve().relative_to(REPO_ROOT).as_posix()
    repo_dir = posixpath.dirname(source)
    published = {**PAGES, **_assets()}
    # MkDocs turns Markdown links into page URLs itself, so they stay relative to the source file.
    # Raw HTML is left alone, so its links must already be relative to the built page.
    markdown_dir = posixpath.dirname(page.file.src_uri)
    html_dir = page.url.rstrip("/") if page.url.endswith("/") else posixpath.dirname(page.url)

    def replace(match: re.Match[str]) -> str:
        if match.group(1):
            return match.group(1) + _rewrite(match.group(2), repo_dir, markdown_dir, published) + match.group(3)
        return match.group(4) + _rewrite(match.group(5), repo_dir, html_dir, published) + match.group(6)

    return LINK.sub(replace, markdown)
