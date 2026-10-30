#!/usr/bin/env python3

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent

SKIP_DIRS = {
    ".git",
    "target",
    "vendor",
}


def source_files():
    result = subprocess.run(
        ["git", "ls-files", "-co", "--exclude-standard", "-z", "--", "*.rs", "*.wave"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )

    for entry in result.stdout.split(b"\x00"):
        if not entry:
            continue;

        name = entry.decode("utf-8")
        path = ROOT / name

        if any(part in SKIP_DIRS for part in path.parts):
            continue

        if path.is_file():
            yield path


def strip_lexical_content(line, state):
    result = []
    i = 0

    while i < len(line):
        if state["raw_string_hashes"] is not None:
            terminator = '"' + "#" * state["raw_string_hashes"]

            if line.startswith(terminator, i):
                state["raw_string_hashes"] = None
                result.extend(" " * len(terminator))
                i += len(terminator)
            else:
                result.append(" ")
                i += 1
            continue

        if state["block_comment"] > 0:
            if line.startswith("/*", i):
                state["block_comment"] += 1
                result.extend("  ")
                i += 2
            elif line.startswith("*/", i):
                state["block_comment"] -= 1
                result.extend("  ")
                i += 2
            else:
                result.append(" ")
                i += 1
            continue

        if state["string"]:
            if state["escape"]:
                state["escape"] = False
                result.append(" ")
                i += 1
            elif line[i] == "\\":
                state["escape"] = True
                result.append(" ")
                i += 1
            elif line[i] == '"':
                state["string"] = False
                result.append(" ")
                i += 1
            else:
                result.append(" ")
                i += 1
            continue

        if state["char"]:
            if state["escape"]:
                state["escape"] = False
                result.append(" ")
                i += 1
            elif line[i] == "\\":
                state["escape"] = True
                result.append(" ")
                i += 1
            elif line[i] == "'":
                state["char"] = False
                result.append(" ")
                i += 1
            else:
                result.append(" ")
                i += 1
            continue

        if line.startswith("//", i):
            result.extend(" " * (len(line) - i))
            break

        if line.startswith("/*", i):
            state["block_comment"] += 1
            result.extend("  ")
            i += 2
            continue

        raw_string = re.match(r'(?:b|c)?r(#{0,})"', line[i:])

        if raw_string:
            state["raw_string_hashes"] = len(raw_string.group(1))
            result.extend(" " * len(raw_string.group(0)))
            i += len(raw_string.group(0))
            continue

        if line[i] == '"':
            state["string"] = True
            result.append(" ")
            i += 1
            continue

        if line[i] == "'":
            # A Rust label or lifetime starts with an identifier but has no
            # closing apostrophe. A later quote can belong to a raw string on
            # the same line, so do not use it as this token's terminator.
            lifetime = re.match(r"'[A-Za-z_]\w*", line[i:])
            if lifetime and not line.startswith("'", i + len(lifetime.group(0))):
                result.append("'")
                i += 1
                continue

            closing_quote = line.find("'", i + 1)
            is_char_literal = (
                closing_quote != -1
                and not any(char.isspace() for char in line[i + 1:closing_quote])
            )

            if not is_char_literal:
                result.append("'")
                i += 1
                continue

            state["char"] = True
            result.append(" ")
            i += 1
            continue

        result.append(line[i])
        i += 1

    return "".join(result)


def is_item_start(text):
    stripped = text.lstrip()
    return re.match(
        r"^(?:(?:pub(?:\s*\([^)]*\))?|export\([^)]*\))\s+)?(?:"
        r"(?:(?:default|const|async|unsafe)\s+)*(?:extern(?:\s+\"[^\"]+\")?\s+)?fn\b|"
        r"fun\s+[A-Za-z_]\w*\s*\(|"
        r"(?:(?:unsafe|auto)\s+)*(?:trait|impl)\b|"
        r"enum\b|struct\b|union\b|mod\b|type\b|use\b|static\b|const\b|"
        r"(?:unsafe\s+)?extern(?:\s+\"[^\"]+\")?\s*(?:\{|crate\b)|"
        r"macro_rules!)",
        stripped,
    ) is not None


def item_ranges(lines):
    state = {
        "block_comment": 0,
        "string": False,
        "char": False,
        "escape": False,
        "raw_string_hashes": None,
    }

    depth = 0
    active = []
    ranges = []
    next_id = 0

    for index, line in enumerate(lines):
        lexical = strip_lexical_content(line, state)
        depth_before = depth

        if is_item_start(lexical):
            active.append({
                "id": next_id,
                "parent_id": active[-1]["id"] if active else None,
                "start": index,
                "parent_depth": depth_before,
                "body_seen": False,
            })
            next_id += 1

        opens = lexical.count("{")
        closes = lexical.count("}")

        if (
            active
            and not active[-1]["body_seen"]
            and opens == 0
            and lexical.rstrip().endswith(";")
        ):
            item = active.pop()
            ranges.append((item["start"], index, item["parent_id"]))

        if active and opens > 0:
            active[-1]["body_seen"] = True

        depth += opens
        depth -= closes

        while active and active[-1]["body_seen"] and depth == active[-1]["parent_depth"]:
            item = active.pop()
            ranges.append((item["start"], index, item["parent_id"]))

    return ranges


def add_item_spacing(text):
    lines = text.splitlines(keepends=True)
    ranges = item_ranges(lines)

    if not ranges:
        return text

    insert_after = set()

    siblings = {}
    for start, end, parent_id in ranges:
        siblings.setdefault(parent_id, []).append((start, end))

    for items in siblings.values():
        items.sort()

        for (_, end), (next_start, _) in zip(items, items[1:]):
            between = lines[end + 1:next_start]
            blank_count = sum(1 for line in between if not line.strip())

            if blank_count == 0:
                insert_after.add(end)

    output = []

    for index, line in enumerate(lines):
        output.append(line)

        if index in insert_after:
            output.append("\n")

    return "".join(output)


def format_file(path, check):
    original = path.read_text(encoding="utf-8")
    formatted = add_item_spacing(original)

    if formatted == original:
        return False

    if check:
        print(path.relative_to(ROOT))
        return True

    path.write_text(formatted, encoding="utf-8")
    print(f"formatted {path.relative_to(ROOT)}")
    return True


def run_rustfmt(check):
    command = ["cargo", "fmt", "--all"]

    if check:
        command.extend(["--", "--check"])

    subprocess.run(command, cwd=ROOT, check=True)


def main():
    parser = argparse.ArgumentParser(
        description="Format Rust and Wave sources with blank lines between items.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check formatting without modifying files.",
    )
    args = parser.parse_args()

    run_rustfmt(args.check)

    changed = False

    for path in source_files():
        changed |= format_file(path, args.check)

    if args.check and changed:
        print(
            "Rust or Wave item spacing does not match the project style.",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
