# Copyright (c) Mysten Labs
# SPDX-License-Identifier: Apache-2.0

import argparse
import os
import re

ROOT = os.path.join(os.path.dirname(__file__), "../")
PATTERN = re.compile(
    '(\s*)(.+) = { git = "https://github.com/.+/move", (?:rev|branch)=".+"(,.*)? }(\s*)'
)


def parse_args():
    parser = argparse.ArgumentParser()
    subparser = parser.add_subparsers(
        dest="command",
        description="""
    Automatically manage the dependency path to Move repository.
    Command "local" switches the dependency from git to local path.
    Command "upgrade" upgrades the git revision. A repository can be
    specified if we want to use a fork instead of upstream.
    A revision or a branch also needs to be specified.
    """,
    )
    subparser.add_parser("local")
    upgrade = subparser.add_parser("upgrade")
    upgrade.add_argument("--repo", required=False, default="diem")
    upgrade_group = upgrade.add_mutually_exclusive_group(required=True)
    upgrade_group.add_argument("--rev")
    upgrade_group.add_argument("--branch")
    return parser.parse_args()


def scan_file(file, process_line, depth=0):
    new_content = []
    with open(file) as f:
        for line in f.readlines():
            new_content.append(process_line(line, depth))
    with open(file, "w") as f:
        f.writelines(new_content)


def scan_files(path, process_line, depth=0):
    for file in os.listdir(path):
        full_path = os.path.join(path, file)
        if os.path.isdir(full_path):
            scan_files(full_path, process_line, depth + 1)
        elif file == "Cargo.toml":
            scan_file(full_path, process_line, depth)


def switch_to_local():
    # Packages that don't directly map to a directory under move/language
    # go here as special cases. By default, we just use language/[name].
    path_map = {
        "move-bytecode-utils": "tools/move-bytecode-utils",
        "move-cli": "tools/move-cli",
        "move-core-types": "move-core/types",
        "move-package": "tools/move-package",
        "move-unit-test": "tools/move-unit-test",
        "move-vm-runtime": "move-vm/runtime",
        "move-vm-types": "move-vm/types",
    }

    def process_line(line, depth):
        m = PATTERN.match(line)
        if m:
            prefix = m.group(1)
            name = m.group(2)
            extra = "" if m.group(3) is None else m.group(3)
            postfix = m.group(4)
            go_back = "".join(["../"] * (depth + 1))
            return '{}{} = {{ path = "{}move/language/{}"{} }}{}'.format(
                prefix, name, go_back, path_map.get(name, name), extra, postfix
            )
        return line

    scan_files(ROOT, process_line)


def upgrade_revision(repo, rev, branch):
    assert (args.rev is None) != (args.branch is None)
    def process_line(line, _):
        m = PATTERN.match(line)
        if m:
            prefix = m.group(1)
            name = m.group(2)
            extra = "" if m.group(3) is None else m.group(3)
            postfix = m.group(4)
            return '{}{} = {{ git = "https://github.com/{}/move", {}="{}"{} }}{}'.format(
                prefix, name, repo,
                "branch" if branch else "rev",
                branch if branch else rev,
                extra,
                postfix
            )
        return line

    scan_files(ROOT, process_line)


args = parse_args()
if args.command == "local":
    switch_to_local()
else:
    assert args.command == "upgrade"
    upgrade_revision(args.repo, args.rev, args.branch)
