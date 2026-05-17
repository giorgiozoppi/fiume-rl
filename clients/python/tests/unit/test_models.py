"""Unit tests for Pydantic models — no network required."""

import pytest
from pydantic import ValidationError

from client.models import CheckRequest, CheckResponse, RateLimitResponse


class TestRateLimitResponse:
    def test_allowed_response(self):
        r = RateLimitResponse(allowed=True, remaining=9, retry_after_ms=0, reason="ok")
        assert r.allowed is True
        assert r.remaining == 9
        assert r.retry_after_secs == 0

    def test_denied_response(self):
        r = RateLimitResponse(allowed=False, remaining=0, retry_after_ms=1500, reason="limit hit")
        assert r.allowed is False
        assert r.retry_after_secs == 1          # floor(1500 / 1000) = 1

    def test_retry_after_secs_minimum_is_one(self):
        r = RateLimitResponse(allowed=False, remaining=0, retry_after_ms=50, reason="x")
        assert r.retry_after_secs == 1          # max(1, 50//1000) = 1

    def test_remaining_non_negative(self):
        with pytest.raises(ValidationError):
            RateLimitResponse(allowed=True, remaining=-1, retry_after_ms=0, reason="")

    def test_retry_after_ms_non_negative(self):
        with pytest.raises(ValidationError):
            RateLimitResponse(allowed=False, remaining=0, retry_after_ms=-1, reason="")

    def test_reason_defaults_to_empty_string(self):
        r = RateLimitResponse(allowed=True, remaining=5, retry_after_ms=0)
        assert r.reason == ""

    def test_model_dump_roundtrip(self):
        r = RateLimitResponse(allowed=True, remaining=3, retry_after_ms=0, reason="ok")
        restored = RateLimitResponse(**r.model_dump())
        assert restored == r


class TestCheckRequest:
    def test_defaults(self):
        q = CheckRequest(client_id="alice")
        assert q.resource == "/"
        assert q.cost == 1

    def test_custom_values(self):
        q = CheckRequest(client_id="bob", resource="/api/orders", cost=5)
        assert q.cost == 5

    def test_empty_client_id_rejected(self):
        with pytest.raises(ValidationError):
            CheckRequest(client_id="")

    def test_cost_below_one_rejected(self):
        with pytest.raises(ValidationError):
            CheckRequest(client_id="x", cost=0)


class TestCheckResponse:
    def test_from_decision(self):
        decision = RateLimitResponse(allowed=True, remaining=7, retry_after_ms=0, reason="ok")
        resp = CheckResponse.from_decision(decision)
        assert resp.allowed is True
        assert resp.remaining == 7
        assert resp.retry_after_ms == 0
        assert resp.reason == "ok"

    def test_serialises_to_json(self):
        resp = CheckResponse(allowed=False, remaining=0, retry_after_ms=500, reason="limit")
        data = resp.model_dump()
        assert data["allowed"] is False
        assert data["retry_after_ms"] == 500
