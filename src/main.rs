use std::sync::{Arc, mpsc};
use std::time::Duration;

use iced::Length::Fill;
use iced::alignment::Vertical::Top;
use iced::border::Radius;
use iced::widget::{
    button, center, column, combo_box, container, pick_list, row, space, text, text_editor,
};
use iced::{Border, Center, Element, Font, Subscription, Theme, window};

use crate::audio::AudioManager;
use crate::model::{AudioInputDevice, TranscriptionCommand, spawn_model_thread};
use crate::ui_helpers::modal;

mod audio;
mod model;
mod ui_helpers;

const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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

struct VoiceRecorder {
    audio_input_devices: combo_box::State<AudioInputDevice>,
    is_recording: bool,
    /// True while the transcription model is loading at startup.
    is_loading: bool,
    spinner_frame: usize,
    model_ready_receiver: Option<mpsc::Receiver<()>>,
    model_sender: mpsc::Sender<TranscriptionCommand>,
    transcription_receiver: mpsc::Receiver<String>,
    editor_content: text_editor::Content,
    selected_audio_input_device: Option<AudioInputDevice>,
    show_settings: bool,
    theme: Theme,
}

#[derive(Clone)]
enum Message {
    ChangeTheme(Theme),
    ChooseAudioInputDevice(AudioInputDevice),
    EditorAction(text_editor::Action),
    HideModal,
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

    fn update(&mut self, message: Message) {
        match message {
            Message::ChooseAudioInputDevice(audio_input_device) => {
                self.selected_audio_input_device = Some(audio_input_device);
            }
            Message::HideModal => self.show_settings = false,
            Message::ShowModal => self.show_settings = true,
            Message::ChangeTheme(theme) => self.theme = theme,
            Message::EditorAction(action) => {
                if !action.is_edit() {
                    self.editor_content.perform(action);
                }
            }
            Message::Tick => {
                if self.is_loading {
                    self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                }

                // Check whether the model has finished loading.
                if let Some(rx) = &self.model_ready_receiver {
                    match rx.try_recv() {
                        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                            self.is_loading = false;
                            self.model_ready_receiver = None;
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
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
                combo_box(
                    &self.audio_input_devices,
                    "Choose audio input device...",
                    self.selected_audio_input_device.as_ref(),
                    Message::ChooseAudioInputDevice
                ),
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

        let page = page.push(
            text_editor(&self.editor_content)
                .on_action(Message::EditorAction)
                .height(Fill),
        );

        if !self.show_settings {
            return page.into();
        }

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
    }
}
