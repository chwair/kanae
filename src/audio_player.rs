use std::num::{NonZeroU16, NonZeroU32, NonZero};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver, TrySendError, TryRecvError};
use rodio::{MixerDeviceSink, DeviceSinkBuilder, Player, Source};

// streaming source

/// continuous audio source backed by an mpsc channel. one source per track keeps
/// rodio's sample-rate converter from being reset at each SamplesBuffer boundary,
/// which would cause an audible click/pop.
struct ChannelSource {
    /// chunks are tagged with the seek generation they were decoded for, so a
    /// seek can discard whatever was queued before it without a new stream.
    receiver: Receiver<Option<(u64, Vec<f32>)>>,
    sample_rate: NonZeroU32,
    channels: NonZeroU16,
    /// iterator over the currently-active chunk; cheap to drain sample-by-sample.
    current: std::vec::IntoIter<f32>,
    done: bool,
    /// cumulative f32 values actually handed to the audio device. counted per
    /// sample rather than per chunk so it tracks what is being *heard*, which
    /// the seek bar and the visualizer both key off.
    samples_emitted: Arc<AtomicU64>,
    /// playback gain, read here rather than at decode time. scaling decoded
    /// chunks before they enter the channel puts the whole buffer's worth of
    /// audio (~400 ms) between a slider move and the change being heard.
    volume: Arc<AtomicU64>,
    /// gain actually applied, chased toward `volume` a sample at a time so a
    /// jump in the slider never lands as a step discontinuity (a click).
    gain: f32,
    /// seek requests from the player; a generation newer than `cur_gen` means
    /// everything still queued is stale.
    seek: Arc<SeekSignal>,
    /// generation of the audio currently being played.
    cur_gen: u64,
    /// generation `samples_emitted` is counting for. the counter is reset at
    /// the switch, so the decode thread only trusts it once this matches.
    emitted_gen: Arc<AtomicU64>,
    /// samples left in the fade-out of the old position while a seek flushes.
    fade_out: u32,
    /// samples played since the last flush, for the fade-in of the new position.
    fade_in: u32,
    /// length of both seek fades, in interleaved samples.
    fade_len: u32,
    /// a newer chunk that arrived mid fade-out, held until the fade completes.
    stash: Option<(u64, Vec<f32>)>,
}

/// per-sample smoothing coefficient: closes most of a volume jump in ~10 ms at
/// 48 kHz, fast enough to feel immediate and slow enough to stay inaudible.
const VOL_SMOOTH: f32 = 0.002;

/// seek fade length: long enough that cutting away from the old position and
/// into the new one never lands as a click, short enough to feel instant.
const SEEK_FADE_MS: u32 = 6;

impl ChannelSource {
    #[inline]
    fn shaped(&mut self, s: f32) -> f32 {
        let target = f64::from_bits(self.volume.load(Ordering::Relaxed)) as f32;
        self.gain += (target - self.gain) * VOL_SMOOTH;
        (s * self.gain).clamp(-1.0, 1.0)
    }

    /// next raw sample of the current generation, pulling queued chunks as
    /// needed and dropping ones decoded for a position that was seeked away
    /// from. `None` means nothing is ready (an underrun, or a seek decoding).
    fn pull(&mut self) -> Option<f32> {
        loop {
            if let Some(s) = self.current.next() { return Some(s); }
            let (gen, chunk) = match self.stash.take() {
                Some(c) => c,
                None => match self.receiver.try_recv() {
                    Ok(Some(c)) => c,
                    Ok(None) | Err(TryRecvError::Disconnected) => {
                        self.done = true;
                        return None;
                    }
                    Err(TryRecvError::Empty) => return None,
                },
            };
            if gen < self.cur_gen { continue; }
            if gen > self.cur_gen {
                self.stash = Some((gen, chunk));
                return None;
            }
            self.current = chunk.into_iter();
        }
    }

    /// drop the old position and start counting for the new one.
    fn switch_to(&mut self, gen: u64) {
        self.current = Vec::new().into_iter();
        self.cur_gen = gen;
        self.fade_in = 0;
        self.samples_emitted.store(0, Ordering::Relaxed);
        self.emitted_gen.store(gen, Ordering::Release);
    }
}

impl Iterator for ChannelSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.done {
            return None;
        }
        // nothing here may block: this runs on the audio callback.
        let want = self.seek.generation();
        if want != self.cur_gen && self.fade_out == 0 {
            self.fade_out = self.fade_len;
        }

        if self.fade_out > 0 {
            // a seek is pending: ramp the old position down before cutting it.
            let s = self.pull();
            let g = self.fade_out as f32 / self.fade_len as f32;
            self.fade_out -= 1;
            // running dry mid-fade leaves nothing to ramp, so cut right away
            if self.fade_out == 0 || s.is_none() {
                self.fade_out = 0;
                self.switch_to(want);
            }
            if self.done { return None; }
            return Some(self.shaped(s.unwrap_or(0.0) * g));
        }

        match self.pull() {
            Some(s) => {
                self.samples_emitted.fetch_add(1, Ordering::Relaxed);
                let s = if self.fade_in < self.fade_len {
                    self.fade_in += 1;
                    s * (self.fade_in as f32 / self.fade_len as f32)
                } else {
                    s
                };
                Some(self.shaped(s))
            }
            None if self.done => None,
            // underrun, or the new position still decoding: emit silence
            // rather than terminating the source.
            None => Some(self.shaped(0.0)),
        }
    }
}

impl Source for ChannelSource {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> NonZero<u16> { self.channels }
    fn sample_rate(&self) -> NonZero<u32> { self.sample_rate }
    fn total_duration(&self) -> Option<std::time::Duration> { None }
}

// StreamSender

/// producer end of a `ChannelSource`.
pub struct StreamSender {
    sender: SyncSender<Option<(u64, Vec<f32>)>>,
    stop_flag: Arc<AtomicBool>,
    seek: Arc<SeekSignal>,
}

impl StreamSender {
    /// push a chunk decoded for seek generation `gen`.  spins with 5 ms sleeps
    /// when the channel is full so the audio device can catch up, giving up
    /// early (dropping the chunk) once a newer seek has made it stale.
    /// returns `false` when a stop was requested or the sink was closed.
    pub fn send(&self, gen: u64, mut samples: Vec<f32>) -> bool {
        loop {
            if self.stop_flag.load(Ordering::Relaxed) {
                return false;
            }
            if self.seek.generation() != gen {
                return true;
            }
            match self.sender.try_send(Some((gen, samples))) {
                Ok(_) => return true,
                Err(TrySendError::Full(Some((_, s)))) => {
                    samples = s;
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(_) => return false, // disconnected
            }
        }
    }

    /// signal end-of-stream; silently ignored if the receiver has been dropped.
    pub fn finish(self) {
        let _ = self.sender.send(None);
    }
}

// AudioController

/// a seek handed to a running stream instead of restarting it, which would
/// reopen the audio device (slow to start) and cut the old audio mid-wave. the
/// target is written before the generation is bumped, so whoever sees a new
/// generation also sees its target.
#[derive(Default)]
pub struct SeekSignal {
    gen: AtomicU64,
    target: AtomicU64,
}

impl SeekSignal {
    pub fn request(&self, secs: f64) {
        self.target.store(secs.to_bits(), Ordering::Relaxed);
        self.gen.fetch_add(1, Ordering::Release);
    }
    pub fn generation(&self) -> u64 { self.gen.load(Ordering::Acquire) }
    pub fn target(&self) -> f64 { f64::from_bits(self.target.load(Ordering::Relaxed)) }
}

pub struct AudioController {
    // keeping the sink alive prevents the audio device from shutting down.
    _sink_handle: MixerDeviceSink,
    player: Player,
}

impl AudioController {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut sink_handle = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Failed to open audio sink: {:?}", e))?;
        // a sink is opened per track; the drop notice is expected, not a bug.
        sink_handle.log_on_drop(false);
        let player = Player::connect_new(&sink_handle.mixer());
        Ok(Self { _sink_handle: sink_handle, player })
    }

    /// attach a continuous streaming source. returns a `StreamSender` to push decoded chunks, an
    /// `Arc<AtomicU64>` counting f32 values consumed since the last seek (for heard-position
    /// tracking), and the seek generation that counter belongs to. `gen` is the generation of
    /// `seek` the stream starts at; `buffer_packets` sets the channel capacity (30 gives ~780 ms
    /// of mp3 headroom); `volume` is read by the source itself, one sample ahead of the device,
    /// so the buffer depth does not become volume latency.
    pub fn begin_stream(
        &self,
        sample_rate: u32,
        channels: u16,
        stop_flag: Arc<AtomicBool>,
        buffer_packets: usize,
        volume: Arc<AtomicU64>,
        seek: Arc<SeekSignal>,
        gen: u64,
    ) -> (StreamSender, Arc<AtomicU64>, Arc<AtomicU64>) {
        let (tx, rx) = sync_channel(buffer_packets);
        let samples_emitted = Arc::new(AtomicU64::new(0));
        let emitted_gen = Arc::new(AtomicU64::new(gen));
        // start at the current level: ramping up from silence would turn every
        // track start into an unasked-for fade-in.
        let gain = f64::from_bits(volume.load(Ordering::Relaxed)) as f32;
        let source = ChannelSource {
            receiver: rx,
            sample_rate: NonZeroU32::new(sample_rate.max(1)).unwrap(),
            channels: NonZeroU16::new(channels.max(1)).unwrap(),
            current: Vec::new().into_iter(),
            done: false,
            samples_emitted: samples_emitted.clone(),
            volume,
            gain,
            seek: seek.clone(),
            cur_gen: gen,
            emitted_gen: emitted_gen.clone(),
            fade_out: 0,
            // a track start needs no fade-in; only seeks land mid-waveform
            fade_in: u32::MAX,
            fade_len: (sample_rate * channels as u32 * SEEK_FADE_MS / 1000).max(1),
            stash: None,
        };
        self.player.append(source);
        (StreamSender { sender: tx, stop_flag, seek }, samples_emitted, emitted_gen)
    }

    /// append a raw `SamplesBuffer` - used by cd playback which already manages
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

// cd helpers

pub fn bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2)
        .map(|chunk| {
            let i16_sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            i16_sample as f32 / 32768.0
        })
        .collect()
}

// spectrum visualizer

/// default bar count, and the band count the tilt/bleed constants were tuned
/// against - both are scaled relative to it so a wider sidebar looks the same.
pub const VIS_BANDS: usize = 20;
const VIS_BANDS_REF: f32 = 20.0;
pub const VIS_BANDS_MIN: usize = 8;
pub const VIS_BANDS_MAX: usize = 64;
/// fft window size (power of two). at 44.1 kHz this is ~23 ms of audio, a good
/// trade between frequency resolution and responsiveness.
const VIS_FFT: usize = 1024;
/// frequency mapping assumes a typical rate - the band layout is aesthetic, not
/// measurement-grade, so a fixed reference keeps `VisTap` free of rate state.
const VIS_RATE: f32 = 44100.0;

/// ring capacity in mono frames (~1.5 s at 44.1 kHz). the decode threads run
/// ahead of the audio device by up to the stream buffer, so the ring has to
/// hold that much history for the window being *heard* to still be resident.
const VIS_RING: usize = 65536;

/// lock-protected ring of recent mono samples, fed pre-volume by the decode threads and drained by
/// the gui once per frame. decoding runs ahead of playback, so reading the newest samples would show
/// the spectrum roughly a buffer's worth of audio early. instead the tap keeps a long history and
/// `attach` binds it to the device's consumed-sample counter, so the window read back is the one
/// currently coming out of the speakers.
pub struct VisTap {
    buf: Vec<f32>,
    /// write cursor into `buf`.
    pos: usize,
    /// total mono frames ever written.
    written: u64,
    /// live count of f32 values the audio device has consumed, plus the channel
    /// count needed to convert it to frames. `None` means "read the newest
    /// samples", used by callers with no playback of their own.
    played: Option<(Arc<AtomicU64>, u16)>,
}

impl VisTap {
    pub fn new() -> Self {
        Self { buf: vec![0.0; VIS_RING], pos: 0, written: 0, played: None }
    }

    /// bind the tap to a stream's consumed-sample counter and clear the ring.
    /// call once per track, right after the stream is opened.
    pub fn attach(&mut self, played: Arc<AtomicU64>, channels: u16) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.pos = 0;
        self.written = 0;
        self.played = Some((played, channels.max(1)));
    }

    /// down-mix an interleaved chunk to mono and append it to the ring.
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

    /// how far the write cursor is ahead of what has actually been played,
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

    /// copy the `VIS_FFT` samples ending at the current playback point out in
    /// chronological order.
    fn snapshot(&self, out: &mut [f32; VIS_FFT]) {
        let start = (self.pos + VIS_RING - self.lag_frames() - VIS_FFT) % VIS_RING;
        for k in 0..VIS_FFT {
            out[k] = self.buf[(start + k) % VIS_RING];
        }
    }
}

/// in-place iterative radix-2 cooley-tukey fft. `re`/`im` length must be a
/// power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    // bit-reversal permutation.
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

/// snapshot the tap and reduce it to `bands` **linear** magnitudes (log-spaced
/// in frequency). no gain, curve, or clamp is applied here; the caller
/// (`update_visualizer`) handles auto-gain, logarithmic scaling, and gravity. the
/// band count is chosen by the frontend from the width it has to draw in, and is
/// clamped to `VIS_BANDS_MIN..=VIS_BANDS_MAX`.
pub fn compute_bands(tap: &VisTap, bands: usize) -> Vec<f32> {
    let n = bands.clamp(VIS_BANDS_MIN, VIS_BANDS_MAX);
    let mut re = [0.0f32; VIS_FFT];
    let mut im = [0.0f32; VIS_FFT];
    {
        let mut samples = [0.0f32; VIS_FFT];
        tap.snapshot(&mut samples);
        // hann window to suppress spectral leakage.
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
        // linear magnitude with a gentle high-frequency tilt (highs carry less
        // energy) so they aren't perpetually dwarfed by the bass.
        let tilt = 1.0 + (b as f32 / n as f32) * (VIS_BANDS_REF * 0.08);
        out[b] = amp * tilt;
    }

    // monstercat-style spatial smoothing: every bar bleeds into its neighbours
    // through a `max`, so a lone noisy fft bin becomes a smooth hill instead of
    // a jittering spike - while true peaks are preserved (a bar is never pulled
    // below its own value). this is the single biggest "quality" step and what
    // cava / monstercat-style visualizers rely on.
    let raw = out.clone();
    // spread the bleed over the same fraction of the display regardless of how
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

#[cfg(test)]
mod tests {
    use super::*;

    // a 1 kHz mono source at 48 kHz, fed straight through the channel
    fn source(seek: &Arc<SeekSignal>) -> (ChannelSource, SyncSender<Option<(u64, Vec<f32>)>>) {
        let (tx, rx) = sync_channel(8);
        let src = ChannelSource {
            receiver: rx,
            sample_rate: NonZeroU32::new(48_000).unwrap(),
            channels: NonZeroU16::new(1).unwrap(),
            current: Vec::new().into_iter(),
            done: false,
            samples_emitted: Arc::new(AtomicU64::new(0)),
            volume: Arc::new(AtomicU64::new(1.0f64.to_bits())),
            gain: 1.0,
            seek: seek.clone(),
            cur_gen: 0,
            emitted_gen: Arc::new(AtomicU64::new(0)),
            fade_out: 0,
            fade_in: u32::MAX,
            fade_len: 48_000 * SEEK_FADE_MS / 1000,
            stash: None,
        };
        (src, tx)
    }

    fn sine(phase: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (phase + i as f32 * 2.0 * std::f32::consts::PI / 48.0).sin()).collect()
    }

    #[test]
    fn seek_drops_stale_audio_and_restarts_the_counter() {
        let seek = Arc::new(SeekSignal::default());
        let (mut src, tx) = source(&seek);
        tx.send(Some((0, vec![0.5; 4800]))).unwrap();
        tx.send(Some((0, vec![0.5; 4800]))).unwrap();
        for _ in 0..100 { src.next(); }

        seek.request(42.0);
        tx.send(Some((1, vec![-0.5; 4800]))).unwrap();
        let out: Vec<f32> = (0..2000).map(|_| src.next().unwrap()).collect();

        // the old position fades out, then only the new one is heard
        assert!(out[..src.fade_len as usize].iter().all(|&s| s >= 0.0));
        assert!(out[src.fade_len as usize + 1..].iter().all(|&s| s < 0.0));
        assert_eq!(src.emitted_gen.load(Ordering::Relaxed), 1);
        assert_eq!(src.samples_emitted.load(Ordering::Relaxed), 2000 - src.fade_len as u64);
        assert_eq!(seek.target(), 42.0);
    }

    #[test]
    fn seek_between_opposite_phases_has_no_jump() {
        let seek = Arc::new(SeekSignal::default());
        let (mut src, tx) = source(&seek);
        tx.send(Some((0, sine(0.0, 4800)))).unwrap();
        for _ in 0..1012 { src.next(); }

        // cut at a peak into the opposite peak: a hard switch would jump by ~2
        seek.request(10.0);
        tx.send(Some((1, sine(std::f32::consts::PI * 1.5, 4800)))).unwrap();
        let mut prev = src.next().unwrap();
        for _ in 0..3000 {
            let s = src.next().unwrap();
            // a 1 kHz sine at 48 kHz moves at most ~0.13 per sample
            assert!((s - prev).abs() < 0.15, "jump {} -> {}", prev, s);
            prev = s;
        }
    }

    #[test]
    fn repeated_seeks_settle_on_the_latest() {
        let seek = Arc::new(SeekSignal::default());
        let (mut src, tx) = source(&seek);
        tx.send(Some((0, vec![0.1; 4800]))).unwrap();
        src.next();
        seek.request(1.0);
        tx.send(Some((1, vec![0.2; 4800]))).unwrap();
        seek.request(2.0);
        tx.send(Some((2, vec![0.3; 4800]))).unwrap();
        let last = (0..3000).map(|_| src.next().unwrap()).last().unwrap();
        assert!((last - 0.3).abs() < 1e-6);
        assert_eq!(src.emitted_gen.load(Ordering::Relaxed), 2);
    }
}
