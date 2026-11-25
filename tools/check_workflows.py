#!/usr/bin/env python3
"""Read every file in .github/workflows/ as YAML, and fail on one that does not read.

Usage, from the repository root:

    python3 tools/check_workflows.py [--root DIR]

A workflow file GitHub cannot parse fails every run at once, with no check to say why: an unquoted
`tests::memory:: --` in a `run:` line once did. This reads each file before a push, in `tools/gate.py`
and in CI's documents job, and names the file, the line, and what is wrong.

CI's Python on Linux has no YAML library, and the tools use the standard library alone, so this is a
reader of the part of YAML the workflows are written in, and strict: block mappings and sequences,
`|` literal blocks, one-line flow `[...]` and `{...}`, and plain, single-quoted, and double-quoted
scalars, every scalar read as text. What lies outside that part (an anchor, an alias, a tag, a `>`
block, a multi-line quoted or flow value, a tab in an indent) is a failure too, never a guess, so a
workflow stays within what this reads. A plain scalar holding `: ` or ending in `:`, a key given twice,
and an indent that fits nothing are YAML's own errors. Before reading the workflows it reads a sample
of each error and fails if any reads, so that the check cannot pass by reading too much.
`tools/test_check_workflows.py` holds what it reads to PyYAML's, where PyYAML is installed.
"""
import re
import sys
from pathlib import Path


class YamlError(Exception):
    def __init__(self, line, message):
        super().__init__(f"line {line}: {message}")
        self.line = line


INDICATORS = set("[]{},#&*!|>'\"%@`")


class Reader:
    def __init__(self, text):
        self.lines = text.split("\n")
        self.i = 0

    # Lines.

    def significant(self):
        """Move to the next line that is not blank or a comment; its index, or None at the end."""
        while self.i < len(self.lines):
            line = self.lines[self.i]
            stripped = line.lstrip(" ")
            if stripped.startswith("\t") or (line[:len(line) - len(stripped)].count("\t")):
                raise YamlError(self.i + 1, "a tab in an indent")
            if stripped and not stripped.startswith("#"):
                return self.i
            self.i += 1
        return None

    def indent(self, i):
        line = self.lines[i]
        return len(line) - len(line.lstrip(" "))

    # Nodes.

    def document(self):
        i = self.significant()
        if i is None:
            return None
        if self.lines[i].strip() in ("---", "..."):
            raise YamlError(i + 1, "a document marker is outside the part of YAML this reads")
        node = self.block(self.indent(i))
        i = self.significant()
        if i is not None:
            raise YamlError(i + 1, "an indent that fits nothing above it")
        return node

    def block(self, n):
        i = self.significant()
        content = self.lines[i][n:]
        if content == "-" or content.startswith("- "):
            return self.sequence(n)
        return self.mapping(n)

    def mapping(self, n):
        found = {}
        while True:
            i = self.significant()
            if i is None or self.indent(i) < n:
                return found
            if self.indent(i) > n:
                raise YamlError(i + 1, "an indent that fits nothing above it")
            content = self.lines[i][n:]
            if content == "-" or content.startswith("- "):
                return found if found else self.sequence(n)
            key, rest = self.key(content, i)
            if key in found:
                raise YamlError(i + 1, f"the key `{key}` is given twice")
            self.i = i + 1
            found[key] = self.value(rest, n, i, in_mapping=True)

    def sequence(self, n):
        items = []
        while True:
            i = self.significant()
            if i is None or self.indent(i) < n:
                return items
            if self.indent(i) > n:
                raise YamlError(i + 1, "an indent that fits nothing above it")
            content = self.lines[i][n:]
            if not (content == "-" or content.startswith("- ")):
                if items:
                    return items
                raise YamlError(i + 1, "a sequence item was expected")
            rest = content[1:]
            inner = len(rest) - len(rest.lstrip(" "))
            rest = rest.lstrip(" ")
            if rest and not rest.startswith("#") and (rest == "-" or rest.startswith("- ") or self.is_key(rest)):
                # A compact node: read it as if it began on its own line, at its own indent.
                m = n + 1 + inner
                self.lines[i] = " " * m + rest
                items.append(self.block(m))
            else:
                self.i = i + 1
                items.append(self.value(rest, n, i, in_mapping=False))

    def value(self, rest, n, i, in_mapping):
        """The value whose text starts as `rest` on line `i`, under a node at indent `n`."""
        if not rest or rest.startswith("#"):
            j = self.significant()
            if j is not None and self.indent(j) > n:
                return self.block(self.indent(j))
            if in_mapping and j is not None and self.indent(j) == n:
                content = self.lines[j][n:]
                if content == "-" or content.startswith("- "):
                    return self.sequence(n)
            return ""
        first = rest[0]
        if first in "|>":
            return self.literal(rest, n, i)
        if first in "[{":
            value, end = self.flow(rest, 0, i)
            self.trailing(rest[end:], i)
            return value
        if first in "'\"":
            value, end = self.quoted(rest, 0, i)
            self.trailing(rest[end:], i)
            return value
        return self.plain(rest, n, i)

    def literal(self, rest, n, i):
        m = re.fullmatch(r"\|([-+]?)\s*(#.*)?", rest)
        if not m:
            raise YamlError(i + 1, "only a `|` block, with `-` or `+` or neither, is in the part of YAML this reads")
        chomp = m.group(1)
        body, width = [], None
        j = i + 1
        while j < len(self.lines):
            line = self.lines[j]
            if line.strip() == "":
                body.append("")
                j += 1
                continue
            have = self.indent(j)
            if width is None:
                if have <= n:
                    break
                width = have
            if have < width:
                break
            body.append(line[width:])
            j += 1
        self.i = j
        while body and body[-1] == "" and chomp != "+":
            body.pop()
        if not body:
            return ""
        text = "\n".join(body)
        return text if chomp == "-" else text + "\n"

    def plain(self, rest, n, i):
        text = self.plain_line(rest, i)
        parts = [text]
        while True:
            j = self.i
            if j >= len(self.lines):
                break
            line = self.lines[j]
            if line.strip() == "" or line.lstrip(" ").startswith("#") or self.indent(j) <= n:
                break
            parts.append(self.plain_line(line.strip(), j))
            self.i = j + 1
        return " ".join(parts)

    def plain_line(self, text, i):
        if text[0] in INDICATORS or (text[0] in "-?:" and (len(text) == 1 or text[1] == " ")):
            raise YamlError(i + 1, f"a plain value cannot start with `{text[0]}`; quote it")
        cut = re.search(r"\s#", text)
        if cut:
            text = text[:cut.start()]
        text = text.rstrip()
        if ": " in text or text.endswith(":"):
            raise YamlError(i + 1, f"a plain value holds `: ` or ends in `:` ({text[:60]!r}); quote it")
        return text

    def is_key(self, content):
        try:
            self.key(content, None)
            return True
        except YamlError:
            return False

    def key(self, content, i):
        """(key, rest of the line) of a `key: value` line."""
        line = 0 if i is None else i + 1
        if content[0] in "'\"":
            key, end = self.quoted(content, 0, i)
            after = content[end:]
            if not (after == ":" or after.startswith(": ")):
                raise YamlError(line, "a quoted key is not followed by `: `")
            return key, after[1:].strip()
        m = re.search(r":( |$)", content)
        cut = re.search(r"\s#", content)
        if not m or (cut and cut.start() < m.start()):
            raise YamlError(line, "a line that is neither `key: value` nor `- item`")
        key = content[:m.start()]
        if not key or key[0] in INDICATORS or key[0] in "-?":
            raise YamlError(line, f"the key `{key}` is outside the part of YAML this reads; quote it")
        return key, content[m.end():].strip()

    def quoted(self, text, start, i):
        quote = text[start]
        out, j = [], start + 1
        while j < len(text):
            c = text[j]
            if quote == "'" and c == "'":
                if text[j + 1:j + 2] == "'":
                    out.append("'")
                    j += 2
                    continue
                return "".join(out), j + 1
            if quote == '"' and c == "\\":
                e = text[j + 1:j + 2]
                simple = {"n": "\n", "t": "\t", "\\": "\\", '"': '"', "/": "/", "0": "\0", " ": " "}
                if e in simple:
                    out.append(simple[e])
                    j += 2
                    continue
                if e == "u" and re.fullmatch(r"[0-9a-fA-F]{4}", text[j + 2:j + 6]):
                    out.append(chr(int(text[j + 2:j + 6], 16)))
                    j += 6
                    continue
                raise YamlError(i + 1, f"the escape `\\{e}` is outside the part of YAML this reads")
            if quote == '"' and c == '"':
                return "".join(out), j + 1
            out.append(c)
            j += 1
        raise YamlError(i + 1, "a quoted value is not closed on its line")

    def flow(self, text, j, i):
        """(value, index after it) of a one-line flow node starting at `text[j]`."""
        close = "]" if text[j] == "[" else "}"
        items, found = [], {}
        j += 1
        while True:
            while j < len(text) and text[j] == " ":
                j += 1
            if j >= len(text):
                raise YamlError(i + 1, "a flow value is not closed on its line")
            if text[j] == close:
                return (items if close == "]" else found), j + 1
            if close == "]":
                value, j = self.flow_scalar(text, j, i)
                items.append(value)
            else:
                key, j = self.flow_scalar(text, j, i)
                if text[j:j + 1] != ":":
                    raise YamlError(i + 1, f"the flow key `{key}` is not followed by `:`")
                j += 1
                while j < len(text) and text[j] == " ":
                    j += 1
                value, j = self.flow_scalar(text, j, i)
                if key in found:
                    raise YamlError(i + 1, f"the key `{key}` is given twice")
                found[key] = value
            while j < len(text) and text[j] == " ":
                j += 1
            if text[j:j + 1] == ",":
                j += 1
            elif text[j:j + 1] != close:
                raise YamlError(i + 1, "a flow value has something other than `,` between its items")

    def flow_scalar(self, text, j, i):
        if j < len(text) and text[j] in "[{":
            return self.flow(text, j, i)
        if j < len(text) and text[j] in "'\"":
            return self.quoted(text, j, i)
        m = re.match(r"[^,\[\]{}#]*?(?=\s*(?:[,\]}]|:\s|:$|$))", text[j:])
        value = m.group(0).strip() if m else ""
        if not value or value[0] in INDICATORS:
            raise YamlError(i + 1, "a flow value is outside the part of YAML this reads")
        return value, j + m.end()

    def trailing(self, rest, i):
        rest = rest.strip()
        if rest and not rest.startswith("#"):
            raise YamlError(i + 1, f"text after a closed value: `{rest[:40]}`")


def read(text):
    return Reader(text).document()


# Each a sample of an error the check must catch: none of them may read.
MUST_FAIL = {
    "a colon and a space in a plain value": "jobs:\n  t:\n    steps:\n      - run: cargo test tests::memory:: --\n",
    "a key given twice": "a: 1\na: 2\n",
    "an indent that fits nothing": "a:\n  b: 1\n   c: 2\n",
    "a tab in an indent": "a:\n\tb: 1\n",
    "an unclosed quote": "a: \"open\n",
    "an unclosed flow": "a: [x, y\n",
    "an alias": "a: *b\n",
}
MUST_READ = ("on:\n  push:\n    branches: [main]\njobs:\n  t:\n    runs-on: ${{ matrix.os }}\n"
             "    steps:\n      - name: \"Say: hello\"\n        run: |\n          cargo test 'tests::memory::' --\n"
             "      - { a: b, c: \"2\" }\n")


def self_check():
    problems = []
    for name, sample in MUST_FAIL.items():
        try:
            read(sample)
            problems.append(f"the check read a sample it must refuse: {name}")
        except YamlError:
            pass
    try:
        read(MUST_READ)
    except YamlError as error:
        problems.append(f"the check refused a sample it must read: {error}")
    return problems


def check(root):
    problems = self_check()
    folder = root / ".github" / "workflows"
    files = sorted(p for p in folder.iterdir() if p.suffix in (".yml", ".yaml")) if folder.is_dir() else []
    for path in files:
        rel = path.relative_to(root).as_posix()
        try:
            data = read(path.read_text(encoding="utf-8"))
        except YamlError as error:
            problems.append(f"{rel}: {error}")
            continue
        if not isinstance(data, dict) or "jobs" not in data or "on" not in data:
            problems.append(f"{rel}: a workflow is a mapping with `on` and `jobs`")
    return problems, len(files)


def main(argv):
    root = Path(argv[argv.index("--root") + 1]).resolve() if "--root" in argv else Path(__file__).resolve().parent.parent
    problems, count = check(root)
    for p in problems:
        print("-", p)
    print(f"{count} workflow file(s) read; {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
