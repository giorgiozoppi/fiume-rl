"""
Locust e2e stress tests for the rate-limiting service.

Targets both transports:
  HTTP → POST http://<host>:8080/check   (axum HTTP server)
  TCP  → raw FlatBuffer framing on :9000 (binary server)

Run locally (single-process, headless):
    locust -f tests/e2e/locustfile.py --headless -u 50 -r 10 --run-time 30s \
           --host http://127.0.0.1:8080

Run with the web UI:
    locust -f tests/e2e/locustfile.py --host http://127.0.0.1:8080
    # open http://localhost:8089

Run distributed (controller + workers):
    locust -f tests/e2e/locustfile.py --master --host http://127.0.0.1:8080
    locust -f tests/e2e/locustfile.py --worker --master-host <controller-ip>

Environment variables:
    RATE_LIMITER_HTTP_HOST   HTTP host:port  (default 127.0.0.1:8080)
    RATE_LIMITER_TCP_HOST    TCP  host:port  (default 127.0.0.1:9000)
    RATE_LIMITER_CLIENT_ID   client-id prefix (default locust)
"""

from __future__ import annotations

import os
import random
import time
from typing import Optional

from locust import HttpUser, TaskSet, between, events, task

from client import RateLimitClient
from client.models import RateLimitResponse

# ── environment ────────────────────────────────────────────────────────────────
_HTTP_HOST    = os.environ.get("RATE_LIMITER_HTTP_HOST", "127.0.0.1:8080")
_TCP_ADDR     = os.environ.get("RATE_LIMITER_TCP_HOST",  "127.0.0.1:9000")
_TCP_HOST, _TCP_PORT = _TCP_ADDR.rsplit(":", 1)
_TCP_PORT     = int(_TCP_PORT)
_CLIENT_PFX   = os.environ.get("RATE_LIMITER_CLIENT_ID", "locust")

_RESOURCES    = ["/api/orders", "/api/users", "/api/products", "/api/search"]


# ── HTTP tasks ─────────────────────────────────────────────────────────────────

class HttpTaskSet(TaskSet):
    @task(5)
    def check_single(self):
        payload = {
            "client_id": f"{_CLIENT_PFX}-{id(self.user)}",
            "resource":  random.choice(_RESOURCES),
            "cost":      1,
        }
        with self.client.post("/check", json=payload, catch_response=True) as resp:
            if resp.status_code in (200, 429):
                resp.success()
            else:
                resp.failure(f"unexpected status {resp.status_code}")

    @task(2)
    def check_high_cost(self):
        payload = {
            "client_id": f"{_CLIENT_PFX}-{id(self.user)}",
            "resource":  random.choice(_RESOURCES),
            "cost":      5,
        }
        with self.client.post("/check", json=payload, catch_response=True) as resp:
            if resp.status_code in (200, 429):
                resp.success()
            else:
                resp.failure(f"unexpected status {resp.status_code}")

    @task(1)
    def check_shared_client(self):
        """Many users sharing one client_id — stresses distributed CAS."""
        payload = {"client_id": f"{_CLIENT_PFX}-shared", "resource": "/api/shared", "cost": 1}
        with self.client.post("/check", json=payload, catch_response=True) as resp:
            if resp.status_code not in (200, 429):
                resp.failure(f"unexpected status {resp.status_code}")
                return
            data = resp.json()
            if "allowed" not in data or "remaining" not in data:
                resp.failure("malformed response body")
            else:
                resp.success()

    @task(1)
    def check_headers_present(self):
        payload = {"client_id": f"{_CLIENT_PFX}-hdr", "resource": "/api/hdr"}
        with self.client.post("/check", json=payload, catch_response=True) as resp:
            if resp.status_code not in (200, 429):
                resp.failure(f"unexpected status {resp.status_code}")
                return
            missing = [
                h for h in ("ratelimit-limit", "ratelimit-remaining", "ratelimit-reset", "ratelimit-policy")
                if h not in resp.headers
            ]
            if missing:
                resp.failure(f"missing headers: {missing}")
            else:
                resp.success()


class RateLimitHttpUser(HttpUser):
    tasks     = [HttpTaskSet]
    wait_time = between(0.05, 0.3)
    host      = f"http://{_HTTP_HOST}"


# ── TCP tasks ──────────────────────────────────────────────────────────────────

class RateLimitTcpUser(HttpUser):
    """Raw FlatBuffer TCP user.  Inherits HttpUser for Locust lifecycle only."""

    wait_time = between(0.05, 0.3)
    host      = f"http://{_HTTP_HOST}"   # required by Locust; unused for TCP

    _tcp: Optional[RateLimitClient] = None

    def on_start(self) -> None:
        self._tcp = RateLimitClient(_TCP_HOST, _TCP_PORT)

    def on_stop(self) -> None:
        if self._tcp:
            self._tcp.close()

    def _fire(self, name: str, fn) -> None:
        start = time.perf_counter()
        exc   = None
        try:
            fn()
        except Exception as e:
            exc = e
            try:
                self._tcp.close()
            except Exception:
                pass
            self._tcp = RateLimitClient(_TCP_HOST, _TCP_PORT)
        finally:
            self.environment.events.request.fire(
                request_type="TCP",
                name=name,
                response_time=(time.perf_counter() - start) * 1_000,
                response_length=0,
                exception=exc,
                context={},
            )

    @task(5)
    def tcp_check_single(self) -> None:
        client_id = f"{_CLIENT_PFX}-tcp-{id(self)}"
        resource  = random.choice(_RESOURCES)
        self._fire("check:single", lambda: self._tcp.check(client_id, resource, cost=1))

    @task(2)
    def tcp_check_shared(self) -> None:
        self._fire("check:shared", lambda: self._tcp.check(f"{_CLIENT_PFX}-shared", "/api/shared", cost=1))


# ── SLO percentile gate ────────────────────────────────────────────────────────
#
# Overridable via environment variables (milliseconds):
#   PERF_SLO_P95_MS   (default 15)  — p95 across all request types
#   PERF_SLO_P99_MS   (default 50)  — p99 across all request types
#
# If either threshold is breached the Locust run exits with a non-zero code,
# which fails CI pipelines.

_SLO_P95_MS = float(os.environ.get("PERF_SLO_P95_MS", 15))
_SLO_P99_MS = float(os.environ.get("PERF_SLO_P99_MS", 50))


@events.quitting.add_listener
def _check_slo_percentiles(environment, **_kwargs) -> None:
    """Fail the run if measured p95/p99 breach the SLO."""
    stats = environment.runner.stats.total
    # Locust stores percentile latencies in response_times (a Counter of ms values).
    p95 = stats.get_response_time_percentile(0.95)
    p99 = stats.get_response_time_percentile(0.99)

    failures: list[str] = []
    if p95 is not None and p95 > _SLO_P95_MS:
        failures.append(f"p95 {p95:.1f} ms > SLO {_SLO_P95_MS} ms")
    if p99 is not None and p99 > _SLO_P99_MS:
        failures.append(f"p99 {p99:.1f} ms > SLO {_SLO_P99_MS} ms")

    if failures:
        print(f"\n[SLO BREACH] Latency percentiles exceeded:\n" + "\n".join(f"  • {f}" for f in failures))
        environment.process_exit_code = 1
    else:
        print(
            f"\n[SLO OK] p95={p95:.1f} ms (≤{_SLO_P95_MS}), "
            f"p99={p99:.1f} ms (≤{_SLO_P99_MS})"
        )
