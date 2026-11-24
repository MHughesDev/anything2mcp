"""Makes a wheel of a test provider package, by hand, for the entry-point
test (package F3). Usage:

    make_wheel.py OUT_FOLDER DISTRIBUTION ENTRY_NAME ENTRY_VALUE PACKAGE MARKER MANIFEST [PROGRAM]

The wheel holds a package folder PACKAGE with an `__init__.py` that writes
the file MARKER when it is imported (so that a test can prove nothing
imported it), the manifest as `anything2mcp-provider.toml`, and, when a
PROGRAM file is given, that file as `bin/<its name>` with the executable
mark. Its `entry_points.txt` declares ENTRY_NAME = ENTRY_VALUE in the group
`anything2mcp.providers`. The Python package is only a carrier.
"""

import base64
import hashlib
import sys
import zipfile
from pathlib import Path

out, dist, entry, value, package, marker, manifest = sys.argv[1:8]
program = Path(sys.argv[8]) if len(sys.argv) > 8 else None
version = "0.1.0"
info = f"{dist}-{version}.dist-info"

files = {
    f"{package}/__init__.py": f"open({marker!r}, 'w').write('imported')\n".encode(),
    f"{package}/anything2mcp-provider.toml": Path(manifest).read_bytes(),
    f"{info}/METADATA": f"Metadata-Version: 2.1\nName: {dist}\nVersion: {version}\n".encode(),
    f"{info}/WHEEL": b"Wheel-Version: 1.0\nGenerator: tests\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
    f"{info}/entry_points.txt": f"[anything2mcp.providers]\n{entry} = {value}\n".encode(),
}
modes = {}
if program is not None:
    name = f"{package}/bin/{program.name}"
    files[name] = program.read_bytes()
    modes[name] = 0o100755


def record_line(name, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


record = [record_line(name, data) for name, data in files.items()]
record.append(f"{info}/RECORD,,")
files[f"{info}/RECORD"] = ("\n".join(record) + "\n").encode()

wheel = Path(out) / f"{dist}-{version}-py3-none-any.whl"
with zipfile.ZipFile(wheel, "w") as archive:
    for name, data in files.items():
        entry_info = zipfile.ZipInfo(name)
        entry_info.external_attr = (modes.get(name, 0o100644)) << 16
        archive.writestr(entry_info, data)
print(wheel)
