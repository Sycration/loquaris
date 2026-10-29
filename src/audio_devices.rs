//! Discover input devices without retaining open audio handles.
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Clone, Debug)]
pub struct Input {
    pub label: String,
    /// None means the backend's default input.
    pub id: Option<cpal::DeviceId>,
}

pub fn systems() -> (Vec<cpal::HostId>, usize) {
    let hosts = cpal::available_hosts();
    let default = cpal::default_host().id();
    let selected = hosts.iter().position(|host| *host == default).unwrap_or(0);
    (hosts, selected)
}

fn show_device(host: cpal::HostId, id: &str, direction: cpal::DeviceDirection) -> bool {
    #[cfg(target_os = "linux")]
    {
        // ALSA hint devices include plugins; list only physical capture devices.
        if host == cpal::HostId::Alsa {
            return id.starts_with("plughw:");
        }
        // PipeWire exposes output sinks as duplex monitor devices too.
        if host == cpal::HostId::PipeWire {
            return direction == cpal::DeviceDirection::Input && id != "input_default";
        }
        if host == cpal::HostId::PulseAudio {
            return !id.ends_with(".monitor");
        }
        // JACK exposes an auto-connected virtual input, not individual ports.
        if host == cpal::HostId::Jack {
            return false;
        }
    }
    let _ = (host, id, direction);
    true
}

pub fn inputs(system: cpal::HostId) -> Result<Vec<Input>, String> {
    let host = cpal::host_from_id(system).map_err(|e| format!("Could not open {system}: {e}"))?;
    let default = host.default_input_device();
    let mut inputs = Vec::new();
    if default.is_some() {
        inputs.push(Input {
            label: "System default".into(),
            id: None,
        });
    }
    for device in host
        .devices()
        .map_err(|e| format!("Could not list input devices: {e}"))?
    {
        let Ok(id) = device.id() else {
            continue;
        };
        let Ok(description) = device.description() else {
            continue;
        };
        if !show_device(system, id.id(), description.direction()) || !device.supports_input() {
            continue;
        }
        inputs.push(Input {
            label: description.name().to_owned(),
            id: Some(id),
        });
    }
    // Identical names must remain distinguishable and select by ID, not label.
    let labels: Vec<_> = inputs.iter().map(|input| input.label.clone()).collect();
    for index in 0..inputs.len() {
        if labels
            .iter()
            .filter(|label| **label == labels[index])
            .count()
            > 1
        {
            if let Some(id) = &inputs[index].id {
                inputs[index].label = format!("{} ({})", inputs[index].label, id.id());
            }
        }
    }
    Ok(inputs)
}

pub fn open(system: cpal::HostId, input: &Input) -> Result<(cpal::Host, cpal::Device), String> {
    let host = cpal::host_from_id(system).map_err(|e| format!("Could not open {system}: {e}"))?;
    let device = match &input.id {
        Some(id) => host.device_by_id(id),
        None => host.default_input_device(),
    }
    .ok_or("The selected input device is no longer available. Refresh the device list.")?;
    Ok((host, device))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn linux_filters_plugins_and_output_monitors() {
        use cpal::{DeviceDirection::*, HostId::*};
        assert!(!show_device(Alsa, "pipewire", Input));
        assert!(!show_device(Alsa, "pulse", Input));
        assert!(show_device(Alsa, "plughw:CARD=0,DEV=0", Input));
        assert!(!show_device(PipeWire, "input_default", Input));
        assert!(!show_device(PipeWire, "speakers", Duplex));
        assert!(show_device(PipeWire, "microphone", Input));
        assert!(!show_device(PulseAudio, "speaker.monitor", Input));
        assert!(show_device(PulseAudio, "microphone", Input));
        assert!(!show_device(Jack, "virtual", Input));
    }

    #[test]
    #[ignore = "requires a running PipeWire server"]
    fn pipewire_sources_can_be_discovered_and_reopened() {
        let inputs = inputs(cpal::HostId::PipeWire).unwrap();
        eprintln!("PipeWire inputs: {inputs:?}");
        for input in &inputs {
            assert!(open(cpal::HostId::PipeWire, input).is_ok());
        }
    }
}
