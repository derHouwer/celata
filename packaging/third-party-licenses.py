#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 derHouwer
"""Generates THIRD-PARTY-LICENSES.md: every third-party component of Celata with its license and license text.

Run it in the project directory after every change of Cargo.lock:

    python3 packaging/third-party-licenses.py            writes THIRD-PARTY-LICENSES.md
    python3 packaging/third-party-licenses.py --check    only checks that the file is up to date (exit code 1 if not)

Sources:
    cargo tree --target x86_64-unknown-linux-gnu -e no-dev   which crates the Linux build really uses
                                                             (respects the enabled features)
    cargo metadata                                           license and directory of every crate
    the directory of the crate                               license files (LICENSE*, COPYING*, NOTICE* ...)

Identical texts appear only once; the tables refer to them by number. If a crate ships no license text, the
standard text of the license it names is used and marked as such. The output depends only on Cargo.lock and
the crates, not on the machine (no paths, no date). `tests/release.rs` fails if the list no longer matches
Cargo.lock.
"""

import collections
import json
import os
import re
import subprocess
import sys

TARGET = "x86_64-unknown-linux-gnu"
OUTPUT = "THIRD-PARTY-LICENSES.md"
OWN = {"celata", "celata-ui"}
FENCE = "~~~~~~~~"

# Files in the directory of a crate that count as license text.
LICENSE_FILE = re.compile(r"(?i)^(licen[sc]e|copying|unlicense|notice|copyright)")
LICENSE_DIR = re.compile(r"(?i)^licen[sc]es?$")

# Artwork shipped with Celata: (name, version, license, license file in the project, use).
ARTWORK = [
    ("flag-icons", "7.6.0", "MIT", "assets/flags/LICENSE", "country flags in `assets/flags/`"),
    ("Lucide", "1.70.0", "ISC", "ui/icons/LICENSE-lucide", "icons in `ui/icons/`"),
]

MIT_TEXT = """MIT License

Copyright (c) {holders}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""


def run(*command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def normalize(text):
    """Unify line endings, drop trailing whitespace and blank lines at both ends."""
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").replace("\f", "").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def read_text(path):
    with open(path, encoding="utf-8", errors="replace") as file:
        return normalize(file.read())


def license_files(folder):
    """(name relative to the directory, text) of all license files of a crate, sorted by name."""
    found = []
    for name in sorted(os.listdir(folder)):
        path = os.path.join(folder, name)
        if os.path.isfile(path) and LICENSE_FILE.match(name):
            found.append((name, read_text(path)))
        elif os.path.isdir(path) and LICENSE_DIR.match(name):
            for inner in sorted(os.listdir(path)):
                inner_path = os.path.join(path, inner)
                if os.path.isfile(inner_path):
                    found.append((f"{name}/{inner}", read_text(inner_path)))
    return [(name, text) for name, text in found if text]


def spdx_ids(expression):
    return [part for part in re.split(r"\s+(?:OR|AND|WITH)\s+|[()/]|\s+", expression or "") if part]


def linux_build():
    """(name, version) of all third-party crates the Linux build uses (without crates only used by tests)."""
    output = run("cargo", "tree", "--locked", "--target", TARGET, "-e", "no-dev", "--prefix", "none", "--format", "{p}")
    used = set()
    for line in output.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v") and parts[0] not in OWN:
            used.add((parts[0], parts[1][1:]))
    return used


def lock_packages():
    with open("Cargo.lock", encoding="utf-8") as file:
        content = file.read()
    found = re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', content)
    return {(name, version) for name, version in found if name not in OWN}


def build():
    metadata = json.loads(run("cargo", "metadata", "--format-version", "1", "--locked"))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"] if p["name"] not in OWN}
    locked = lock_packages()
    missing = locked - set(packages)
    if missing:
        sys.exit(f"cargo metadata does not know {len(missing)} entries of Cargo.lock, for example {sorted(missing)[:3]}")
    used = linux_build()
    if not used <= locked:
        sys.exit(f"cargo tree names crates that are not in Cargo.lock: {sorted(used - locked)[:3]}")

    # Standard text of the Apache license: the text most crates ship word for word.
    apache = collections.Counter()
    collected = {}
    for key in sorted(used):
        folder = os.path.dirname(packages[key]["manifest_path"])
        collected[key] = license_files(folder)
        for _name, text in collected[key]:
            if text.startswith("Apache License") and "END OF TERMS AND CONDITIONS" in text:
                apache[text] += 1
    apache_text = apache.most_common(1)[0][0]

    texts = []  # (text, [users])
    index = {}

    def number(text, user):
        if text not in index:
            index[text] = len(texts)
            texts.append((text, []))
        texts[index[text]][1].append(user)
        return index[text] + 1

    rows = []
    without_text = []
    for key in sorted(used, key=lambda k: (k[0].lower(), k[1])):
        name, version = key
        package = packages[key]
        expression = package.get("license") or "see license texts"
        numbers = [number(text, f"{name} {version} (`{file}`)") for file, text in collected[key]]
        if not numbers:
            without_text.append(key)
            ids = spdx_ids(expression)
            holders = ", ".join(package.get("authors") or []) or f"the authors of {name}"
            holders = re.sub(r"\s*<[^>]*>", "", holders)
            note = f"{name} {version} (standard text, the package ships no license file)"
            if "MIT" in ids:
                numbers.append(number(MIT_TEXT.format(holders=holders), note))
            if "Apache-2.0" in ids:
                numbers.append(number(apache_text, note))
            if not numbers:
                sys.exit(f"{name} {version}: no license text and no standard license ({expression})")
        links = ", ".join(f"[{n}](#text-{n})" for n in sorted(set(numbers)))
        rows.append(f"| {name} | {version} | {expression} | {links} |")

    art_rows = []
    for name, version, license_id, path, use in ARTWORK:
        n = number(read_text(path), f"{name} {version}")
        art_rows.append(f"| {name} | {version} | {license_id} | [{n}](#text-{n}) | {use} |")

    other_rows = []
    for key in sorted(locked - used, key=lambda k: (k[0].lower(), k[1])):
        other_rows.append(f"| {key[0]} | {key[1]} | {packages[key].get('license') or 'see the package'} |")

    out = [
        "# Third-party licenses",
        "",
        "Celata itself is licensed under GPL-3.0-only, see [LICENSE](LICENSE). This file lists the third-party",
        "components Celata contains or is built with, each with its license and the license texts it ships.",
        "",
        "- Where a component offers a choice of licenses, Celata uses it under a license that is compatible",
        "  with GPL-3.0-only. The Slint components are used under GPL-3.0-only.",
        "- Identical license texts appear only once; the numbers in the tables link to them.",
        f"- {len(without_text)} packages ship no license file. For them the standard text of the license they",
        "  name is given and marked as such.",
        "- The hide.me client is not part of Celata and is not distributed with it.",
        "",
        "This file is generated from `Cargo.lock`. Do not edit it by hand.",
        "",
        "## Rust crates in the Linux build",
        "",
        f"{len(rows)} crates are compiled into the Linux program or are needed to build it.",
        "",
        "| Crate | Version | License | Texts |",
        "|---|---|---|---|",
        *rows,
        "",
        "## Artwork",
        "",
        "| Name | Version | License | Text | Used for |",
        "|---|---|---|---|---|",
        *art_rows,
        "",
        "## License texts",
        "",
    ]
    for position, (text, users) in enumerate(texts, start=1):
        if any(line.startswith(FENCE) for line in text.split("\n")):
            sys.exit(f"license text {position} contains the fence line {FENCE}")
        out += [f"### Text {position}", "", "Applies to: " + "; ".join(users), "", FENCE, text, FENCE, ""]
    out += [
        "## Not part of the Linux build",
        "",
        f"`Cargo.lock` names {len(other_rows)} further crates. They belong to other operating systems or to",
        "features Celata does not enable, and are not compiled into the Linux program. They are listed here so",
        "that every entry of `Cargo.lock` is accounted for.",
        "",
        "| Crate | Version | License |",
        "|---|---|---|",
        *other_rows,
        "",
    ]
    return "\n".join(out), len(rows), len(other_rows), len(texts), without_text


def main():
    content, used, other, texts, without_text = build()
    if "--check" in sys.argv[1:]:
        with open(OUTPUT, encoding="utf-8") as file:
            current = file.read()
        if current != content:
            sys.exit(f"{OUTPUT} is out of date: run python3 packaging/third-party-licenses.py")
        print(f"{OUTPUT} is up to date")
        return
    with open(OUTPUT, "w", encoding="utf-8") as file:
        file.write(content)
    print(f"{OUTPUT}: {used} crates in the Linux build, {other} more in Cargo.lock, {texts} license texts, "
          f"{len(content) // 1024} KiB")
    print(f"without a license file ({len(without_text)}): " + ", ".join(f"{n} {v}" for n, v in without_text))


if __name__ == "__main__":
    main()
