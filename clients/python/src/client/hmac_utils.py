"""HMAC-SHA256 verification for the X-RateLimit-Mac response header.

The rate-limit HTTP server sets this header when ``strict_security: true``:

    X-RateLimit-Mac: t=<unix_ms>,v=<hmac_sha256_hex>

Signed payload (pipe-delimited, same order as the server):

    "{client_id}|{resource}|{allowed}|{remaining}|{retry_after_ms}|{t}"

Usage::

    from client.hmac_utils import verify_rate_hmac

    ok = verify_rate_hmac(
        response.headers["x-ratelimit-mac"],
        secret="change-me",
        client_id="alice",
        resource="/api/orders",
        allowed=True,
        remaining=9,
        retry_after_ms=0,
    )
    if not ok:
        raise SecurityError("rate-limit response failed HMAC verification")
"""

from __future__ import annotations

import hashlib
import hmac
import time


def verify_rate_hmac(
    header_value: str,
    secret: str,
    *,
    client_id: str,
    resource: str,
    allowed: bool,
    remaining: int,
    retry_after_ms: int,
    max_age_ms: int = 60_000,
) -> bool:
    """Return ``True`` iff the X-RateLimit-Mac header is authentic and fresh.

    Parameters
    ----------
    header_value:
        Raw value of the ``X-RateLimit-Mac`` response header.
    secret:
        Shared HMAC secret (must match ``hmac_secret`` in server config.yaml).
    client_id / resource / allowed / remaining / retry_after_ms:
        Fields from the rate-limit decision — used to reconstruct the signed payload.
    max_age_ms:
        Maximum acceptable age of the timestamp embedded in the header (default 60 s).
        Prevents replay attacks (Fu et al., 2001 §3.3).
    """
    try:
        parts = dict(p.split("=", 1) for p in header_value.split(","))
        ts = int(parts["t"])
        received_sig = parts["v"]
    except (KeyError, ValueError):
        return False

    now_ms = int(time.time() * 1_000)
    if abs(now_ms - ts) > max_age_ms:
        return False

    msg = f"{client_id}|{resource}|{allowed}|{remaining}|{retry_after_ms}|{ts}"
    expected_sig = hmac.new(
        secret.encode(),
        msg.encode(),
        hashlib.sha256,
    ).hexdigest()

    return hmac.compare_digest(expected_sig, received_sig)
