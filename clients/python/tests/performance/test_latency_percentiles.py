"""Latency-percentile tests for the rate-limit service.

Verifies the NFR: a rate-limit check must add < 1 ms to p99 request latency
(server processing time). On localhost the round-trip also includes socket
overhead, so thresholds are deliberately generous.

Each test class provides two complementary views of the same SLO:

  Percentile view  — "p99 ≤ X ms"
  Percentage view  — "≥ 99 % of requests complete within X ms"

Both views are equivalent; the percentage view makes the distribution
directly human-readable in CI output.

Calibrated against the observed locust baseline (50 users, 30 s, local):
  HTTP: p50=1 ms  p95=2 ms  p99=3 ms  max=25 ms  (~132 req/s)
  TCP:  p50<1 ms  p95=1 ms  p99=1 ms  max= 4 ms  (~134 req/s combined)

Run::

    cargo run --bin server &
    uv run pytest tests/performance/ -v -s

Environment variables
---------------------
PERF_TCP_ADDR      host:port for the TCP server   (default 127.0.0.1:9000)
PERF_HTTP_URL      base URL for the HTTP server   (default http://127.0.0.1:8080)
PERF_SAMPLES       number of round-trips to time  (default 500)
PERF_WARMUP        warm-up calls before recording (default 20)

Percentile thresholds (milliseconds, overridable via env vars)
--------------------------------------------------------------
PERF_TCP_P50_MS    (default  1.0)
PERF_TCP_P95_MS    (default  5.0)
PERF_TCP_P99_MS    (default 10.0)
PERF_HTTP_P50_MS   (default  5.0)
PERF_HTTP_P95_MS   (default 15.0)
PERF_HTTP_P99_MS   (default 30.0)
"""

from __future__ import annotations

import os
import socket
import statistics
import time
from typing import Callable

import pytest

# ── helpers ───────────────────────────────────────────────────────────────────

def _env_float(name: str, default: float) -> float:
    return float(os.environ.get(name, default))


def _env_int(name: str, default: int) -> int:
    return int(os.environ.get(name, default))


def _percentile(data: list[float], p: float) -> float:
    """Return the p-th percentile (0–100) of *data* using nearest-rank."""
    if not data:
        raise ValueError("empty data")
    s = sorted(data)
    idx = max(0, min(int(len(s) * p / 100), len(s) - 1))
    return s[idx]


def _pct_within(data: list[float], threshold_ms: float) -> float:
    """Return the percentage of samples whose latency is ≤ *threshold_ms*."""
    return 100.0 * sum(1 for d in data if d <= threshold_ms) / len(data)


# Bucket breakpoints used for the distribution table.
# Chosen to span both TCP (<1 ms) and HTTP (~25 ms max) ranges.
_DIST_BUCKETS_MS = [0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 25.0, 50.0]


def _measure(fn: Callable[[], object], n: int, warmup: int) -> list[float]:
    """Call *fn* (warmup + n) times, return the last *n* durations in ms."""
    for _ in range(warmup):
        fn()
    durations: list[float] = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        durations.append((time.perf_counter() - t0) * 1_000)
    return durations


def _report(label: str, samples: list[float]) -> dict[str, float]:
    """Print a full latency report and return the key stats as a dict.

    The returned dict contains:
      p50, p75, p90, p95, p99, p99_9        — latency percentiles (ms)
      pct_within_<N>                         — % of samples ≤ N ms, for
                                               each bucket in _DIST_BUCKETS_MS
    """
    n = len(samples)
    mean = statistics.mean(samples)
    stdev = statistics.stdev(samples) if n > 1 else 0.0

    p50   = _percentile(samples, 50)
    p75   = _percentile(samples, 75)
    p90   = _percentile(samples, 90)
    p95   = _percentile(samples, 95)
    p99   = _percentile(samples, 99)
    p99_9 = _percentile(samples, 99.9)
    p_max = max(samples)

    pct = {b: _pct_within(samples, b) for b in _DIST_BUCKETS_MS}

    # ── percentile table ──────────────────────────────────────────────────────
    lines = [
        f"\n{'─' * 62}",
        f"  {label}  (n={n})",
        f"{'─' * 62}",
        f"  Latency percentiles",
        f"    mean  = {mean:7.3f} ms  (σ={stdev:.3f} ms)",
        f"    p50   = {p50:7.3f} ms",
        f"    p75   = {p75:7.3f} ms",
        f"    p90   = {p90:7.3f} ms",
        f"    p95   = {p95:7.3f} ms",
        f"    p99   = {p99:7.3f} ms",
        f"    p99.9 = {p99_9:7.3f} ms",
        f"    max   = {p_max:7.3f} ms",
        f"",
        f"  % of requests within threshold",
    ]

    for b, pct_val in sorted(pct.items()):
        bar_len = int(pct_val / 2)          # 50 chars → 100 %
        bar = "█" * bar_len + "░" * (50 - bar_len)
        lines.append(f"    ≤ {b:5.1f} ms  {bar}  {pct_val:6.2f}%")

    lines.append(f"{'─' * 62}")
    print("\n".join(lines))

    result = {
        "p50": p50, "p75": p75, "p90": p90,
        "p95": p95, "p99": p99, "p99_9": p99_9,
    }
    for b, pct_val in pct.items():
        result[f"pct_within_{b}"] = pct_val
    return result


# ── fixtures ──────────────────────────────────────────────────────────────────

_TCP_ADDR = os.environ.get("PERF_TCP_ADDR", "127.0.0.1:9000")
_TCP_HOST, _TCP_PORT = _TCP_ADDR.rsplit(":", 1)
_TCP_PORT_INT = int(_TCP_PORT)

_HTTP_URL = os.environ.get("PERF_HTTP_URL", "http://127.0.0.1:8080")

_SAMPLES = _env_int("PERF_SAMPLES", 500)
_WARMUP  = _env_int("PERF_WARMUP", 20)


def _tcp_reachable() -> bool:
    try:
        socket.create_connection((_TCP_HOST, _TCP_PORT_INT), timeout=1).close()
        return True
    except OSError:
        return False


@pytest.fixture(scope="module")
def tcp_client():
    if not _tcp_reachable():
        pytest.skip(f"TCP server not reachable at {_TCP_ADDR}")
    from client import RateLimitClient
    with RateLimitClient(_TCP_HOST, _TCP_PORT_INT) as c:
        yield c


# ── TCP percentile + percentage tests ─────────────────────────────────────────

class TestTcpLatencyPercentiles:
    """Assert round-trip latency SLOs for the FlatBuffers TCP path.

    Observed baseline (locust, 50 users, local loopback):
      p50 < 1 ms  |  p95 = 1 ms  |  p99 = 1 ms  |  max = 4 ms
    """

    p50_limit = _env_float("PERF_TCP_P50_MS",  1.0)
    p95_limit = _env_float("PERF_TCP_P95_MS",  5.0)
    p99_limit = _env_float("PERF_TCP_P99_MS", 10.0)

    @pytest.fixture(autouse=True)
    def _require_tcp(self, tcp_client):
        self._client = tcp_client

    def _check(self) -> None:
        self._client.check("perf-tcp", resource="/api/perf", cost=1)

    def _samples(self) -> list[float]:
        return _measure(self._check, _SAMPLES, _WARMUP)

    # ── percentile assertions ─────────────────────────────────────────────────

    def test_p50_within_slo(self):
        p = _report("TCP latency — p50 check", self._samples())
        assert p["p50"] <= self.p50_limit, (
            f"TCP p50 {p['p50']:.3f} ms exceeds SLO {self.p50_limit} ms"
        )

    def test_p95_within_slo(self):
        p = _report("TCP latency — p95 check", self._samples())
        assert p["p95"] <= self.p95_limit, (
            f"TCP p95 {p['p95']:.3f} ms exceeds SLO {self.p95_limit} ms"
        )

    def test_p99_within_slo(self):
        p = _report("TCP latency — p99 check", self._samples())
        assert p["p99"] <= self.p99_limit, (
            f"TCP p99 {p['p99']:.3f} ms exceeds SLO {self.p99_limit} ms"
        )

    def test_no_outlier_beyond_10x_p99(self):
        samples = self._samples()
        p = _report("TCP latency — outlier check", samples)
        worst = max(samples)
        ceiling = self.p99_limit * 10
        assert worst <= ceiling, (
            f"TCP worst-case {worst:.3f} ms > 10× p99 SLO ({ceiling:.1f} ms) — "
            "possible lock convoy or scheduler jitter"
        )

    # ── percentage assertions ─────────────────────────────────────────────────

    def test_pct_within_p95_slo(self):
        """At least 95 % of requests must complete within the p95 threshold."""
        samples = self._samples()
        _report("TCP latency — % within p95 SLO", samples)
        pct = _pct_within(samples, self.p95_limit)
        assert pct >= 95.0, (
            f"Only {pct:.2f}% of TCP requests completed within "
            f"{self.p95_limit} ms (need ≥ 95.00%)"
        )

    def test_pct_within_p99_slo(self):
        """At least 99 % of requests must complete within the p99 threshold."""
        samples = self._samples()
        _report("TCP latency — % within p99 SLO", samples)
        pct = _pct_within(samples, self.p99_limit)
        assert pct >= 99.0, (
            f"Only {pct:.2f}% of TCP requests completed within "
            f"{self.p99_limit} ms (need ≥ 99.00%)"
        )


# ── HTTP percentile + percentage tests ────────────────────────────────────────

class TestHttpLatencyPercentiles:
    """Assert round-trip latency SLOs for the axum HTTP path.

    Observed baseline (locust, 50 users, local loopback):
      p50 = 1 ms  |  p95 = 2 ms  |  p99 = 3 ms  |  max = 25 ms
    """

    p50_limit = _env_float("PERF_HTTP_P50_MS",  5.0)
    p95_limit = _env_float("PERF_HTTP_P95_MS", 15.0)
    p99_limit = _env_float("PERF_HTTP_P99_MS", 30.0)

    @pytest.fixture(autouse=True)
    def _require_http(self):
        import json as _json
        import urllib.request as _req
        try:
            probe = _req.Request(
                _HTTP_URL + "/check",
                data=b'{"client_id":"probe","resource":"/","cost":0}',
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            _req.urlopen(probe, timeout=2)
        except Exception:
            pytest.skip(f"HTTP server not reachable at {_HTTP_URL}")

        import itertools as _itertools
        _counter = _itertools.count()

        def _check():
            # Rotate client_id so every 10 requests get a fresh bucket;
            # this exercises both allowed (200) and denied (429) latencies.
            idx = next(_counter) % 50
            body = _json.dumps({
                "client_id": f"perf-http-{idx}",
                "resource": "/api/perf",
                "cost": 1,
            }).encode()
            req = _req.Request(
                _HTTP_URL + "/check",
                data=body,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            try:
                with _req.urlopen(req, timeout=5):
                    pass
            except _req.HTTPError as exc:
                if exc.code != 429:
                    raise

        self._check = _check

    def _samples(self) -> list[float]:
        return _measure(self._check, _SAMPLES, _WARMUP)

    # ── percentile assertions ─────────────────────────────────────────────────

    def test_p50_within_slo(self):
        p = _report("HTTP latency — p50 check", self._samples())
        assert p["p50"] <= self.p50_limit, (
            f"HTTP p50 {p['p50']:.3f} ms exceeds SLO {self.p50_limit} ms"
        )

    def test_p95_within_slo(self):
        p = _report("HTTP latency — p95 check", self._samples())
        assert p["p95"] <= self.p95_limit, (
            f"HTTP p95 {p['p95']:.3f} ms exceeds SLO {self.p95_limit} ms"
        )

    def test_p99_within_slo(self):
        p = _report("HTTP latency — p99 check", self._samples())
        assert p["p99"] <= self.p99_limit, (
            f"HTTP p99 {p['p99']:.3f} ms exceeds SLO {self.p99_limit} ms"
        )

    def test_no_outlier_beyond_10x_p99(self):
        samples = self._samples()
        p = _report("HTTP latency — outlier check", samples)
        worst = max(samples)
        ceiling = self.p99_limit * 10
        assert worst <= ceiling, (
            f"HTTP worst-case {worst:.3f} ms > 10× p99 SLO ({ceiling:.1f} ms)"
        )

    # ── percentage assertions ─────────────────────────────────────────────────

    def test_pct_within_p95_slo(self):
        """At least 95 % of requests must complete within the p95 threshold."""
        samples = self._samples()
        _report("HTTP latency — % within p95 SLO", samples)
        pct = _pct_within(samples, self.p95_limit)
        assert pct >= 95.0, (
            f"Only {pct:.2f}% of HTTP requests completed within "
            f"{self.p95_limit} ms (need ≥ 95.00%)"
        )

    def test_pct_within_p99_slo(self):
        """At least 99 % of requests must complete within the p99 threshold."""
        samples = self._samples()
        _report("HTTP latency — % within p99 SLO", samples)
        pct = _pct_within(samples, self.p99_limit)
        assert pct >= 99.0, (
            f"Only {pct:.2f}% of HTTP requests completed within "
            f"{self.p99_limit} ms (need ≥ 99.00%)"
        )
