#!/usr/bin/env python3
"""Extract the reviewed release entry, failing closed when it is absent or empty."""
import pathlib
import re
import sys


def release_notes(changelog: str, version: str) -> str:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("expected a stable semantic version")
    heading = re.compile(r"^## \[" + re.escape(version) + r"\] — .+$", re.MULTILINE)
    matches = list(heading.finditer(changelog))
    if len(matches) != 1:
        raise ValueError("expected exactly one changelog entry for " + version)
    start = matches[0].end()
    end = re.search(r"^## ", changelog[start:], re.MULTILINE)
    body = changelog[start:start + end.start()] if end else changelog[start:]
    # Link definitions at the bottom of the document are not release notes.
    body = re.split(r"^\[[^\]]+\]:", body, maxsplit=1, flags=re.MULTILINE)[0].strip()
    if not body:
        raise ValueError("empty changelog entry")
    return f"{body}\n\n[Full changelog](https://github.com/leostera/git-twig/blob/v{version}/CHANGELOG.md)\n"


if __name__ == "__main__":
    print(release_notes(pathlib.Path("CHANGELOG.md").read_text(), sys.argv[1]), end="")
