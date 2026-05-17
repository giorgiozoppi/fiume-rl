"""Unit tests for client.hmac_utils.verify_rate_hmac (X-RateLimit-Mac header)."""

import hashlib
import hmac
import time

import pytest

from client.hmac_utils import verify_rate_hmac

SECRET = "test-secret-key"
CLIENT_ID = "alice"
RESOURCE = "/api/orders"
ALLOWED = True
REMAINING = 9
RETRY_AFTER_MS = 0


def _make_header(
    secret: str = SECRET,
    ts: int | None = None,
    client_id: str = CLIENT_ID,
    resource: str = RESOURCE,
    allowed: bool = ALLOWED,
    remaining: int = REMAINING,
    retry_after_ms: int = RETRY_AFTER_MS,
) -> str:
    if ts is None:
        ts = int(time.time() * 1_000)
    msg = f"{client_id}|{resource}|{allowed}|{remaining}|{retry_after_ms}|{ts}"
    sig = hmac.new(secret.encode(), msg.encode(), hashlib.sha256).hexdigest()
    return f"t={ts},v={sig}"


class TestVerifyRateHmac:
    def test_valid_header_returns_true(self):
        header = _make_header()
        assert verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_wrong_secret_returns_false(self):
        header = _make_header()
        assert not verify_rate_hmac(
            header,
            "wrong-secret",
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_tampered_remaining_returns_false(self):
        header = _make_header(remaining=REMAINING)
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING + 1,  # tampered
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_tampered_allowed_returns_false(self):
        header = _make_header(allowed=True)
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=False,  # tampered
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_stale_timestamp_returns_false(self):
        old_ts = int(time.time() * 1_000) - 120_000  # 2 minutes ago
        header = _make_header(ts=old_ts)
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_custom_max_age_accepts_older_timestamp(self):
        old_ts = int(time.time() * 1_000) - 10_000  # 10 s ago
        header = _make_header(ts=old_ts)
        assert verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
            max_age_ms=30_000,  # allow up to 30 s
        )

    def test_malformed_header_returns_false(self):
        assert not verify_rate_hmac(
            "not-a-valid-header",
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_missing_t_field_returns_false(self):
        header = f"v=abc123"
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_different_client_id_returns_false(self):
        header = _make_header(client_id="alice")
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id="bob",  # different
            resource=RESOURCE,
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )

    def test_different_resource_returns_false(self):
        header = _make_header(resource="/api/orders")
        assert not verify_rate_hmac(
            header,
            SECRET,
            client_id=CLIENT_ID,
            resource="/api/users",  # different
            allowed=ALLOWED,
            remaining=REMAINING,
            retry_after_ms=RETRY_AFTER_MS,
        )
