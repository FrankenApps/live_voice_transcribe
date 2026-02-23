use std::sync::{Arc, mpsc};
use std::time::Duration;

use iced::widget::text_input;
use iced::{
    Border, Center, Color, Element, Font,
    Length::Fill,
    Subscription, Task, Theme,
    alignment::{Horizontal::Right, Vertical::Top},
    border::Radius,
    widget::{
        button, center, column, combo_box, container, pick_list, row, space, stack, text,
        text_editor,
    },
    window,
};

use crate::audio::AudioManager;
use crate::model::{AudioInputDevice, TranscriptionCommand, spawn_model_thread};
use crate::ui_helpers::modal;

mod audio;
mod model;
mod ui_helpers;

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// Snackbar animation constants (each tick = 100 ms).
/// Maximum clip height used for the slide animation. Must be ≥ the toast's
/// natural rendered height (padding + text). 70 px is generous for 1–2 lines.
const SNACKBAR_MAX_HEIGHT: f32 = 70.0;
/// Ticks spent sliding in (0 → max height).
const SNACKBAR_ENTER_TICKS: u8 = 7;
/// Ticks spent sliding out (max height → 0).
const SNACKBAR_EXIT_TICKS: u8 = 7;
/// Total lifetime of a snackbar in ticks (enter + visible + exit).
const SNACKBAR_TICKS: u8 = 64; // 7 enter + 50 visible + 7 exit

/// Smooth-step (ease-in / ease-out) easing curve, t ∈ [0, 1].
fn smooth_step(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn main() -> iced::Result {
    iced::application(
        VoiceRecorder::new,
        VoiceRecorder::update,
        VoiceRecorder::view,
    )
    .theme(VoiceRecorder::theme)
    .subscription(VoiceRecorder::subscription)
    .settings(iced::Settings {
        antialiasing: true,
        fonts: vec![std::borrow::Cow::Borrowed(VoiceRecorder::ICON_FONT)],
        ..Default::default()
    })
    .title(VoiceRecorder::title)
    .window(window::Settings {
        exit_on_close_request: true,
        icon: Some(
            iced::window::icon::from_rgba(
                include_bytes!("../assets/icon/icon.rgba").to_vec(),
                128,
                128,
            )
            .unwrap(),
        ),
        size: (500.0, 550.0).into(),
        ..Default::default()
    })
    .run()
}

struct ActiveSnackbar {
    message: String,
    background: Color,
    ticks_remaining: u8,
}

impl ActiveSnackbar {
    fn new(message: impl Into<String>, background: Color) -> Self {
        Self {
            message: message.into(),
            background,
            // Start one tick into the enter animation so the first rendered
            // frame already shows a non-zero height (avoids a blank first frame).
            ticks_remaining: SNACKBAR_TICKS - 1,
        }
    }
}

struct VoiceRecorder {
    audio_input_devices: combo_box::State<AudioInputDevice>,
    is_recording: bool,
    /// True while the transcription model is loading at startup.
    is_loading: bool,
    spinner_frame: usize,
    model_ready_receiver: Option<mpsc::Receiver<Result<(), String>>>,
    model_sender: mpsc::Sender<TranscriptionCommand>,
    transcription_receiver: mpsc::Receiver<String>,
    editor_content: text_editor::Content,
    selected_audio_input_device: Option<AudioInputDevice>,
    show_settings: bool,
    theme: Theme,
    snackbar: Option<ActiveSnackbar>,
}

#[derive(Clone)]
enum Message {
    ChangeTheme(Theme),
    ChooseAudioInputDevice(AudioInputDevice),
    CopyToClipboard,
    EditorAction(text_editor::Action),
    HideModal,
    RefreshAudioDevices,
    ShowModal,
    Tick,
    ToggleRecording,
}

impl VoiceRecorder {
    const ICON_FONT: &'static [u8] = include_bytes!("../assets/fonts/icons.ttf");

    fn new() -> Self {
        let audio_manager = AudioManager::new();
        let audio_input_devices = audio_manager.find_input_devices();

        let preselected_input_device = audio_input_devices
            .clone()
            .into_iter()
            .find(|device| device.default)
            .or_else(|| audio_input_devices.first().cloned());

        let (model_tx, ready_rx, text_rx) = spawn_model_thread();

        Self {
            audio_input_devices: combo_box::State::new(audio_input_devices),
            is_recording: false,
            is_loading: true,
            spinner_frame: 0,
            model_ready_receiver: Some(ready_rx),
            model_sender: model_tx,
            transcription_receiver: text_rx,
            editor_content: text_editor::Content::new(),
            selected_audio_input_device: preselected_input_device,
            show_settings: false,
            theme: Theme::Dark,
            snackbar: None,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn title(&self) -> String {
        if self.is_loading {
            "Voice Recorder - Starting...".to_string()
        } else if self.is_recording {
            "Voice Recorder - Recording...".to_string()
        } else {
            "Voice Recorder".to_string()
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ChooseAudioInputDevice(audio_input_device) => {
                self.selected_audio_input_device = Some(audio_input_device);
                Task::none()
            }
            Message::HideModal => {
                self.show_settings = false;
                Task::none()
            }
            Message::RefreshAudioDevices => {
                let new_devices = AudioManager::new().find_input_devices();
                let new_selection = new_devices
                    .iter()
                    .find(|d| d.default)
                    .cloned()
                    .or_else(|| {
                        self.selected_audio_input_device
                            .as_ref()
                            .and_then(|current| {
                                new_devices.iter().find(|d| d.name == current.name).cloned()
                            })
                    })
                    .or_else(|| new_devices.first().cloned());
                self.audio_input_devices = combo_box::State::new(new_devices);
                self.selected_audio_input_device = new_selection;
                Task::none()
            }
            Message::ShowModal => {
                self.show_settings = true;
                Task::none()
            }
            Message::ChangeTheme(theme) => {
                self.theme = theme;
                Task::none()
            }
            Message::CopyToClipboard => {
                self.snackbar = Some(ActiveSnackbar::new(
                    "Copied to clipboard.",
                    Color {
                        r: 0.1,
                        g: 0.6,
                        b: 0.2,
                        a: 0.75,
                    },
                ));
                iced::clipboard::write(self.editor_content.text())
            }
            Message::EditorAction(action) => {
                if !action.is_edit() {
                    self.editor_content.perform(action);
                }
                Task::none()
            }
            Message::Tick => {
                if self.is_loading {
                    self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                }

                // Check whether the model has finished loading.
                if let Some(rx) = &self.model_ready_receiver {
                    match rx.try_recv() {
                        Ok(Ok(())) => {
                            self.is_loading = false;
                            self.model_ready_receiver = None;
                            self.snackbar = Some(ActiveSnackbar::new(
                                "Model loaded — ready to transcribe.",
                                Color {
                                    r: 0.1,
                                    g: 0.6,
                                    b: 0.2,
                                    a: 0.75,
                                },
                            ));
                        }
                        Ok(Err(e)) => {
                            self.is_loading = false;
                            self.model_ready_receiver = None;
                            self.snackbar = Some(ActiveSnackbar::new(
                                e,
                                Color {
                                    r: 0.75,
                                    g: 0.15,
                                    b: 0.15,
                                    a: 0.75,
                                },
                            ));
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            self.is_loading = false;
                            self.model_ready_receiver = None;
                            self.snackbar = Some(ActiveSnackbar::new(
                                "Model thread crashed unexpectedly.",
                                Color {
                                    r: 0.75,
                                    g: 0.15,
                                    b: 0.15,
                                    a: 0.75,
                                },
                            ));
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                }

                // Auto-dismiss the snackbar after its timer expires.
                if let Some(ref mut sb) = self.snackbar {
                    if sb.ticks_remaining == 0 {
                        self.snackbar = None;
                    } else {
                        sb.ticks_remaining -= 1;
                    }
                }

                // Drain all pending transcription fragments into the editor.
                while let Ok(text) = self.transcription_receiver.try_recv() {
                    self.editor_content
                        .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
                    self.editor_content.perform(text_editor::Action::Edit(
                        text_editor::Edit::Paste(Arc::new(text)),
                    ));
                }
                Task::none()
            }
            Message::ToggleRecording => {
                self.is_recording = !self.is_recording;

                if self.is_recording {
                    if let Some(device) = &mut self.selected_audio_input_device {
                        device.start_recording(self.model_sender.clone());
                    }
                } else {
                    if let Some(device) = &mut self.selected_audio_input_device {
                        device.stop_recording();
                    }
                    let _ = self.model_sender.send(TranscriptionCommand::Flush);
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let recording_button_icon = if self.is_recording {
            '\u{0e901}'
        } else {
            '\u{0e91e}'
        };

        let status_label: Option<Element<Message>> = if self.is_loading {
            Some(
                text(format!(
                    "{} Starting model…",
                    SPINNER_FRAMES[self.spinner_frame]
                ))
                .size(13)
                .into(),
            )
        } else {
            None
        };

        // The record button is only enabled once the model is ready.
        let record_button = button(center(
            text(recording_button_icon)
                .font(Font::with_name("icomoon"))
                .shaping(text::Shaping::Basic)
                .size(60),
        ))
        .height(100)
        .width(100)
        .style(|theme, status| {
            let mut primary = button::primary(theme, status);
            primary.border = Border {
                color: primary.border.color,
                width: primary.border.width,
                radius: Radius::new(50),
            };
            primary
        });

        let record_button: Element<Message> = if self.is_loading {
            record_button.into()
        } else {
            record_button.on_press(Message::ToggleRecording).into()
        };

        let device_display = self
            .selected_audio_input_device
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default();

        let audio_device_selector: Element<Message> = if self.is_recording {
            text_input("Choose audio input device...", &device_display).into()
        } else {
            combo_box(
                &self.audio_input_devices,
                "Choose audio input device...",
                self.selected_audio_input_device.as_ref(),
                Message::ChooseAudioInputDevice,
            )
            .into()
        };

        let refresh_button: Element<Message> = {
            let btn = button(text('\u{0e984}')).style(button::subtle);
            if self.is_recording {
                btn.into()
            } else {
                btn.on_press(Message::RefreshAudioDevices).into()
            }
        };

        let mut page = column![
            row![
                space::horizontal(),
                button(
                    text('\u{0e994}')
                        .font(Font::with_name("icomoon"))
                        .shaping(text::Shaping::Basic)
                )
                .style(button::subtle)
                .on_press(Message::ShowModal)
            ]
            .align_y(Top)
            .width(Fill),
            row![
                text("Audio Input Device:"),
                audio_device_selector,
                refresh_button,
            ]
            .align_y(Center)
            .spacing(20)
            .width(Fill),
            record_button,
        ]
        .align_x(Center)
        .height(Fill)
        .padding(20)
        .spacing(10);

        if let Some(label) = status_label {
            page = page.push(label);
        }

        let copy_button = container(
            button(
                text('\u{0e9b8}')
                    .font(Font::with_name("icomoon"))
                    .shaping(text::Shaping::Basic)
                    .size(14),
            )
            .style(button::subtle)
            .padding([3, 6])
            .on_press(Message::CopyToClipboard),
        )
        .align_x(Right)
        .align_y(Top)
        .width(Fill)
        .padding(4);

        let editor_area = stack![
            text_editor(&self.editor_content)
                .on_action(Message::EditorAction)
                .height(Fill),
            copy_button,
        ]
        .height(Fill);

        let page = page.push(editor_area);

        let base: Element<Message> = if self.show_settings {
            let settings = container(
                column![
                    text("Settings").size(24),
                    column![
                        column![
                            text("Theme").size(12),
                            pick_list(Theme::ALL, Some(self.theme.clone()), Message::ChangeTheme)
                                .padding(5),
                        ]
                        .spacing(5),
                        row![
                            space::horizontal(),
                            button(text("Save")).on_press(Message::HideModal),
                        ]
                        .width(Fill),
                    ]
                    .spacing(10)
                ]
                .spacing(10),
            )
            .width(300)
            .padding(10)
            .style(container::rounded_box);

            modal(page, settings, Message::HideModal)
        } else {
            page.into()
        };

        if let Some(snackbar) = &self.snackbar {
            // Compute animated clip height using smooth-step easing.
            //
            // Timeline (each step = 1 tick = 100 ms):
            //   ticks SNACKBAR_TICKS-1 … SNACKBAR_TICKS-SNACKBAR_ENTER_TICKS+1
            //     → entering  (height 0 → max)
            //   ticks SNACKBAR_TICKS-SNACKBAR_ENTER_TICKS … SNACKBAR_EXIT_TICKS+1
            //     → fully visible (height = max)
            //   ticks SNACKBAR_EXIT_TICKS … 0
            //     → exiting   (height max → 0)
            let enter_threshold = SNACKBAR_TICKS - SNACKBAR_ENTER_TICKS;
            let anim_height = if snackbar.ticks_remaining > enter_threshold {
                let elapsed = (SNACKBAR_TICKS - snackbar.ticks_remaining) as f32;
                smooth_step(elapsed / SNACKBAR_ENTER_TICKS as f32) * SNACKBAR_MAX_HEIGHT
            } else if snackbar.ticks_remaining <= SNACKBAR_EXIT_TICKS {
                smooth_step(snackbar.ticks_remaining as f32 / SNACKBAR_EXIT_TICKS as f32)
                    * SNACKBAR_MAX_HEIGHT
            } else {
                SNACKBAR_MAX_HEIGHT
            };
            ui_helpers::snackbar(
                base,
                snackbar.message.clone(),
                snackbar.background,
                anim_height,
            )
        } else {
            base
        }
    }
}
