"""TCP client for the rate-limiting server.

Protocol: 4-byte little-endian length prefix + FlatBuffer payload.

Usage (CLI):
    rate-limit-client [host:port] [client_id] [num_requests] [delay_ms]
    rate-limit-client 127.0.0.1:9000 alice 10 200

Usage (library):
    from client import RateLimitClient

    with RateLimitClient("127.0.0.1", 9000) as c:
        resp = c.check("alice", resource="/api/orders", cost=1)
        if resp.allowed:
            ...
"""

from __future__ import annotations

import socket
import struct
import sys
import time

import flatbuffers
import flatbuffers.encode
import flatbuffers.number_types
from flatbuffers.table import Table

from .models import RateLimitResponse

# ── VTable offsets (must match schema/messages.fbs) ──────────────────────────
_RESP_VT_ALLOWED       = 4
_RESP_VT_REMAINING     = 6
_RESP_VT_RETRY_AFTER_MS = 8
_RESP_VT_REASON        = 10


def _build_request(client_id: str, resource: str, cost: int) -> bytes:
    builder = flatbuffers.Builder(256)
    cid_off = builder.CreateString(client_id)
    res_off = builder.CreateString(resource)
    builder.StartObject(3)
    builder.PrependUOffsetTRelativeSlot(0, cid_off, 0)
    builder.PrependUOffsetTRelativeSlot(1, res_off, 0)
    builder.PrependUint32Slot(2, cost, 1)
    root = builder.EndObject()
    builder.Finish(root)
    return bytes(builder.Output())


def _parse_response(buf: bytes) -> RateLimitResponse:
    raw = bytearray(buf)
    root_off = flatbuffers.encode.Get(
        flatbuffers.number_types.UOffsetTFlags.packer_type, raw, 0
    )
    tab = Table(raw, root_off)

    def _bool(vt: int) -> bool:
        o = tab.Offset(vt)
        return bool(tab.Get(flatbuffers.number_types.BoolFlags, tab.Pos + o)) if o else False

    def _i64(vt: int) -> int:
        o = tab.Offset(vt)
        return tab.Get(flatbuffers.number_types.Int64Flags, tab.Pos + o) if o else 0

    def _str(vt: int) -> str:
        o = tab.Offset(vt)
        return tab.String(tab.Pos + o).decode() if o else ""

    return RateLimitResponse(
        allowed=_bool(_RESP_VT_ALLOWED),
        remaining=_i64(_RESP_VT_REMAINING),
        retry_after_ms=_i64(_RESP_VT_RETRY_AFTER_MS),
        reason=_str(_RESP_VT_REASON),
    )


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("server closed connection")
        buf += chunk
    return bytes(buf)


class RateLimitClient:
    """Synchronous TCP client.  Use as a context manager or call close() manually."""

    def __init__(self, host: str = "127.0.0.1", port: int = 9000) -> None:
        self._sock = socket.create_connection((host, port))
        self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def check(self, client_id: str, resource: str = "/", cost: int = 1) -> RateLimitResponse:
        payload = _build_request(client_id, resource, cost)
        self._sock.sendall(struct.pack("<I", len(payload)) + payload)
        (length,) = struct.unpack("<I", _recv_exact(self._sock, 4))
        return _parse_response(_recv_exact(self._sock, length))

    def close(self) -> None:
        self._sock.close()

    def __enter__(self) -> "RateLimitClient":
        return self

    def __exit__(self, *_) -> None:
        self.close()


def main() -> None:
    args = sys.argv[1:]
    addr         = args[0] if len(args) > 0 else "127.0.0.1:9000"
    client_id    = args[1] if len(args) > 1 else "client_1"
    num_requests = int(args[2]) if len(args) > 2 else 20
    delay_ms     = int(args[3]) if len(args) > 3 else 100

    host, port_str = addr.rsplit(":", 1)

    print(f"→ connecting to {addr}  client_id={client_id}  requests={num_requests}  delay={delay_ms}ms\n")
    print(f"{'#':>3}  {'allowed':<8}  {'remaining':>10}  {'retry_after_ms':>15}  reason")
    print("─" * 72)

    with RateLimitClient(host, int(port_str)) as c:
        for i in range(1, num_requests + 1):
            resp = c.check(client_id, resource="/api/v1/data")
            print(
                f"{i:>3}  {'✓ YES' if resp.allowed else '✗ NO ':>8}"
                f"  {resp.remaining:>10}  {resp.retry_after_ms:>15}  {resp.reason}"
            )
            if delay_ms > 0:
                time.sleep(delay_ms / 1000)


if __name__ == "__main__":
    main()
