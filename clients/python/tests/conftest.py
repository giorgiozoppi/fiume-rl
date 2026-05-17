"""Shared pytest fixtures."""

import socket

import pytest


def _server_available(host: str = "127.0.0.1", port: int = 9000) -> bool:
    try:
        s = socket.create_connection((host, port), timeout=1)
        s.close()
        return True
    except OSError:
        return False


# ── marks ─────────────────────────────────────────────────────────────────────
requires_server = pytest.mark.skipif(
    not _server_available(),
    reason="rate-limit server not running on 127.0.0.1:9000",
)

# ── fixtures ──────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def tcp_client():
    """Live TCP client — skipped when the server is not running."""
    from client import RateLimitClient

    if not _server_available():
        pytest.skip("rate-limit server not running")
    with RateLimitClient("127.0.0.1", 9000) as c:
        yield c


@pytest.fixture(scope="module")
def http_client():
    """FastAPI TestClient — skipped when the server is not running.

    Imported lazily so that collection of unit tests never triggers a
    connection attempt.
    """
    if not _server_available():
        pytest.skip("rate-limit server not running")

    from app.main import app as fastapi_app  # noqa: PLC0415
    from fastapi.testclient import TestClient

    with TestClient(fastapi_app, raise_server_exceptions=False) as c:
        yield c
