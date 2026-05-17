"""Rate-limit middleware for FastAPI / Starlette.

Usage:
    app.add_middleware(
        RateLimitMiddleware,
        limiter=RateLimitClient("127.0.0.1", 9000),
        resource_fn=lambda req: req.url.path,
        client_id_fn=lambda req: req.headers.get("X-Client-ID", req.client.host),
    )
"""

from __future__ import annotations

from typing import Callable, Optional

from fastapi import Request, Response
from fastapi.responses import JSONResponse
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.types import ASGIApp

from .client import RateLimitClient
from .models import RateLimitResponse


class RateLimitMiddleware(BaseHTTPMiddleware):
    """Enforces rate limits via the TCP service before each request reaches a handler."""

    def __init__(
        self,
        app: ASGIApp,
        limiter: RateLimitClient,
        *,
        resource_fn: Optional[Callable[[Request], str]] = None,
        client_id_fn: Optional[Callable[[Request], str]] = None,
        cost: int = 1,
    ) -> None:
        super().__init__(app)
        self._limiter = limiter
        self._resource_fn = resource_fn or (lambda req: req.url.path)
        self._client_id_fn = client_id_fn or _default_client_id
        self._cost = cost

    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        client_id = self._client_id_fn(request)
        resource  = self._resource_fn(request)

        decision: RateLimitResponse = self._limiter.check(client_id, resource, self._cost)

        if not decision.allowed:
            return JSONResponse(
                status_code=429,
                content={"detail": decision.reason, "retry_after_ms": decision.retry_after_ms},
                headers={
                    "Retry-After": str(decision.retry_after_secs),
                    "RateLimit-Remaining": str(decision.remaining),
                },
            )

        response: Response = await call_next(request)
        response.headers["RateLimit-Remaining"] = str(decision.remaining)
        return response


def _default_client_id(request: Request) -> str:
    forwarded = request.headers.get("X-Forwarded-For")
    if forwarded:
        return forwarded.split(",")[0].strip()
    return request.client.host if request.client else "unknown"
