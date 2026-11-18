#!/usr/bin/env python3
"""Select official Ubuntu HTTPS archives on the hosted Deb822 runner image.

Only the URIs field of recognized, active Ubuntu stanzas changes. The runner's
cloud-specific mirror list is deliberately neither read nor rewritten.

Runner source: https://github.com/actions/runner-images/blob/
d9c48363ed37529f6ef929845cda83bbdae0ef74/images/ubuntu/scripts/build/configure-apt-sources.sh
APT format: https://manpages.ubuntu.com/manpages/resolute/man5/sources.list.5.html
"""

import argparse
from pathlib import Path
import re
import stat


ARCHIVE = "https://archive.ubuntu.com/ubuntu/"
SECURITY = "https://security.ubuntu.com/ubuntu/"
MIRROR = "mirror+file:/etc/apt/apt-mirrors.txt"
KNOWN_URIS = {MIRROR} | {
    f"{scheme}://{host}/ubuntu{slash}"
    for scheme in ("http", "https")
    for host in (
        "azure.archive.ubuntu.com",
        "us-east-2.ec2.archive.ubuntu.com",
        "archive.ubuntu.com",
        "security.ubuntu.com",
    )
    for slash in ("", "/")
}


def transform_stanza(stanza):
    """Return the transformed stanza and number of active Ubuntu entries."""
    lines = stanza.splitlines(keepends=True)
    fields = {}
    current = None
    for index, line in enumerate(lines):
        value = line.rstrip("\r\n")
        if not value.strip() or value.lstrip().startswith("#"):
            continue
        if value[:1].isspace():
            if current is None:
                raise ValueError("Deb822 continuation without a field")
            fields[current][0].append(value.strip())
            fields[current][1].append(index)
            continue
        match = re.fullmatch(r"([A-Za-z][A-Za-z0-9-]*):[ \t]*(.*)", value)
        if match is None:
            raise ValueError("Malformed Deb822 field")
        current = match[1].lower()
        if current in fields:
            raise ValueError(f"Duplicate Deb822 field: {current}")
        fields[current] = ([match[2]], [index])

    def values(name):
        return " ".join(fields.get(name, ([], []))[0]).split()

    # APT's StringToBool accepts these words as well as numeric 0/1.
    enabled = " ".join(values("enabled")).lower()
    if enabled in {"no", "false", "without", "off", "disable", "0"}:
        return stanza, 0
    uris = values("uris")
    if not any(uri in KNOWN_URIS for uri in uris):
        return stanza, 0
    if not uris or any(uri not in KNOWN_URIS for uri in uris):
        raise ValueError("Mixed recognized Ubuntu and unrelated URIs")
    if enabled not in {"", "yes", "true", "with", "on", "enable", "1"}:
        raise ValueError("Unsupported Ubuntu Enabled value")
    if not values("types") or any(kind not in ("deb", "deb-src") for kind in values("types")):
        raise ValueError("Unsupported Ubuntu source types")
    suites = values("suites")
    if not suites or any(re.fullmatch(r"[a-z][a-z0-9-]*", suite) is None for suite in suites):
        raise ValueError("Expected named Ubuntu suites")
    if not values("components"):
        raise ValueError("Expected Ubuntu components")
    security = [suite.endswith("-security") for suite in suites]
    if any(security) and not all(security):
        raise ValueError("Mixed security and nonsecurity suites require separate stanzas")
    target = SECURITY if all(security) else ARCHIVE
    if uris == [target]:
        return stanza, 1
    indexes = fields["uris"][1]
    first = indexes[0]
    prefix = re.match(r"[^:]+:[ \t]*", lines[first])[0]
    ending = "\r\n" if lines[first].endswith("\r\n") else "\n" if lines[first].endswith("\n") else ""
    lines[first] = prefix + target + ending
    for index in indexes[1:]:
        lines[index] = ""
    return "".join(lines), 1


def transform(contents):
    """Validate every stanza before returning any proposed replacement."""
    blocks = re.split(r"((?:\r?\n)[ \t]*(?:\r?\n))", contents)
    recognized = 0
    for index in range(0, len(blocks), 2):
        blocks[index], count = transform_stanza(blocks[index])
        recognized += count
    if not recognized:
        raise ValueError("No active recognized Ubuntu Deb822 sources")
    return "".join(blocks)


def normalize(path):
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError("Expected a regular Ubuntu Deb822 source file")
    before = path.read_bytes()
    after = transform(before.decode("utf-8")).encode("utf-8")
    if after != before:
        path.write_bytes(after)
        return True
    return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sources", type=Path, default=Path("/etc/apt/sources.list.d/ubuntu.sources"))
    args = parser.parse_args()
    changed = normalize(args.sources)
    print("Ubuntu official HTTPS archive/security sources "
          + ("selected" if changed else "already selected")
          + "; suites, components and signing fields preserved")


if __name__ == "__main__":
    main()
