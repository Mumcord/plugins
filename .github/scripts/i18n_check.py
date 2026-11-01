#!/usr/bin/env python3
"""Translation completeness check for official plugins.

Usage: i18n_check.py <plugin-folder>...

Every official plugin must ship i18n/<tag>.json for all 24 Mumcord languages. Each file has
exactly the keys of en.json. Plural keys end in _zero/_one/_two/_few/_many/_other; the variants
a language uses may differ, but _other must exist for every plural key.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

LANGUAGES = (
    "bg cs da de el en es et fi fr ga hr hu it lt lv mt nl pl pt ro sk sl sv".split()
)
PLURAL_SUFFIX = re.compile(r"^(.+)_(zero|one|two|few|many|other)$")


def load(path: Path, problems: list[str]) -> dict[str, object] | None:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        problems.append(f"{path.name}: cannot be read as JSON ({error})")
        return None
    if not isinstance(data, dict):
        problems.append(f"{path.name}: must be a JSON object")
        return None
    return data


def split(keys: set[str]) -> tuple[set[str], set[str]]:
    """Returns (plain keys, plural base names)."""
    plain, plural = set(), set()
    for key in keys:
        match = PLURAL_SUFFIX.match(key)
        if match:
            plural.add(match.group(1))
        else:
            plain.add(key)
    return plain, plural


def check_folder(folder: Path) -> list[str]:
    """Returns a list of problems for one plugin folder; empty when complete."""
    i18n = folder / "i18n"
    problems: list[str] = []
    english = load(i18n / "en.json", problems)
    if english is None:
        return problems
    en_plain, en_plural = split(set(english))
    for base in sorted(en_plural):
        if f"{base}_other" not in english:
            problems.append(f"en.json: plural key {base!r} lacks {base}_other")
    for tag in LANGUAGES:
        if tag == "en":
            continue
        if not (i18n / f"{tag}.json").is_file():
            problems.append(f"{tag}.json: language file is missing")
            continue
        data = load(i18n / f"{tag}.json", problems)
        if data is None:
            continue
        plain, plural = split(set(data))
        missing = sorted(en_plain - plain) + [f"{b}_other" for b in sorted(en_plural)
                                              if f"{b}_other" not in data]
        extra = sorted(plain - en_plain) + sorted(
            f"{b}_*" for b in plural - en_plural)
        if missing:
            problems.append(f"{tag}.json: missing keys: {', '.join(missing)}")
        if extra:
            problems.append(f"{tag}.json: keys not in en.json: {', '.join(extra)}")
    return problems


def main(folders: list[str]) -> int:
    failed = False
    for name in folders:
        problems = check_folder(Path(name))
        for problem in problems:
            print(f"::error::{name}/i18n/{problem}")
        failed = failed or bool(problems)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
