use std::num::{NonZeroU16, NonZeroU32};
use rodio::{MixerDeviceSink, DeviceSinkBuilder, Player};

pub struct AudioController {
    // Keep the sink alive for the duration of playback — dropping it stops all audio
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

    pub fn append_samples(&self, samples_f32: Vec<f32>, sample_rate: u32, channels: u16) {
        let buffer = rodio::buffer::SamplesBuffer::new(
            NonZeroU16::new(channels).unwrap(),
            NonZeroU32::new(sample_rate).unwrap(),
            samples_f32,
        );
        self.player.append(buffer);
    }

    pub fn is_empty(&self) -> bool {
        self.player.empty()
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    #[allow(dead_code)]
    pub fn set_volume(&self, v: f32) {
        self.player.set_volume(v);
    }

    pub fn queue_len(&self) -> usize {
        self.player.len()
    }
}

pub fn bytes_to_f32_samples(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2)
        .map(|chunk| {
            let i16_sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            i16_sample as f32 / 32768.0
        })
        .collect()
}
