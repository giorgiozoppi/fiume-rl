//! Test client — fires N requests against the server and prints each decision.
//!
//! Usage:
//! ```bash
//! cargo run --bin client [host:port] [client_id] [num_requests] [delay_ms]
//!
//! # defaults:  127.0.0.1:9000   client_1   20   100
//! ```

use anyhow::Result;
use flatbuffers::FlatBufferBuilder;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::Duration,
};

use rate_limiting::flatbuf::{
    root_as_rate_limit_response, RateLimitRequestBuilder,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:9000".into());
    let client_id = args.get(2).cloned().unwrap_or_else(|| "client_1".into());
    let num_requests: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);
    let delay_ms: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(100);

    println!("→ connecting to {addr}  client_id={client_id}  requests={num_requests}  delay={delay_ms}ms\n");

    let mut stream = TcpStream::connect(&addr).await?;

    println!(
        "{:>3}  {:<8}  {:>10}  {:>15}  reason",
        "#", "allowed", "remaining", "retry_after_ms"
    );
    println!("{}", "─".repeat(72));

    for i in 1..=num_requests {
        // ── build request FlatBuffer ──────────────────────────────────────
        let mut fbb = FlatBufferBuilder::with_capacity(256);
        let cid = fbb.create_string(&client_id);
        let res = fbb.create_string("/api/v1/data");

        let mut rb = RateLimitRequestBuilder::new(&mut fbb);
        rb.add_client_id(cid);
        rb.add_resource(res);
        rb.add_cost(1);
        let req = rb.finish();
        fbb.finish(req, None);

        let data = fbb.finished_data();
        let len = data.len() as u32;

        stream.write_all(&len.to_le_bytes()).await?;
        stream.write_all(data).await?;

        // ── read response ─────────────────────────────────────────────────
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut resp_body = vec![0u8; resp_len];
        stream.read_exact(&mut resp_body).await?;

        let resp = root_as_rate_limit_response(&resp_body)?;

        let allowed_str = if resp.allowed() { "✓ YES" } else { "✗ NO " };
        println!(
            "{i:>3}  {allowed_str}     {remaining:>10}  {retry:>15}  {reason}",
            allowed_str = allowed_str,
            remaining = resp.remaining(),
            retry = resp.retry_after_ms(),
            reason = resp.reason().unwrap_or("")
        );

        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    Ok(())
}
