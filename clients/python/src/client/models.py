"""Pydantic models shared across the client, middleware, and app."""

from pydantic import BaseModel, Field


class RateLimitResponse(BaseModel):
    """Decision returned by the rate-limit service for a single check."""

    allowed: bool
    remaining: int = Field(ge=0, description="Capacity units left after this request")
    retry_after_ms: int = Field(ge=0, description="Milliseconds to wait before retrying (0 when allowed)")
    reason: str = Field(default="", description="Human-readable explanation")

    @property
    def retry_after_secs(self) -> int:
        """Seconds to wait — convenience for the Retry-After HTTP header."""
        return max(1, self.retry_after_ms // 1000) if self.retry_after_ms > 0 else 0


class CheckRequest(BaseModel):
    """Rate-limit check request sent to the HTTP or TCP endpoint."""

    client_id: str = Field(min_length=1, description="Opaque caller identifier")
    resource: str = Field(default="/", description="Resource path being accessed")
    cost: int = Field(default=1, ge=1, description="Capacity units to consume")


class CheckResponse(BaseModel):
    """JSON body returned by the HTTP /check endpoint."""

    allowed: bool
    remaining: int
    retry_after_ms: int
    reason: str

    @classmethod
    def from_decision(cls, decision: RateLimitResponse) -> "CheckResponse":
        return cls(
            allowed=decision.allowed,
            remaining=decision.remaining,
            retry_after_ms=decision.retry_after_ms,
            reason=decision.reason,
        )
