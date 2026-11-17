"""Anything2MCP for Python: the ``anything2mcp`` command and a thin client
of the local engine.

    import anything2mcp

    with anything2mcp.connect(project=".") as client:
        status = client.call("engine.status")
        print(status.ok, status.result)

The client holds no engine logic. It sends an action and its input to the
engine that runs as this user, over the local channel, and returns the
engine's response as it was written. When no engine runs, ``connect``
runs the installed program's ``anything2mcp engine start``, as any command
would start one, unless it is given ``start=False``.
"""

from ._channel import ChannelError, socket_path
from ._client import LOCAL_API, Client, Error, Response, connect
from ._binary import binary_path

__all__ = [
    "LOCAL_API",
    "ChannelError",
    "Client",
    "Error",
    "Response",
    "binary_path",
    "connect",
    "socket_path",
]
