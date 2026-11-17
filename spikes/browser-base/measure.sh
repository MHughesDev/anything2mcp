#!/bin/sh
# Throwaway. Prints, for each candidate, the number of crates it adds to the
# shared harness and the result of the licence check against the closed list.
# Usage: sh measure.sh /path/to/cargo-deny
set -u
here=$(cd "$(dirname "$0")" && pwd)
deny=${1:-cargo-deny}
export CARGO_TARGET_DIR="$here/target/meta"
count() { (cd "$1" && cargo tree -e normal --prefix none 2>/dev/null | sed 's/ (\*)//' | sort -u | wc -l); }
base=$(count "$here/z-baseline")
for dir in "$here"/a-own-pipe "$here"/b-chromiumoxide "$here"/c-thirtyfour "$here"/checked-not-run/*/; do
    total=$(count "$dir")
    refused=$("$deny" --manifest-path "$dir/Cargo.toml" --config "$here/../../deny.toml" -f json check licenses 2>&1 \
        | python3 -c '
import json, sys
found = set()
for line in sys.stdin:
    if not line.startswith("{"):
        continue
    fields = json.loads(line).get("fields", {})
    if fields.get("code") == "rejected" and fields.get("graphs"):
        crate = fields["graphs"][0]["Krate"]
        found.add(crate["name"] + " " + crate["version"])
print(", ".join(sorted(found)))')
    echo "$(basename "$dir"): adds $((total - base)) crates on Linux; refused by the licence list: ${refused:-none}"
done
