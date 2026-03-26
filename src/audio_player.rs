use std::num::{NonZeroU16, NonZeroU32, NonZero};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver, TrySendError, TryRecvError};
use rodio::{MixerDeviceSink, DeviceSinkBuilder, Player, Source};

// ─── Streaming source ──────────────────────────────────────────────────────

/// A continuous audio source backed by an mpsc channel.
///
/// Using one source per track means rodio's sample-rate converter is
/// initialised once and never reset between packets.  Resetting the converter
/// at each SamplesBuffer boundary causes a phase discontinuity that manifests
/// as an audible click/pop, which this design eliminates.
struct ChannelSource {
    receiver: Receiver<Option<Vec<f32>>>,
    sample_rate: NonZeroU32,
    channels: NonZeroU16,
    /// Iterator over the currently-active chunk; cheap to drain sample-by-sample.
    current: std::vec::IntoIter<f32>,
    done: bool,
    /// Cumulative f32 values received from the channel (updated once per chunk).
    samples_emitted: Arc<AtomicU64>,
}

impl Iterator for ChannelSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.done {
            return None;
        }
        // Fast path: still draining the current chunk.
        if let Some(s) = self.current.next() {
            return Some(s);
        }
        // Current chunk exhausted — fetch the next without blocking
        // (the audio callback must never block).
        match self.receiver.try_recv() {
            Ok(Some(chunk)) => {
                self.samples_emitted.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                self.current = chunk.into_iter();
                self.current.next().or(Some(0.0))
            }
            Ok(None) | Err(TryRecvError::Disconnected) => {
                self.done = true;
                None
            }
            Err(TryRecvError::Empty) => {
                // Transient underrun: emit silence rather than terminating the
                // source.  With buffer_packets=30 this path is essentially
                // unreachable during normal decoding.
                Some(0.0)
            }
        }
    }
}

impl Source for ChannelSource {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> NonZero<u16> { self.channels }
    fn sample_rate(&self) -> NonZero<u32> { self.sample_rate }
    fn total_duration(&self) -> Option<std::time::Duration> { None }
}

// ─── StreamSender ──────────────────────────────────────────────────────────

/// Producer end of a `ChannelSource`.
pub struct StreamSender {
    sender: SyncSender<Option<Vec<f32>>>,
    stop_flag: Arc<AtomicBool>,
}

impl StreamSender {
    /// Push a decoded chunk.  Spins with 5 ms sleeps when the channel is full
    /// so the audio device can catch up.  Returns `false` when a stop was
    /// requested or the sink was closed.
    pub fn send(&self, mut samples: Vec<f32>) -> bool {
        loop {
            if self.stop_flag.load(Ordering::Relaxed) {
                return false;
            }
            match self.sender.try_send(Some(samples)) {
                Ok(_) => return true,
                Err(TrySendError::Full(Some(s))) => {
                    samples = s;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => return false, // Disconnected
            }
        }
    }

    /// Signal end-of-stream; silently ignored if the receiver has been dropped.
    pub fn finish(self) {
        let _ = self.sender.send(None);
    }
}

// ─── AudioController ───────────────────────────────────────────────────────

pub struct AudioController {
    // Keeping the sink alive prevents the audio device from shutting down.
    _sink_handle: MixerDeviceSink,
    player: Player,
}

impl AudioController {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let sink_handle = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open audio sink: {:?}", e))?;
        let player = Player::connect_new(&sink_handle.mixer());
        Ok(Self { _sink_handle: sink_handle, player })
    }

    /// Attach a continuous streaming source and return:
    /// - a `StreamSender` to push decoded chunks, and
    /// - an `Arc<AtomicU64>` counting how many f32 values the audio thread
    ///   has received so far (for heard-position tracking).
    ///
    /// `buffer_packets` sets the channel capacity; 30 gives ~780 ms of MP3
    /// headroom and is more than enough for any supported format.
    pub fn begin_stream(
        &self,
        sample_rate: u32,
        channels: u16,
        stop_flag: Arc<AtomicBool>,
        buffer_packets: usize,
    ) -> (StreamSender, Arc<AtomicU64>) {
        let (tx, rx) = sync_channel(buffer_packets);
        let samples_emitted = Arc::new(AtomicU64::new(0));
        let source = ChannelSource {
            receiver: rx,
            sample_rate: NonZeroU32::new(sample_rate.max(1)).unwrap(),
            channels: NonZeroU16::new(channels.max(1)).unwrap(),
            current: Vec::new().into_iter(),
            done: false,
            samples_emitted: samples_emitted.clone(),
        };
        self.player.append(source);
        (StreamSender { sender: tx, stop_flag }, samples_emitted)
    }

    /// Append a raw `SamplesBuffer` — used by CD playback which already manages
    /// its own throttle and does not need per-track streaming continuity.
    pub fn append_samples(&self, samples_f32: Vec<f32>, sample_rate: u32, channels: u16) {
        if samples_f32.is_empty() { return; }
        let buffer = rodio::buffer::SamplesBuffer::new(
            NonZeroU16::new(channels.max(1)).unwrap(),
            NonZeroU32::new(sample_rate.max(1)).unwrap(),
            samples_f32,
        );
        self.player.append(buffer);
    }

    pub fn is_empty(&self) -> bool { self.player.empty() }
    pub fn stop(&self) { self.player.stop(); }
    #[allow(dead_code)]
    pub fn set_volume(&self, v: f32) { self.player.set_volume(v); }
    pub fn queue_len(&self) -> usize { self.player.len() }
}

// ─── CD helpers ────────────────────────────────────────────────────────────

pub fn bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2)
        .map(|chunk| {
            let i16_sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            i16_sample as f32 / 32768.0
        })
        .collect()
}
