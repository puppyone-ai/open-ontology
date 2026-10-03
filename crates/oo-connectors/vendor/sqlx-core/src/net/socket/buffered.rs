use crate::error::Error;
use crate::net::Socket;
use bytes::BytesMut;
use std::mem::{replace, size_of};
use std::ops::ControlFlow;
use std::{cmp, io};

use crate::io::{AsyncRead, AsyncReadExt, ProtocolDecode, ProtocolEncode};

/// One socket read performed while filling a protocol frame.
///
/// Lengths are byte counts measured on this buffer. They are not configuration knobs
/// and they are not `malloc_usable_size` or RSS.
///
/// `view_capacity_before` and `view_capacity_after` are `read.capacity() + available.capacity()`.
/// That sum is the two live windows. It is not a physical resident total: the windows may
/// share one allocation, or they may not, and a retained frame is a separate view that is
/// not added in. An offset kept outside both windows is missing from the sum as well.
#[derive(Copy, Clone, Debug)]
pub struct IncomingAllocEvent {
    /// Absolute buffer length the parser asked to have present.
    pub logical_need: u64,
    /// Argument passed to `BytesMut::reserve`. Zero when that call was not made.
    ///
    /// The argument is additional bytes beyond the buffer's current `len`, which is
    /// what `BytesMut::reserve` requires. It is not the number of bytes of capacity
    /// that were added.
    pub reserve_argument: u64,
    /// View-capacity increase for this step: the sum of the two windows after the
    /// step, minus the sum before it. Zero when the call did not grow a window.
    pub reserved_growth: u64,
    /// `read` view capacity before the reserve.
    pub read_capacity_before: u64,
    /// `available` view capacity before the reserve.
    pub available_capacity_before: u64,
    /// `available.len()` before the reserve. Zero means that view is spare only.
    pub available_len_before: u64,
    /// `available` view capacity after the reserve and before the socket read.
    pub available_capacity_after: u64,
    /// Sum of the two view capacities before this step.
    pub view_capacity_before: u64,
    /// Sum of the two view capacities after this step. For a read that has not
    /// returned, this is the sum after the reserve only.
    pub view_capacity_after: u64,
    /// `read.len()` before this socket read.
    pub read_len_before: u64,
    /// Bytes returned by this socket read. Zero when the read has not returned.
    pub bytes_returned: u64,
    /// Bytes copied while joining the windows on this step. Zero when `unsplit` stayed
    /// a pointer adjustment. This is not the size of the allocation.
    pub assembly_bytes: u64,
    /// `1` after the socket read returns, including a zero-byte EOF. `0` when the
    /// future was dropped while the read was pending.
    pub read_completed: u64,
}

/// Lifetime high-water of incoming socket buffering.
///
/// Peaks are taken from `BytesMut` view capacity and from read counts. They are not
/// copies of a bound, not RSS, and not `malloc_usable_size`.
///
/// `view_capacity_peak` is the largest sum of the two live windows. Summing those
/// windows does not yield a physical resident total. `accepted_backing_peak` is that
/// same kind of sum sampled when a frame is accepted. Retained frames are not added
/// into either figure.
#[derive(Clone, Debug)]
pub struct IncomingHighWater {
    pub logical_requested_peak: u64,
    /// Sum of view-capacity increases. A `BytesMut::reserve` argument that does not
    /// change capacity is not included.
    pub reserved_additional: u64,
    /// Number of `BytesMut::reserve` calls, including calls whose argument was already
    /// covered by spare capacity.
    pub reserve_calls: u64,
    /// Calls included in `reserve_calls` whose capacity did not change.
    pub noop_reserve_calls: u64,
    /// Sum of the arguments of those no-op calls. Not capacity growth.
    pub noop_reserve_arguments: u64,
    /// Sum of every argument passed to `BytesMut::reserve`, including no-op calls.
    pub reserve_argument_bytes: u64,
    pub read_capacity_peak: u64,
    pub available_capacity_peak: u64,
    pub view_capacity_peak: u64,
    /// Largest spare, above the reserve argument, left by a call that did grow.
    /// bytes 1.12.1 can raise a new buffer to its original-capacity floor, and
    /// `Vec::reserve` can add more than requested. Zero means the measured spare
    /// matched the argument. This is not allocator slack below `BytesMut::capacity`.
    pub rounding_peak: u64,
    pub overread_bytes: u64,
    pub reads: u64,
    pub bytes_read: u64,
    pub messages: u64,
    pub accepted_logical_payload: u64,
    pub accepted_backing_peak: u64,
    /// Bytes copied while joining `read` and `available`. A pointer-only `unsplit`
    /// adds nothing. Relocation of a retained prefix and a non-contiguous append
    /// both add the number of bytes copied. Capacity already advanced past, in
    /// front of both windows, is not part of either view and is not added. In-place
    /// growth that keeps the pointer adds nothing.
    pub assembly_bytes: u64,
    /// Most recent parser decision length, including a rejection that did not read again.
    pub last_logical_need: u64,
    /// Diagnostic rows that were not stored because `events` is capped.
    pub events_dropped: u64,
    pub events: Vec<IncomingAllocEvent>,
}

impl Default for IncomingHighWater {
    fn default() -> Self {
        Self {
            logical_requested_peak: 0,
            reserved_additional: 0,
            reserve_calls: 0,
            noop_reserve_calls: 0,
            noop_reserve_arguments: 0,
            reserve_argument_bytes: 0,
            read_capacity_peak: 0,
            available_capacity_peak: 0,
            view_capacity_peak: 0,
            rounding_peak: 0,
            overread_bytes: 0,
            reads: 0,
            bytes_read: 0,
            messages: 0,
            accepted_logical_payload: 0,
            accepted_backing_peak: 0,
            assembly_bytes: 0,
            last_logical_need: 0,
            events_dropped: 0,
            events: Vec::new(),
        }
    }
}

impl IncomingHighWater {
    /// Capacity of the event log, not the length of the log and not the socket buffer.
    pub fn trace_bytes(&self) -> usize {
        self.events.capacity() * size_of::<IncomingAllocEvent>()
    }
}

// Tokio, async-std, and std all use this as the default capacity for their buffered I/O.
const DEFAULT_BUF_SIZE: usize = 8192;

/// The event log is a diagnostic ring of recent reads, not the socket buffer.
/// Lifetime counters keep moving after this cap is reached.
const MAX_INCOMING_ALLOC_EVENTS: usize = 64;

pub struct BufferedSocket<S> {
    socket: S,
    write_buf: WriteBuffer,
    read_buf: ReadBuffer,
}

pub struct WriteBuffer {
    buf: Vec<u8>,
    bytes_written: usize,
    bytes_flushed: usize,
}

pub struct ReadBuffer {
    read: BytesMut,
    available: BytesMut,
    meter: IncomingHighWater,
    last_sampled_view: u64,
}

impl<S: Socket> BufferedSocket<S> {
    pub fn new(socket: S) -> Self
    where
        S: Sized,
    {
        let mut socket = BufferedSocket {
            socket,
            write_buf: WriteBuffer {
                buf: Vec::with_capacity(DEFAULT_BUF_SIZE),
                bytes_written: 0,
                bytes_flushed: 0,
            },
            read_buf: ReadBuffer {
                read: BytesMut::new(),
                available: BytesMut::with_capacity(DEFAULT_BUF_SIZE),
                meter: IncomingHighWater::default(),
                last_sampled_view: 0,
            },
        };
        // `with_capacity` is not a `reserve` call. The initial spare is still resident.
        socket.read_buf.sample_peaks();
        socket
    }

    /// Measured incoming allocation, as plain integers.
    ///
    /// Order: logical peak, reserved growth, reserve calls, read capacity peak,
    /// available capacity peak, view capacity peak, rounding peak, overread, reads,
    /// bytes read, messages, accepted logical payload, accepted backing peak,
    /// assembly bytes, last logical need, trace bytes, no-op reserve calls,
    /// no-op reserve arguments, reserve argument bytes, events dropped.
    pub fn incoming_meter_parts(&self) -> [u64; 20] {
        let meter = &self.read_buf.meter;
        [
            meter.logical_requested_peak,
            meter.reserved_additional,
            meter.reserve_calls,
            meter.read_capacity_peak,
            meter.available_capacity_peak,
            meter.view_capacity_peak,
            meter.rounding_peak,
            meter.overread_bytes,
            meter.reads,
            meter.bytes_read,
            meter.messages,
            meter.accepted_logical_payload,
            meter.accepted_backing_peak,
            meter.assembly_bytes,
            meter.last_logical_need,
            meter.trace_bytes() as u64,
            meter.noop_reserve_calls,
            meter.noop_reserve_arguments,
            meter.reserve_argument_bytes,
            meter.events_dropped,
        ]
    }

    /// Socket-read events, in the field order of [`IncomingAllocEvent`].
    pub fn incoming_meter_events(&self) -> Vec<[u64; 13]> {
        self.read_buf
            .meter
            .events
            .iter()
            .map(|event| {
                [
                    event.logical_need,
                    event.reserve_argument,
                    event.reserved_growth,
                    event.read_capacity_before,
                    event.available_capacity_before,
                    event.available_len_before,
                    event.available_capacity_after,
                    event.view_capacity_before,
                    event.view_capacity_after,
                    event.read_len_before,
                    event.bytes_returned,
                    event.assembly_bytes,
                    event.read_completed,
                ]
            })
            .collect()
    }

    #[doc(hidden)]
    pub fn note_logical_need(&mut self, n: u64) {
        self.read_buf.note_logical(n);
    }

    #[doc(hidden)]
    pub fn note_message_accepted(&mut self, payload_len: usize) {
        self.read_buf.note_message_accepted(payload_len);
    }

    pub async fn read_buffered(&mut self, len: usize) -> Result<BytesMut, Error> {
        self.try_read(|buf| {
            Ok(if buf.len() < len {
                ControlFlow::Continue(len)
            } else {
                ControlFlow::Break(buf.split_to(len))
            })
        })
        .await
    }

    /// Retryable read operation.
    ///
    /// The callback should check the contents of the buffer passed to it and either:
    ///
    /// * Remove a full message from the buffer and return [`ControlFlow::Break`], or:
    /// * Return [`ControlFlow::Continue`] with the expected _total_ length of the buffer,
    ///   _without_ modifying it.
    ///
    /// Cancel-safe as long as the callback does not modify the passed `BytesMut`
    /// before returning [`ControlFlow::Continue`].
    pub async fn try_read<F, R>(&mut self, mut try_read: F) -> Result<R, Error>
    where
        F: FnMut(&mut BytesMut) -> Result<ControlFlow<R, usize>, Error>,
    {
        loop {
            // Sample before the callback consumes a complete frame.
            self.read_buf.sample_peaks();
            let read_len = match try_read(&mut self.read_buf.read)? {
                ControlFlow::Continue(read_len) => read_len,
                ControlFlow::Break(ret) => return Ok(ret),
            };

            self.read_buf.read(read_len, &mut self.socket).await?;
        }
    }

    pub fn write_buffer(&self) -> &WriteBuffer {
        &self.write_buf
    }

    pub fn write_buffer_mut(&mut self) -> &mut WriteBuffer {
        &mut self.write_buf
    }

    pub async fn read<'de, T>(&mut self, byte_len: usize) -> Result<T, Error>
    where
        T: ProtocolDecode<'de, ()>,
    {
        self.read_with(byte_len, ()).await
    }

    pub async fn read_with<'de, T, C>(&mut self, byte_len: usize, context: C) -> Result<T, Error>
    where
        T: ProtocolDecode<'de, C>,
    {
        T::decode_with(self.read_buffered(byte_len).await?.freeze(), context)
    }

    #[inline(always)]
    pub fn write<'en, T>(&mut self, value: T) -> Result<(), Error>
    where
        T: ProtocolEncode<'en, ()>,
    {
        self.write_with(value, ())
    }

    #[inline(always)]
    pub fn write_with<'en, T, C>(&mut self, value: T, context: C) -> Result<(), Error>
    where
        T: ProtocolEncode<'en, C>,
    {
        value.encode_with(self.write_buf.buf_mut(), context)?;
        self.write_buf.bytes_written = self.write_buf.buf.len();
        self.write_buf.sanity_check();

        Ok(())
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        while !self.write_buf.is_empty() {
            let written = self.socket.write(self.write_buf.get()).await?;
            self.write_buf.consume(written);
            self.write_buf.sanity_check();
        }

        self.socket.flush().await?;

        Ok(())
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.flush().await?;
        self.socket.shutdown().await
    }

    pub fn shrink_buffers(&mut self) {
        // Won't drop data still in the buffer.
        self.write_buf.shrink();
        self.read_buf.shrink();
    }

    pub fn into_inner(self) -> S {
        self.socket
    }

    pub fn boxed(self) -> BufferedSocket<Box<dyn Socket>> {
        BufferedSocket {
            socket: Box::new(self.socket),
            write_buf: self.write_buf,
            read_buf: self.read_buf,
        }
    }
}

impl WriteBuffer {
    fn sanity_check(&self) {
        assert_ne!(self.buf.capacity(), 0);
        assert!(self.bytes_written <= self.buf.len());
        assert!(self.bytes_flushed <= self.bytes_written);
    }

    pub fn buf_mut(&mut self) -> &mut Vec<u8> {
        self.buf.truncate(self.bytes_written);
        self.sanity_check();
        &mut self.buf
    }

    pub fn init_remaining_mut(&mut self) -> &mut [u8] {
        self.buf.resize(self.buf.capacity(), 0);
        self.sanity_check();
        &mut self.buf[self.bytes_written..]
    }

    pub fn put_slice(&mut self, slice: &[u8]) {
        // If we already have an initialized area that can fit the slice,
        // don't change `self.buf.len()`
        if let Some(dest) = self.buf[self.bytes_written..].get_mut(..slice.len()) {
            dest.copy_from_slice(slice);
        } else {
            self.buf.truncate(self.bytes_written);
            self.buf.extend_from_slice(slice);
        }
        self.advance(slice.len());
        self.sanity_check();
    }

    pub fn advance(&mut self, amt: usize) {
        let new_bytes_written = self
            .bytes_written
            .checked_add(amt)
            .expect("self.bytes_written + amt overflowed");

        assert!(new_bytes_written <= self.buf.len());

        self.bytes_written = new_bytes_written;

        self.sanity_check();
    }

    /// Read into the buffer from `source`, returning the number of bytes read.
    ///
    /// The buffer is automatically advanced by the number of bytes read.
    pub async fn read_from(&mut self, mut source: impl AsyncRead + Unpin) -> io::Result<usize> {
        let read = match () {
            // Tokio lets us read into the buffer without zeroing first
            #[cfg(feature = "_rt-tokio")]
            _ => source.read_buf(self.buf_mut()).await?,
            #[cfg(not(feature = "_rt-tokio"))]
            _ => source.read(self.init_remaining_mut()).await?,
        };

        if read > 0 {
            self.advance(read);
        }

        Ok(read)
    }

    pub fn is_empty(&self) -> bool {
        self.bytes_flushed >= self.bytes_written
    }

    pub fn is_full(&self) -> bool {
        self.bytes_written == self.buf.len()
    }

    pub fn get(&self) -> &[u8] {
        &self.buf[self.bytes_flushed..self.bytes_written]
    }

    pub fn get_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.bytes_flushed..self.bytes_written]
    }

    pub fn shrink(&mut self) {
        if self.bytes_flushed > 0 {
            // Move any data that remains to be flushed to the beginning of the buffer,
            // if necessary.
            self.buf
                .copy_within(self.bytes_flushed..self.bytes_written, 0);
            self.bytes_written -= self.bytes_flushed;
            self.bytes_flushed = 0
        }

        // Drop excess capacity.
        self.buf
            .truncate(cmp::max(self.bytes_written, DEFAULT_BUF_SIZE));
        self.buf.shrink_to_fit();
    }

    fn consume(&mut self, amt: usize) {
        let new_bytes_flushed = self
            .bytes_flushed
            .checked_add(amt)
            .expect("self.bytes_flushed + amt overflowed");

        assert!(new_bytes_flushed <= self.bytes_written);

        self.bytes_flushed = new_bytes_flushed;

        if self.bytes_flushed == self.bytes_written {
            // Reset cursors to zero if we've consumed the whole buffer
            self.bytes_flushed = 0;
            self.bytes_written = 0;
        }

        self.sanity_check();
    }
}

impl ReadBuffer {
    fn note_logical(&mut self, n: u64) {
        self.meter.last_logical_need = n;
        self.meter.logical_requested_peak = self.meter.logical_requested_peak.max(n);
    }

    fn note_message_accepted(&mut self, payload_len: usize) {
        self.meter.messages += 1;
        self.meter.accepted_logical_payload += payload_len as u64;
        self.meter.accepted_backing_peak = self
            .meter
            .accepted_backing_peak
            .max(self.last_sampled_view);
    }

    fn sample_peaks(&mut self) {
        let read_cap = self.read.capacity() as u64;
        let available_cap = self.available.capacity() as u64;
        let view = read_cap + available_cap;
        self.meter.read_capacity_peak = self.meter.read_capacity_peak.max(read_cap);
        self.meter.available_capacity_peak = self.meter.available_capacity_peak.max(available_cap);
        self.meter.view_capacity_peak = self.meter.view_capacity_peak.max(view);
        self.last_sampled_view = view;
    }

    fn view_capacity(&self) -> u64 {
        (self.read.capacity() + self.available.capacity()) as u64
    }

    async fn read(&mut self, len: usize, socket: &mut impl Socket) -> io::Result<()> {
        // Because of how `BytesMut` works, we should only be shifting capacity back and forth
        // between `read` and `available` unless we have to read an oversize message.
        self.note_logical(len as u64);
        while self.read.len() < len {
            let deficit = len - self.read.len();
            let read_len_before = self.read.len();
            let read_capacity_before = self.read.capacity();
            let available_len_before = self.available.len();
            let available_capacity_before = self.available.capacity();
            let view_before = self.view_capacity();
            let (reserve_argument, mut assembly) = self.prepare_window(deficit);
            let view_prepared = self.view_capacity();
            let growth = view_prepared
                .checked_sub(view_before)
                .expect("incoming view shrank during reserve");
            self.note_reserve(reserve_argument, growth);
            self.note_rounding(reserve_argument, growth);
            if assembly > 0 {
                self.meter.assembly_bytes += assembly;
            }
            let available_capacity_after = self.available.capacity();
            self.sample_peaks();
            // The row is stored before the await. Dropping the parked read keeps the
            // growth and the copy that already happened; it does not invent a socket read.
            let pushed = self.push_event(IncomingAllocEvent {
                logical_need: len as u64,
                reserve_argument: reserve_argument as u64,
                reserved_growth: growth,
                read_capacity_before: read_capacity_before as u64,
                available_capacity_before: available_capacity_before as u64,
                available_len_before: available_len_before as u64,
                available_capacity_after: available_capacity_after as u64,
                view_capacity_before: view_before,
                view_capacity_after: view_prepared,
                read_len_before: read_len_before as u64,
                bytes_returned: 0,
                assembly_bytes: assembly,
                read_completed: 0,
            });

            let read = socket.read(&mut self.available).await?;

            if read == 0 {
                self.finish_event(pushed, 0, assembly, view_prepared, growth);
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "expected to read {} bytes, got {} bytes at EOF",
                        len,
                        self.read.len()
                    ),
                ));
            }

            // A socket read stops at the spare capacity, which can pass the logical deficit.
            let overread = read.saturating_sub(deficit);
            self.meter.reads += 1;
            self.meter.bytes_read += read as u64;
            self.meter.overread_bytes += overread as u64;
            let copied = self.advance(read);
            assembly += copied;
            if copied > 0 {
                self.meter.assembly_bytes += copied;
            }
            self.sample_peaks();
            let view_done = self.view_capacity();
            let extra = view_done
                .checked_sub(view_prepared)
                .expect("incoming view shrank during advance");
            if extra > 0 {
                self.meter.reserved_additional += extra;
            }
            self.finish_event(pushed, read as u64, assembly, view_done, growth + extra);
        }

        Ok(())
    }

    /// Make `available` able to accept `deficit` more bytes.
    ///
    /// `BytesMut::reserve(n)` guarantees `capacity - len >= n`. `n` is therefore
    /// `deficit` (additional bytes beyond the current length), not `deficit` minus
    /// the current spare. Subtracting the spare and passing that remainder asks
    /// for less room than the read needs, and the call returns without growing
    /// whenever the remainder already fits in the spare.
    ///
    /// When the spare is already at least `deficit`, this does not call `reserve`.
    /// Returns the argument that was passed (0 if none) and bytes copied while
    /// relocating a retained prefix.
    fn prepare_window(&mut self, deficit: usize) -> (usize, u64) {
        let spare = self
            .available
            .capacity()
            .saturating_sub(self.available.len());
        if deficit <= spare {
            return (0, 0);
        }
        // A tail that already holds bytes is not the usual spare. Grow it in place
        // rather than rejoining those bytes into `read` twice.
        if self.available.len() != 0 {
            return self.grow_available(deficit);
        }
        // Put the spare back on `read` when the two windows are contiguous, grow
        // that one buffer, then split the spare off again. The following `unsplit`
        // of the bytes just read stays a pointer adjustment instead of a copy.
        let tail = replace(&mut self.available, BytesMut::new());
        match self.read.try_unsplit(tail) {
            Ok(()) => self.grow_read(deficit),
            Err(tail) => {
                self.available = tail;
                self.grow_available(deficit)
            }
        }
    }

    fn grow_read(&mut self, deficit: usize) -> (usize, u64) {
        let ptr = self.read.as_ptr();
        let old = self.read.len();
        self.read.reserve(deficit);
        let assembly = if old > 0 && self.read.as_ptr() != ptr {
            old as u64
        } else {
            0
        };
        let kept = self.read.len();
        self.available = self.read.split_off(kept);
        (deficit, assembly)
    }

    fn grow_available(&mut self, deficit: usize) -> (usize, u64) {
        let ptr = self.available.as_ptr();
        let old = self.available.len();
        self.available.reserve(deficit);
        let assembly = if old > 0 && self.available.as_ptr() != ptr {
            old as u64
        } else {
            0
        };
        (deficit, assembly)
    }

    fn note_reserve(&mut self, argument: usize, growth: u64) {
        if argument == 0 && growth == 0 {
            return;
        }
        if argument > 0 {
            self.meter.reserve_calls += 1;
            self.meter.reserve_argument_bytes += argument as u64;
        }
        if growth == 0 {
            self.meter.noop_reserve_calls += 1;
            self.meter.noop_reserve_arguments += argument as u64;
        } else {
            self.meter.reserved_additional += growth;
        }
    }

    fn note_rounding(&mut self, argument: usize, growth: u64) {
        if argument == 0 || growth == 0 {
            return;
        }
        let spare_after = self
            .available
            .capacity()
            .saturating_sub(self.available.len());
        if spare_after > argument {
            let slack = (spare_after - argument) as u64;
            self.meter.rounding_peak = self.meter.rounding_peak.max(slack);
        }
    }

    fn push_event(&mut self, event: IncomingAllocEvent) -> bool {
        if self.meter.events.len() == MAX_INCOMING_ALLOC_EVENTS {
            self.meter.events_dropped += 1;
            return false;
        }
        self.meter.events.push(event);
        true
    }

    fn finish_event(
        &mut self,
        pushed: bool,
        bytes_returned: u64,
        assembly: u64,
        view_done: u64,
        growth: u64,
    ) {
        if !pushed {
            return;
        }
        let event = self
            .meter
            .events
            .last_mut()
            .expect("pushed incoming allocation event");
        event.bytes_returned = bytes_returned;
        event.assembly_bytes = assembly;
        event.view_capacity_after = view_done;
        event.reserved_growth = growth;
        event.read_completed = 1;
    }

    /// Move `amt` bytes from `available` onto `read`.
    ///
    /// An empty `read` takes the view. A contiguous tail is `unsplit` in O(1).
    /// Anything else is appended, and the returned count is the number of bytes copied.
    fn advance(&mut self, amt: usize) -> u64 {
        let piece = self.available.split_to(amt);
        if self.read.is_empty() {
            self.read = piece;
            return 0;
        }
        let piece_len = piece.len();
        match self.read.try_unsplit(piece) {
            Ok(()) => 0,
            Err(piece) => {
                let ptr = self.read.as_ptr();
                let old = self.read.len();
                self.read.extend_from_slice(&piece);
                let mut copied = piece_len as u64;
                if old > 0 && self.read.as_ptr() != ptr {
                    copied += old as u64;
                }
                copied
            }
        }
    }

    fn shrink(&mut self) {
        if self.available.capacity() > DEFAULT_BUF_SIZE {
            // `BytesMut` doesn't have a way to shrink its capacity,
            // but we only use `available` for spare capacity anyway so we can just replace it.
            //
            // If `self.read` still contains data on the next call to `advance` then this might
            // force a memcpy as they'll no longer be pointing to the same allocation,
            // but that's kind of unavoidable.
            //
            // The `async-std` impl of `Socket` will also need to re-zero the buffer,
            // but that's also kind of unavoidable.
            //
            // We should be warning the user not to call this often.
            self.available = BytesMut::with_capacity(DEFAULT_BUF_SIZE);
        }
    }
}
