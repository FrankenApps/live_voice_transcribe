use std::sync::Arc;
use std::sync::mpsc;

use cpal::{
    Device, I24, Sample, SupportedStreamConfig, U24,
    traits::{DeviceTrait, StreamTrait},
};
use parakeet_rs::{Nemotron, NemotronMode};

use crate::types::Language;

/// Number of 16 kHz samples fed to the model per chunk (560 ms of audio):
/// the streaming Nemotron encoder consumes 56 mel frames × 160 hop length.
pub const CHUNK_SAMPLES: usize = 8960;

/// Commands sent to the persistent transcription thread.
pub enum TranscriptionCommand {
    /// A chunk of 16 kHz mono f32 audio samples to transcribe.
    Chunk(Vec<f32>),
    /// End-of-recording signal: flushes the model's remaining context.
    Flush,
    /// Switch the language the model transcribes.
    SetLanguage(Language),
}

/// Events emitted by the transcription thread.
pub enum ModelEvent {
    /// Fires once when the model has finished loading (or failed to).
    Ready(Result<(), String>),
    /// A non-fatal error worth surfacing to the user.
    Error(String),
    /// A transcribed text fragment.
    Text(String),
}

/// Spawns the persistent transcription thread.
///
/// `language` is applied once the model has loaded; switch it later via
/// [`TranscriptionCommand::SetLanguage`].
///
/// Returns a sender for commands and a receiver for model events.
pub fn spawn_model_thread(
    language: Language,
) -> (
    mpsc::Sender<TranscriptionCommand>,
    mpsc::Receiver<ModelEvent>,
) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TranscriptionCommand>();
    let (event_tx, event_rx) = mpsc::channel::<ModelEvent>();

    std::thread::spawn(move || {
        let mut model = match Nemotron::from_pretrained(".", None) {
            Ok(m) => m,
            Err(e) => {
                let _ = event_tx.send(ModelEvent::Ready(Err(format!("Failed to load model: {e}"))));
                return;
            }
        };

        let set_language = |model: &mut Nemotron, language: Language| match model.mode() {
            NemotronMode::Multilingual => {
                if let Err(e) = model.set_target_lang(language.code()) {
                    let _ =
                        event_tx.send(ModelEvent::Error(format!("Failed to set language: {e}")));
                }
            }
            NemotronMode::EnglishOnly => {
                if !matches!(language, Language::Auto | Language::English) {
                    let _ = event_tx.send(ModelEvent::Error(
                        "The loaded model only supports English. Download the multilingual \
                         Nemotron 3.5 model files to transcribe other languages."
                            .to_string(),
                    ));
                }
            }
        };

        set_language(&mut model, language);

        // Signal the UI that the model is ready.
        let _ = event_tx.send(ModelEvent::Ready(Ok(())));

        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                TranscriptionCommand::Chunk(chunk) => match model.transcribe_chunk(&chunk) {
                    Ok(text) if !text.is_empty() => {
                        let _ = event_tx.send(ModelEvent::Text(text));
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("Transcription error: {e}"),
                },
                TranscriptionCommand::Flush => {
                    let silence = vec![0.0f32; CHUNK_SAMPLES];
                    for _ in 0..3 {
                        if let Ok(text) = model.transcribe_chunk(&silence)
                            && !text.is_empty()
                        {
                            let _ = event_tx.send(ModelEvent::Text(text));
                        }
                    }
                    // Separate recording sessions with a blank line.
                    let _ = event_tx.send(ModelEvent::Text("\n".to_string()));
                    // Start the next session with fresh encoder/decoder state;
                    // the configured target language is preserved.
                    model.reset();
                }
                TranscriptionCommand::SetLanguage(language) => set_language(&mut model, language),
            }
        }
        // Thread exits only when all senders are dropped (i.e., the application exits).
    });

    (cmd_tx, event_rx)
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
        let supported_configurations = self.device.supported_input_configs().unwrap_or_else(|_| {
            panic!(
                "Failed to retrieve supported input configurations of device {}",
                self.name
            )
        });

        let config =
            AudioInputDevice::find_16khz_config(supported_configurations).unwrap_or_else(|| {
                self.device.default_input_config().unwrap_or_else(|_| {
                    panic!(
                        "Failed to read default audio input configuration of device: {}",
                        self.name
                    )
                })
            });

        let channels = config.channels();
        let sample_rate = config.sample_rate();
        let stream_config: cpal::StreamConfig = config.clone().into();
        let fmt = config.sample_format();

        // Builds a typed input stream for the given sample type `$T`, converting
        // every sample to f32 before passing it into the shared processing pipeline.
        macro_rules! build_stream {
            ($T:ty) => {{
                let mut accumulator: Vec<f32> = Vec::new();
                let model_tx = model_tx.clone();
                self.device.build_input_stream(
                    &stream_config,
                    move |data: &[$T], _: &_| {
                        let f32_data: Vec<f32> =
                            data.iter().map(|&s| f32::from_sample(s)).collect();
                        let mono = AudioInputDevice::to_mono_f32(&f32_data, channels);
                        let resampled =
                            AudioInputDevice::resample_if_needed(&mono, sample_rate as usize);
                        accumulator.extend_from_slice(&resampled);
                        while accumulator.len() >= CHUNK_SAMPLES {
                            let chunk: Vec<f32> = accumulator.drain(..CHUNK_SAMPLES).collect();
                            let _ = model_tx.send(TranscriptionCommand::Chunk(chunk));
                        }
                    },
                    |err| eprintln!("An error occurred on the audio stream: {err}"),
                    None,
                )
            }};
        }

        let stream = match fmt {
            cpal::SampleFormat::F32 => build_stream!(f32),
            cpal::SampleFormat::F64 => build_stream!(f64),
            cpal::SampleFormat::I8 => build_stream!(i8),
            cpal::SampleFormat::I16 => build_stream!(i16),
            cpal::SampleFormat::I24 => build_stream!(I24),
            cpal::SampleFormat::I32 => build_stream!(i32),
            cpal::SampleFormat::I64 => build_stream!(i64),
            cpal::SampleFormat::U8 => build_stream!(u8),
            cpal::SampleFormat::U16 => build_stream!(u16),
            cpal::SampleFormat::U24 => build_stream!(U24),
            cpal::SampleFormat::U32 => build_stream!(u32),
            cpal::SampleFormat::U64 => build_stream!(u64),
            fmt => {
                eprintln!("Unsupported sample format {fmt}: cannot start recording.");
                return;
            }
        }
        .expect("Failed to initialize stream.");

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
        let configs: Vec<_> = supported_configurations.collect();

        // Prefer F32 at 16 kHz — no conversion overhead in the capture pipeline.
        for configuration in &configs {
            if configuration.max_sample_rate() < 16000 || configuration.min_sample_rate() > 16000 {
                continue;
            }
            if configuration.sample_format() == cpal::SampleFormat::F32 {
                return Some(SupportedStreamConfig::new(
                    1,
                    16000,
                    *configuration.buffer_size(),
                    cpal::SampleFormat::F32,
                ));
            }
        }

        // Fall back to any format that supports 16 kHz; samples will be converted in software.
        for configuration in &configs {
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
