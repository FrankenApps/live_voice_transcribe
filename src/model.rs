use std::sync::Arc;
use std::sync::mpsc;

use cpal::{
    Device, SupportedStreamConfig,
    traits::{DeviceTrait, StreamTrait},
};
use parakeet_rs::Nemotron;

/// Commands sent to the persistent transcription thread.
pub enum TranscriptionCommand {
    /// A chunk of 16 kHz mono f32 audio samples to transcribe.
    Chunk(Vec<f32>),
    /// End-of-recording signal: flushes the model's remaining context.
    Flush,
}

/// Spawns the persistent transcription thread.
///
/// Returns:
/// - a sender for audio chunks and flush commands
/// - a receiver that fires once when the model has finished loading
/// - a receiver that yields transcribed text fragments in real time
pub fn spawn_model_thread() -> (
    mpsc::Sender<TranscriptionCommand>,
    mpsc::Receiver<()>,
    mpsc::Receiver<String>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TranscriptionCommand>();
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (text_tx, text_rx) = mpsc::channel::<String>();

    std::thread::spawn(move || {
        let mut model = match Nemotron::from_pretrained(".", None) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to load Nemotron streaming model: {e}");
                return;
            }
        };

        // Signal the UI that the model is ready.
        let _ = ready_tx.send(());

        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                TranscriptionCommand::Chunk(chunk) => match model.transcribe_chunk(&chunk) {
                    Ok(text) if !text.is_empty() => {
                        let _ = text_tx.send(text);
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("Transcription error: {e}"),
                },
                TranscriptionCommand::Flush => {
                    for _ in 0..3 {
                        if let Ok(text) = model.transcribe_chunk(&vec![0.0f32; 8960])
                            && !text.is_empty()
                        {
                            let _ = text_tx.send(text);
                        }
                    }
                    // Separate recording sessions with a blank line.
                    let _ = text_tx.send("\n".to_string());
                }
            }
        }
        // Thread exits only when all senders are dropped (i.e., the application exits).
    });

    (cmd_tx, ready_rx, text_rx)
}

/// Represents an audio input device provided by the operating system,
/// which is ready to record audio.
#[derive(Clone)]
pub struct AudioInputDevice {
    pub default: bool,
    device: Device,
    pub name: String,
    pub recording: bool,
    stream: Option<Arc<cpal::Stream>>,
}

impl std::fmt::Display for AudioInputDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.default {
            return f.write_str(format!("{} (Default)", self.name).as_str());
        }

        f.write_str(self.name.as_str())
    }
}

impl AudioInputDevice {
    pub fn new(device: Device, name: &str, is_default: bool) -> Self {
        Self {
            default: is_default,
            device,
            name: name.to_string(),
            recording: false,
            stream: None,
        }
    }

    /// Starts the audio input stream, sending captured chunks to `model_tx`.
    pub fn start_recording(&mut self, model_tx: mpsc::Sender<TranscriptionCommand>) {
        let supported_configurations = self.device.supported_input_configs().unwrap_or_else(|_| panic!("Failed to retrieve supported input configurations of device {}",
                self.name));

        let config =
            AudioInputDevice::find_16khz_config(supported_configurations).unwrap_or_else(|| {
                self.device.default_input_config().unwrap_or_else(|_| panic!("Failed to read default audio input configuration of device: {}",
                        self.name))
            });

        let mut accumulator: Vec<f32> = Vec::new();
        let channels = config.channels();
        let sample_rate = config.sample_rate();

        let err_fn = move |err| {
            eprintln!("An error occurred on the audio stream: {err}");
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => self
                .device
                .build_input_stream(
                    &config.into(),
                    move |data: &[f32], _info: &_| {
                        let mono = AudioInputDevice::to_mono_f32(data, channels);
                        let resampled =
                            AudioInputDevice::resample_if_needed(&mono, sample_rate as usize);

                        accumulator.extend_from_slice(&resampled);
                        while accumulator.len() >= 8960 {
                            let chunk: Vec<f32> = accumulator.drain(..8960).collect();
                            let _ = model_tx.send(TranscriptionCommand::Chunk(chunk));
                        }
                    },
                    err_fn,
                    None,
                )
                .expect("Failed to initialize stream."),
            _ => panic!("Unsupported sample format."),
        };

        stream
            .play()
            .expect("Failed to start recording audio input.");

        self.stream = Some(Arc::new(stream));
        self.recording = true;
    }

    pub fn stop_recording(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        self.recording = false;
    }

    /// Performs simple linear resampling to 16 kHz.
    fn resample_if_needed(input: &[f32], rate: usize) -> Vec<f32> {
        if rate == 16000 {
            return input.to_vec();
        }

        let ratio = 16000f64 / rate as f64;
        let dst_length = ((input.len() as f64) * ratio).round() as usize;

        if dst_length == 0 {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(dst_length);
        for i in 0..dst_length {
            let src_pos = (i as f64) / ratio;
            let idx = src_pos.floor() as usize;
            let frac = src_pos - idx as f64;

            if idx + 1 < input.len() {
                let s0 = input[idx] as f64;
                let s1 = input[idx + 1] as f64;
                out.push(((1.0 - frac) * s0 + frac * s1) as f32);
            } else {
                out.push(input[input.len() - 1]);
            }
        }

        out
    }

    fn to_mono_f32(input: &[f32], channels: u16) -> Vec<f32> {
        if channels == 1 {
            return input.to_vec();
        }

        if channels == 2 {
            return input
                .chunks_exact(2)
                .map(|ch| 0.5 * (ch[0] + ch[1]))
                .collect();
        }

        let mut mono = Vec::with_capacity(input.len() / channels as usize);
        for frame in input.chunks(channels as usize) {
            let sum: f32 = frame.iter().copied().sum();
            mono.push(sum / channels as f32);
        }
        mono
    }

    fn find_16khz_config(
        supported_configurations: cpal::SupportedInputConfigs,
    ) -> Option<SupportedStreamConfig> {
        for configuration in supported_configurations {
            if configuration.max_sample_rate() < 16000 || configuration.min_sample_rate() > 16000 {
                continue;
            }

            return Some(SupportedStreamConfig::new(
                1,
                16000,
                *configuration.buffer_size(),
                configuration.sample_format(),
            ));
        }

        None
    }
}
