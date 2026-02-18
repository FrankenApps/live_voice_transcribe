use cpal::{
    Host,
    traits::{DeviceTrait, HostTrait},
};

use crate::AudioInputDevice;

pub struct AudioManager {
    host: Host,
}

impl AudioManager {
    pub fn find_input_devices(&self) -> Vec<AudioInputDevice> {
        let devices = self
            .host
            .input_devices()
            .expect("Failed to get audio input devices.");
        let default_input_device = self.host.default_input_device();

        let mut input_devices = devices
            .map(|device| {
                let is_default = if let Some(default) = &default_input_device {
                    device.id() == default.id()
                } else {
                    false
                };

                AudioInputDevice::new(
                    device.clone(),
                    device
                        .description()
                        .expect("Failed to get name of audio input device.")
                        .name(),
                    is_default,
                )
            })
            .collect::<Vec<AudioInputDevice>>();

        // Sort devices alphabetically.
        input_devices.sort_by(|a, b| a.name.cmp(&b.name));

        input_devices
    }

    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
}
