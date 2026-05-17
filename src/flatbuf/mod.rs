//! Hand-written FlatBuffers bindings for the two tables defined in
//! `schema/messages.fbs`.  These are equivalent to what `flatc --rust` would
//! generate, so you can replace this file with flatc output at any time:
//!
//! ```bash
//! flatc --rust -o src/flatbuf schema/messages.fbs
//! ```
//!
//! Wire protocol used by the server:
//! ```text
//! ┌──────────────┬────────────────────────────┐
//! │  len : u32LE │  flatbuffer payload : [u8]  │
//! └──────────────┴────────────────────────────┘
//! ```

#![allow(
    unused_imports,
    dead_code,
    clippy::redundant_field_names,
    clippy::needless_lifetimes
)]

use flatbuffers::{self, EndianScalar, Follow, Push};

// ─────────────────────────────────────────────────────────────────────────────
// RateLimitRequest
// ─────────────────────────────────────────────────────────────────────────────

pub enum RateLimitRequestOffset {}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RateLimitRequest<'a> {
    pub _tab: flatbuffers::Table<'a>,
}

impl<'a> flatbuffers::Follow<'a> for RateLimitRequest<'a> {
    type Inner = RateLimitRequest<'a>;
    #[inline]
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Self {
            _tab: flatbuffers::Table::new(buf, loc),
        }
    }
}

impl<'a> RateLimitRequest<'a> {
    // VTable slot offsets (field index × 2 + 4).
    pub const VT_CLIENT_ID: flatbuffers::VOffsetT = 4;
    pub const VT_RESOURCE: flatbuffers::VOffsetT = 6;
    pub const VT_COST: flatbuffers::VOffsetT = 8;

    #[inline]
    pub unsafe fn init_from_table(table: flatbuffers::Table<'a>) -> Self {
        RateLimitRequest { _tab: table }
    }

    /// Build a `RateLimitRequest` into `_fbb`.
    #[allow(unused_mut)]
    pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        _fbb: &'mut_bldr mut flatbuffers::FlatBufferBuilder<'bldr>,
        args: &'args RateLimitRequestArgs<'args>,
    ) -> flatbuffers::WIPOffset<RateLimitRequest<'bldr>> {
        let mut builder = RateLimitRequestBuilder::new(_fbb);
        builder.add_cost(args.cost);
        if let Some(x) = args.resource {
            builder.add_resource(x);
        }
        if let Some(x) = args.client_id {
            builder.add_client_id(x);
        }
        builder.finish()
    }

    #[inline]
    pub fn client_id(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<flatbuffers::ForwardsUOffset<&str>>(Self::VT_CLIENT_ID, None)
        }
    }

    #[inline]
    pub fn resource(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<flatbuffers::ForwardsUOffset<&str>>(Self::VT_RESOURCE, None)
        }
    }

    #[inline]
    pub fn cost(&self) -> u32 {
        unsafe { self._tab.get::<u32>(Self::VT_COST, Some(1)).unwrap_or(1) }
    }
}

impl flatbuffers::Verifiable for RateLimitRequest<'_> {
    #[inline]
    fn run_verifier(
        v: &mut flatbuffers::Verifier,
        pos: usize,
    ) -> Result<(), flatbuffers::InvalidFlatbuffer> {
        use flatbuffers::Verifiable;
        v.visit_table(pos)?
            .visit_field::<flatbuffers::ForwardsUOffset<&str>>(
                "client_id",
                Self::VT_CLIENT_ID,
                true,
            )?
            .visit_field::<flatbuffers::ForwardsUOffset<&str>>(
                "resource",
                Self::VT_RESOURCE,
                false,
            )?
            .visit_field::<u32>("cost", Self::VT_COST, false)?
            .finish();
        Ok(())
    }
}

pub struct RateLimitRequestArgs<'a> {
    pub client_id: Option<flatbuffers::WIPOffset<&'a str>>,
    pub resource: Option<flatbuffers::WIPOffset<&'a str>>,
    pub cost: u32,
}

impl<'a> Default for RateLimitRequestArgs<'a> {
    fn default() -> Self {
        RateLimitRequestArgs {
            client_id: None,
            resource: None,
            cost: 1,
        }
    }
}

pub struct RateLimitRequestBuilder<'a: 'b, 'b> {
    fbb_: &'b mut flatbuffers::FlatBufferBuilder<'a>,
    start_: flatbuffers::WIPOffset<flatbuffers::TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> RateLimitRequestBuilder<'a, 'b> {
    #[inline]
    pub fn add_client_id(&mut self, client_id: flatbuffers::WIPOffset<&'b str>) {
        self.fbb_.push_slot_always::<flatbuffers::WIPOffset<_>>(
            RateLimitRequest::VT_CLIENT_ID,
            client_id,
        );
    }
    #[inline]
    pub fn add_resource(&mut self, resource: flatbuffers::WIPOffset<&'b str>) {
        self.fbb_.push_slot_always::<flatbuffers::WIPOffset<_>>(
            RateLimitRequest::VT_RESOURCE,
            resource,
        );
    }
    #[inline]
    pub fn add_cost(&mut self, cost: u32) {
        self.fbb_
            .push_slot::<u32>(RateLimitRequest::VT_COST, cost, 1);
    }
    #[inline]
    pub fn new(fbb: &'b mut flatbuffers::FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        RateLimitRequestBuilder {
            fbb_: fbb,
            start_: start,
        }
    }
    #[inline]
    pub fn finish(self) -> flatbuffers::WIPOffset<RateLimitRequest<'a>> {
        let o = self.fbb_.end_table(self.start_);
        flatbuffers::WIPOffset::new(o.value())
    }
}

/// Parse the root `RateLimitRequest` from a finished FlatBuffer byte slice.
pub fn root_as_rate_limit_request(
    buf: &[u8],
) -> Result<RateLimitRequest<'_>, flatbuffers::InvalidFlatbuffer> {
    flatbuffers::root::<RateLimitRequest>(buf)
}

// ─────────────────────────────────────────────────────────────────────────────
// RateLimitResponse
// ─────────────────────────────────────────────────────────────────────────────

pub enum RateLimitResponseOffset {}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RateLimitResponse<'a> {
    pub _tab: flatbuffers::Table<'a>,
}

impl<'a> flatbuffers::Follow<'a> for RateLimitResponse<'a> {
    type Inner = RateLimitResponse<'a>;
    #[inline]
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        Self {
            _tab: flatbuffers::Table::new(buf, loc),
        }
    }
}

impl<'a> RateLimitResponse<'a> {
    pub const VT_ALLOWED: flatbuffers::VOffsetT = 4;
    pub const VT_REMAINING: flatbuffers::VOffsetT = 6;
    pub const VT_RETRY_AFTER_MS: flatbuffers::VOffsetT = 8;
    pub const VT_REASON: flatbuffers::VOffsetT = 10;

    #[inline]
    pub unsafe fn init_from_table(table: flatbuffers::Table<'a>) -> Self {
        RateLimitResponse { _tab: table }
    }

    #[allow(unused_mut)]
    pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        _fbb: &'mut_bldr mut flatbuffers::FlatBufferBuilder<'bldr>,
        args: &'args RateLimitResponseArgs<'args>,
    ) -> flatbuffers::WIPOffset<RateLimitResponse<'bldr>> {
        let mut builder = RateLimitResponseBuilder::new(_fbb);
        if let Some(x) = args.reason {
            builder.add_reason(x);
        }
        builder.add_retry_after_ms(args.retry_after_ms);
        builder.add_remaining(args.remaining);
        builder.add_allowed(args.allowed);
        builder.finish()
    }

    #[inline]
    pub fn allowed(&self) -> bool {
        unsafe {
            self._tab
                .get::<bool>(Self::VT_ALLOWED, Some(false))
                .unwrap_or(false)
        }
    }
    #[inline]
    pub fn remaining(&self) -> i64 {
        unsafe {
            self._tab
                .get::<i64>(Self::VT_REMAINING, Some(0))
                .unwrap_or(0)
        }
    }
    #[inline]
    pub fn retry_after_ms(&self) -> i64 {
        unsafe {
            self._tab
                .get::<i64>(Self::VT_RETRY_AFTER_MS, Some(0))
                .unwrap_or(0)
        }
    }
    #[inline]
    pub fn reason(&self) -> Option<&'a str> {
        unsafe {
            self._tab
                .get::<flatbuffers::ForwardsUOffset<&str>>(Self::VT_REASON, None)
        }
    }
}

impl flatbuffers::Verifiable for RateLimitResponse<'_> {
    #[inline]
    fn run_verifier(
        v: &mut flatbuffers::Verifier,
        pos: usize,
    ) -> Result<(), flatbuffers::InvalidFlatbuffer> {
        use flatbuffers::Verifiable;
        v.visit_table(pos)?
            .visit_field::<bool>("allowed", Self::VT_ALLOWED, false)?
            .visit_field::<i64>("remaining", Self::VT_REMAINING, false)?
            .visit_field::<i64>("retry_after_ms", Self::VT_RETRY_AFTER_MS, false)?
            .visit_field::<flatbuffers::ForwardsUOffset<&str>>("reason", Self::VT_REASON, false)?
            .finish();
        Ok(())
    }
}

pub struct RateLimitResponseArgs<'a> {
    pub allowed: bool,
    pub remaining: i64,
    pub retry_after_ms: i64,
    pub reason: Option<flatbuffers::WIPOffset<&'a str>>,
}

impl<'a> Default for RateLimitResponseArgs<'a> {
    fn default() -> Self {
        RateLimitResponseArgs {
            allowed: false,
            remaining: 0,
            retry_after_ms: 0,
            reason: None,
        }
    }
}

pub struct RateLimitResponseBuilder<'a: 'b, 'b> {
    fbb_: &'b mut flatbuffers::FlatBufferBuilder<'a>,
    start_: flatbuffers::WIPOffset<flatbuffers::TableUnfinishedWIPOffset>,
}

impl<'a: 'b, 'b> RateLimitResponseBuilder<'a, 'b> {
    #[inline]
    pub fn add_allowed(&mut self, allowed: bool) {
        self.fbb_
            .push_slot::<bool>(RateLimitResponse::VT_ALLOWED, allowed, false);
    }
    #[inline]
    pub fn add_remaining(&mut self, remaining: i64) {
        self.fbb_
            .push_slot::<i64>(RateLimitResponse::VT_REMAINING, remaining, 0);
    }
    #[inline]
    pub fn add_retry_after_ms(&mut self, retry_after_ms: i64) {
        self.fbb_
            .push_slot::<i64>(RateLimitResponse::VT_RETRY_AFTER_MS, retry_after_ms, 0);
    }
    #[inline]
    pub fn add_reason(&mut self, reason: flatbuffers::WIPOffset<&'b str>) {
        self.fbb_.push_slot_always::<flatbuffers::WIPOffset<_>>(
            RateLimitResponse::VT_REASON,
            reason,
        );
    }
    #[inline]
    pub fn new(fbb: &'b mut flatbuffers::FlatBufferBuilder<'a>) -> Self {
        let start = fbb.start_table();
        RateLimitResponseBuilder {
            fbb_: fbb,
            start_: start,
        }
    }
    #[inline]
    pub fn finish(self) -> flatbuffers::WIPOffset<RateLimitResponse<'a>> {
        let o = self.fbb_.end_table(self.start_);
        flatbuffers::WIPOffset::new(o.value())
    }
}

/// Parse the root `RateLimitResponse` from a finished FlatBuffer byte slice.
pub fn root_as_rate_limit_response(
    buf: &[u8],
) -> Result<RateLimitResponse<'_>, flatbuffers::InvalidFlatbuffer> {
    flatbuffers::root::<RateLimitResponse>(buf)
}
