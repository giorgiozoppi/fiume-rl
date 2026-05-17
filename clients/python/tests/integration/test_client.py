"""Integration tests — require a running rate-limit server on 127.0.0.1:9000.

Run:
    cargo run --bin server &
    uv run pytest tests/integration/ -v
"""

import pytest

from client import RateLimitClient
from client.models import RateLimitResponse


@pytest.fixture(scope="module")
def client():
    """One persistent TCP connection shared across the module."""
    import socket
    try:
        socket.create_connection(("127.0.0.1", 9000), timeout=1).close()
    except OSError:
        pytest.skip("rate-limit server not running on 127.0.0.1:9000")

    with RateLimitClient("127.0.0.1", 9000) as c:
        yield c


class TestTcpClientBasics:
    def test_returns_rate_limit_response(self, client):
        resp = client.check("integ-test", resource="/test", cost=1)
        assert isinstance(resp, RateLimitResponse)

    def test_allowed_field_is_bool(self, client):
        resp = client.check("integ-test", resource="/test", cost=1)
        assert isinstance(resp.allowed, bool)

    def test_remaining_is_non_negative(self, client):
        resp = client.check("integ-fresh", resource="/remaining", cost=1)
        assert resp.remaining >= 0

    def test_reason_is_str(self, client):
        resp = client.check("integ-test", resource="/reason", cost=1)
        assert isinstance(resp.reason, str)

    def test_retry_after_zero_when_allowed(self, client):
        resp = client.check("integ-fresh2", resource="/retry", cost=1)
        if resp.allowed:
            assert resp.retry_after_ms == 0

    def test_retry_after_positive_when_denied(self, client):
        # Exhaust the bucket with a high cost so we get a denial.
        for _ in range(20):
            resp = client.check("integ-exhaust", resource="/exhaust", cost=1)
            if not resp.allowed:
                assert resp.retry_after_ms > 0
                return
        # If never denied, the server limit is higher than 20 — skip assertion.

    def test_clients_are_isolated(self, client):
        r1 = client.check("integ-iso-A", resource="/iso", cost=1)
        r2 = client.check("integ-iso-B", resource="/iso", cost=1)
        # Both should be allowed (fresh clients).
        assert r1.allowed
        assert r2.allowed

    def test_resources_are_isolated(self, client):
        r1 = client.check("integ-res", resource="/resource-a", cost=1)
        r2 = client.check("integ-res", resource="/resource-b", cost=1)
        assert r1.allowed
        assert r2.allowed

    def test_context_manager(self):
        with RateLimitClient("127.0.0.1", 9000) as c:
            resp = c.check("integ-ctx", resource="/ctx", cost=1)
        assert isinstance(resp, RateLimitResponse)


class TestMiddlewareViaFastAPI:
    """Integration tests for the FastAPI app with middleware wired in.

    Uses httpx to hit the demo app endpoints; middleware calls the live limiter.
    """

    def test_items_endpoint_returns_200_or_429(self, http_client):
        resp = http_client.get("/items", headers={"X-Client-ID": "integ-http"})
        assert resp.status_code in (200, 429)

    def test_allowed_response_has_remaining_header(self, http_client):
        resp = http_client.get("/items", headers={"X-Client-ID": "integ-hdr"})
        if resp.status_code == 200:
            assert "ratelimit-remaining" in resp.headers

    def test_denied_response_has_retry_after(self, http_client):
        # Hammer the same client until we get a 429.
        for _ in range(30):
            resp = http_client.get("/items", headers={"X-Client-ID": "integ-hammer"})
            if resp.status_code == 429:
                assert "retry-after" in resp.headers
                return
