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
    /// Cumulative f32 values actually handed to the audio device. Counted per
    /// sample rather than per chunk so it tracks what is being *heard*, which
    /// the seek bar and the visualizer both key off.
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
            self.samples_emitted.fetch_add(1, Ordering::Relaxed);
            return Some(s);
        }
        // Current chunk exhausted — fetch the next without blocking
        // (the audio callback must never block).
        match self.receiver.try_recv() {
            Ok(Some(chunk)) => {
                self.current = chunk.into_iter();
                match self.current.next() {
                    Some(s) => {
                        self.samples_emitted.fetch_add(1, Ordering::Relaxed);
                        Some(s)
                    }
                    None => Some(0.0),
                }
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
        let mut sink_handle = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open audio sink: {:?}", e))?;
        // A sink is opened per track; the drop notice is expected, not a bug.
        sink_handle.log_on_drop(false);
        let player = Player::connect_new(&sink_handle.mixer());
        Ok(Self { _sink_handle: sink_handle, player })
    }

    /// Attach a continuous streaming source and return:
    /// - a `StreamSender` to push decoded chunks, and
    /// - an `Arc<AtomicU64>` counting how many f32 values the audio thread
    ///   has consumed so far (for heard-position tracking).
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

// ─── Spectrum visualizer ─────────────────────────────────────────────────────

/// Default bar count, and the band count the tilt/bleed constants were tuned
/// against — both are scaled relative to it so a wider sidebar looks the same.
pub const VIS_BANDS: usize = 20;
const VIS_BANDS_REF: f32 = 20.0;
pub const VIS_BANDS_MIN: usize = 8;
pub const VIS_BANDS_MAX: usize = 64;
/// FFT window size (power of two). At 44.1 kHz this is ~23 ms of audio, a good
/// trade between frequency resolution and responsiveness.
const VIS_FFT: usize = 1024;
/// Frequency mapping assumes a typical rate — the band layout is aesthetic, not
/// measurement-grade, so a fixed reference keeps `VisTap` free of rate state.
const VIS_RATE: f32 = 44100.0;

/// Ring capacity in mono frames (~1.5 s at 44.1 kHz). The decode threads run
/// ahead of the audio device by up to the stream buffer, so the ring has to
/// hold that much history for the window being *heard* to still be resident.
const VIS_RING: usize = 65536;

/// Lock-protected ring of recent mono samples, fed pre-volume by the decode
/// threads and drained by the GUI once per frame.
///
/// Decoding runs ahead of playback, so reading the newest samples would show
/// the spectrum roughly a buffer's worth of audio early. Instead the tap keeps
/// a long history and `attach` binds it to the device's consumed-sample
/// counter, so the window read back is the one currently coming out of the
/// speakers.
pub struct VisTap {
    buf: Vec<f32>,
    /// Write cursor into `buf`.
    pos: usize,
    /// Total mono frames ever written.
    written: u64,
    /// Live count of f32 values the audio device has consumed, plus the channel
    /// count needed to convert it to frames. `None` means "read the newest
    /// samples", used by callers with no playback of their own.
    played: Option<(Arc<AtomicU64>, u16)>,
}

impl VisTap {
    pub fn new() -> Self {
        Self { buf: vec![0.0; VIS_RING], pos: 0, written: 0, played: None }
    }

    /// Bind the tap to a stream's consumed-sample counter and clear the ring.
    /// Call once per track, right after the stream is opened.
    pub fn attach(&mut self, played: Arc<AtomicU64>, channels: u16) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.pos = 0;
        self.written = 0;
        self.played = Some((played, channels.max(1)));
    }

    /// Down-mix an interleaved chunk to mono and append it to the ring.
    pub fn push(&mut self, interleaved: &[f32], channels: u16) {
        let ch = channels.max(1) as usize;
        let mut i = 0;
        while i + ch <= interleaved.len() {
            let mut m = 0.0f32;
            for c in 0..ch { m += interleaved[i + c]; }
            self.buf[self.pos] = m / ch as f32;
            self.pos = (self.pos + 1) % VIS_RING;
            self.written += 1;
            i += ch;
        }
    }

    /// How far the write cursor is ahead of what has actually been played,
    /// in mono frames, clamped to what the ring can still serve.
    fn lag_frames(&self) -> usize {
        let lag = match &self.played {
            Some((counter, ch)) => {
                let played = counter.load(Ordering::Relaxed) / *ch as u64;
                self.written.saturating_sub(played)
            }
            None => 0,
        };
        (lag as usize).min(VIS_RING - VIS_FFT)
    }

    /// Copy the `VIS_FFT` samples ending at the current playback point out in
    /// chronological order.
    fn snapshot(&self, out: &mut [f32; VIS_FFT]) {
        let start = (self.pos + VIS_RING - self.lag_frames() - VIS_FFT) % VIS_RING;
        for k in 0..VIS_FFT {
            out[k] = self.buf[(start + k) % VIS_RING];
        }
    }
}

/// In-place iterative radix-2 Cooley–Tukey FFT. `re`/`im` length must be a
/// power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 { j ^= bit; bit >>= 1; }
        j |= bit;
        if i < j { re.swap(i, j); im.swap(i, j); }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wlr, wli) = (ang.cos(), ang.sin());
        let mut base = 0;
        while base < n {
            let (mut wr, mut wi) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = base + k;
                let b = base + k + len / 2;
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr; im[b] = im[a] - ti;
                re[a] += tr;        im[a] += ti;
                let nwr = wr * wlr - wi * wli;
                wi = wr * wli + wi * wlr; wr = nwr;
            }
            base += len;
        }
        len <<= 1;
    }
}

/// Snapshot the tap and reduce it to `bands` **linear** magnitudes (log-spaced
/// in frequency). No gain, curve, or clamp is applied here — the caller
/// (`update_visualizer`) handles auto-gain, logarithmic scaling, and gravity.
///
/// The band count is chosen by the frontend from the width it has to draw in,
/// and is clamped to `VIS_BANDS_MIN..=VIS_BANDS_MAX`.
pub fn compute_bands(tap: &VisTap, bands: usize) -> Vec<f32> {
    let n = bands.clamp(VIS_BANDS_MIN, VIS_BANDS_MAX);
    let mut re = [0.0f32; VIS_FFT];
    let mut im = [0.0f32; VIS_FFT];
    {
        let mut samples = [0.0f32; VIS_FFT];
        tap.snapshot(&mut samples);
        // Hann window to suppress spectral leakage.
        for i in 0..VIS_FFT {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (VIS_FFT as f32 - 1.0)).cos();
            re[i] = samples[i] * w;
        }
    }
    fft(&mut re, &mut im);

    let mut out = vec![0.0f32; n];
    let (fmin, fmax) = (45.0f32, 16000.0f32);
    let bin_hz = VIS_RATE / VIS_FFT as f32;
    let nyq_bin = VIS_FFT / 2;
    for b in 0..n {
        let lo = fmin * (fmax / fmin).powf(b as f32 / n as f32);
        let hi = fmin * (fmax / fmin).powf((b + 1) as f32 / n as f32);
        let lo_bin = ((lo / bin_hz).floor() as usize).max(1);
        let hi_bin = ((hi / bin_hz).ceil() as usize).clamp(lo_bin + 1, nyq_bin);
        let mut sum = 0.0f32;
        for k in lo_bin..hi_bin {
            sum += (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
        let amp = sum / (hi_bin - lo_bin) as f32 * (2.0 / VIS_FFT as f32);
        // Linear magnitude with a gentle high-frequency tilt (highs carry less
        // energy) so they aren't perpetually dwarfed by the bass.
        let tilt = 1.0 + (b as f32 / n as f32) * (VIS_BANDS_REF * 0.08);
        out[b] = amp * tilt;
    }

    // Monstercat-style spatial smoothing: every bar bleeds into its neighbours
    // through a `max`, so a lone noisy FFT bin becomes a smooth hill instead of
    // a jittering spike — while true peaks are preserved (a bar is never pulled
    // below its own value). This is the single biggest "quality" step and what
    // cava / Monstercat-style visualizers rely on.
    let raw = out.clone();
    // Spread the bleed over the same fraction of the display regardless of how
    // many bars there are, so the hills keep their shape as the count changes.
    let spread = 0.8 * (VIS_BANDS_REF / n as f32).powi(2);
    for b in 0..n {
        for j in 0..n {
            if b == j { continue; }
            let dist = (b as i32 - j as i32).abs() as f32;
            let bleed = raw[b] / (1.0 + dist * dist * spread);
            if bleed > out[j] { out[j] = bleed; }
        }
    }
    out
}
