//! Tokio TCP server (length-prefixed FlatBuffers protocol).
//!
//! Wire protocol:
//! ```text
//! ┌──────────────────────┬──────────────────────────────────────┐
//! │  len : u32 (LE)      │  FlatBuffer payload  (len bytes)     │
//! └──────────────────────┴──────────────────────────────────────┘
//! ```

use std::sync::Arc;

use anyhow::Result;
use flatbuffers::FlatBufferBuilder;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{debug, error, info, warn};

use crate::{
    flatbuf::{root_as_rate_limit_request, RateLimitResponseBuilder},
    limiter::{Decision, SharedLimiter},
};

const MAX_MSG_BYTES: usize = 1 << 20;

pub struct Server {
    listener: TcpListener,
    limiter: SharedLimiter,
}

impl Server {
    pub async fn bind(addr: &str, limiter: SharedLimiter) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        info!(addr, algorithm = limiter.name(), "TCP server listening");
        Ok(Self { listener, limiter })
    }

    pub async fn run(self) -> Result<()> {
        let limiter = Arc::clone(&self.limiter);
        loop {
            match self.listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let _ = stream.set_nodelay(true);
                    debug!(%peer_addr, "accepted connection");
                    let limiter = Arc::clone(&limiter);
                    tokio::spawn(async move {
                        if let Err(e) = handle_conn(stream, limiter).await {
                            warn!(%peer_addr, err = %e, "connection closed with error");
                        }
                    });
                }
                Err(e) => error!(err = %e, "accept failed"),
            }
        }
    }
}

async fn handle_conn(mut stream: TcpStream, limiter: SharedLimiter) -> Result<()> {
    loop {
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }

        let msg_len = u32::from_le_bytes(len_buf) as usize;
        if msg_len == 0 || msg_len > MAX_MSG_BYTES {
            warn!(msg_len, "rejecting message with invalid length");
            return Ok(());
        }

        let mut body = vec![0u8; msg_len];
        stream.read_exact(&mut body).await?;

        let resp_bytes = match root_as_rate_limit_request(&body) {
            Ok(req) => {
                let client_id = req.client_id().unwrap_or("unknown");
                let resource = req.resource().unwrap_or("/");
                let cost = req.cost();
                debug!(client_id, resource, cost, "rate-limit check");
                let decision = limiter.check(client_id, resource, cost).await;
                encode_response(decision)
            }
            Err(e) => {
                warn!(err = %e, "FlatBuffer parse error — closing connection");
                return Ok(());
            }
        };

        let resp_len = resp_bytes.len() as u32;
        stream.write_all(&resp_len.to_le_bytes()).await?;
        stream.write_all(&resp_bytes).await?;
    }
}

fn encode_response(d: Decision) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::with_capacity(256);
    let reason = fbb.create_string(&d.reason);

    let mut rb = RateLimitResponseBuilder::new(&mut fbb);
    rb.add_allowed(d.allowed);
    rb.add_remaining(d.remaining);
    rb.add_retry_after_ms(d.retry_after_ms);
    rb.add_reason(reason);
    let root = rb.finish();

    fbb.finish(root, None);
    fbb.finished_data().to_vec()
}
