"""
FastAPI demo — rate limiter as middleware.

The rate-limiter is injected as Starlette middleware: every inbound request is
checked against the TCP service before it reaches a route handler.
Denied requests get a 429 with Retry-After; allowed requests carry
X-RateLimit-Remaining on the response.

Start the rate-limiter server first:
    cargo run --bin server          # TCP :9000  HTTP :8080

Run this demo:
    uvicorn app.main:app --reload   # listens on http://127.0.0.1:8000

Try it:
    curl -i http://127.0.0.1:8000/items
    curl -i -H "X-Client-ID: bob" http://127.0.0.1:8000/items
    for i in $(seq 1 15); do curl -s -o /dev/null -w "%{http_code}\n" \
        http://127.0.0.1:8000/items; done
"""

import os
from typing import Any

from fastapi import FastAPI
from pydantic import BaseModel

from client import RateLimitClient, RateLimitMiddleware

# ── configuration ─────────────────────────────────────────────────────────────
_RL_HOST = os.environ.get("RATE_LIMITER_HOST", "127.0.0.1")
_RL_PORT = int(os.environ.get("RATE_LIMITER_PORT", "9000"))

# ── app + middleware ───────────────────────────────────────────────────────────
app = FastAPI(title="Rate-limited demo API")

_limiter = RateLimitClient(_RL_HOST, _RL_PORT)

app.add_middleware(
    RateLimitMiddleware,
    limiter=_limiter,
    client_id_fn=lambda req: (
        req.headers.get("X-Client-ID")
        or (req.client.host if req.client else "unknown")
    ),
    resource_fn=lambda req: req.url.path,
    cost=1,
)

# ── response models ───────────────────────────────────────────────────────────

class ItemList(BaseModel):
    items: list[str]

class Item(BaseModel):
    item_id: int
    name: str

class OrderRequest(BaseModel):
    product_id: int
    quantity: int = 1

class OrderResponse(BaseModel):
    order_id: int
    status: str
    payload: Any

class HealthResponse(BaseModel):
    status: str

# ── routes ────────────────────────────────────────────────────────────────────

@app.get("/")
async def root() -> dict[str, str]:
    return {"message": "rate-limited API — try GET /items or POST /orders"}


@app.get("/items", response_model=ItemList)
async def list_items() -> ItemList:
    return ItemList(items=["widget", "gadget", "doohickey"])


@app.get("/items/{item_id}", response_model=Item)
async def get_item(item_id: int) -> Item:
    return Item(item_id=item_id, name=f"item-{item_id}")


@app.post("/orders", response_model=OrderResponse)
async def create_order(body: OrderRequest) -> OrderResponse:
    return OrderResponse(order_id=42, status="accepted", payload=body.model_dump())


@app.get("/health", response_model=HealthResponse)
async def health() -> HealthResponse:
    return HealthResponse(status="ok")
