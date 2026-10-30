//! Entry audio is stored as base64-encoded WAV in the project file.
use base64::{Engine, engine::general_purpose::STANDARD};
use cpal::traits::{DeviceTrait, StreamTrait};
use eframe::egui;
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

pub struct Editor {
    pub target: (usize, usize),
    pub original: crate::Entry,
    systems: Vec<cpal::HostId>,
    selected_system: usize,
    inputs: Vec<crate::audio_devices::Input>,
    selected: usize,
    recording: Option<Recording>,
    clip: Option<String>,
    error: Option<String>,
    playback: Option<(rodio::MixerDeviceSink, rodio::Player)>,
    position: f64,
    duration: f64,
    diagnostics: Vec<String>,
    last_capture: Option<CaptureSnapshot>,
}

#[derive(Default)]
struct CaptureStats {
    callbacks: AtomicUsize,
    samples: AtomicUsize,
    peak: AtomicUsize,
}

struct CaptureSnapshot {
    callbacks: usize,
    samples: usize,
    peak: usize,
    seconds: f64,
    rate: u32,
    channels: u16,
}

struct Recording {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<i16>>>,
    error: Arc<Mutex<Option<String>>>,
    spec: hound::WavSpec,
    stats: Arc<CaptureStats>,
    started: Instant,
}

pub struct Outcome {
    pub closed: bool,
    pub changed: Option<Option<String>>,
    pub actions: Vec<crate::keybinds::Action>,
}

pub(crate) fn clip_duration(clip: &str) -> Result<f64, String> {
    let data = STANDARD.decode(clip).map_err(|e| e.to_string())?;
    let reader = hound::WavReader::new(Cursor::new(data)).map_err(|e| e.to_string())?;
    let rate = reader.spec().sample_rate;
    if rate == 0 || reader.spec().channels == 0 {
        return Err("Invalid WAV format".into());
    }
    Ok(reader.duration() as f64 / rate as f64)
}

pub(crate) fn wav_decoder(data: Vec<u8>) -> Result<rodio::Decoder<Cursor<Vec<u8>>>, String> {
    let header = hound::WavReader::new(Cursor::new(&data))
        .map_err(|e| format!("Could not decode audio: {e}"))?;
    if header.spec().channels == 0 || header.spec().sample_rate == 0 {
        return Err("Could not decode audio: invalid WAV channel count or sample rate.".into());
    }
    rodio::Decoder::try_from(Cursor::new(data)).map_err(|e| format!("Could not decode audio: {e}"))
}

/// Apply a 250 ms linear fade to each end, using the same gain for every
/// channel in a frame. Short clips use the lower of the overlapping ramps.
fn fade_recording(samples: &mut [i16], sample_rate: u32, channels: u16) {
    let channels = usize::from(channels);
    if channels == 0 || sample_rate == 0 {
        return;
    }
    let frames = samples.len() / channels;
    let fade_frames = ((u64::from(sample_rate) + 2) / 4).max(1) as usize;
    for (frame, values) in samples.chunks_exact_mut(channels).enumerate() {
        let distance = frame.min(frames - 1 - frame).min(fade_frames);
        let gain = distance as f64 / fade_frames as f64;
        for sample in values {
            *sample = (f64::from(*sample) * gain).round() as i16;
        }
    }
}

/// Remove DC and subsonic capture drift independently in each channel.
/// Initialize from the first frame to avoid creating a filter startup impulse.
fn high_pass_recording(samples: &mut [i16], sample_rate: u32, channels: u16) {
    let channels = usize::from(channels);
    if channels == 0 || sample_rate == 0 || samples.len() < channels {
        return;
    }
    let alpha = (-std::f64::consts::TAU * 10.0 / f64::from(sample_rate)).exp();
    let mut previous_input: Vec<f64> = samples[..channels].iter().map(|&s| f64::from(s)).collect();
    let mut previous_output = vec![0.0; channels];
    for frame in samples.chunks_exact_mut(channels) {
        for (channel, sample) in frame.iter_mut().enumerate() {
            let input = f64::from(*sample);
            let output = alpha * (previous_output[channel] + input - previous_input[channel]);
            previous_input[channel] = input;
            previous_output[channel] = output;
            *sample = output
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
        }
    }
}

fn encode_recording(samples: &mut [i16], spec: hound::WavSpec) -> Result<String, String> {
    high_pass_recording(samples, spec.sample_rate, spec.channels);
    fade_recording(samples, spec.sample_rate, spec.channels);
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut output, spec).map_err(|e| e.to_string())?;
        for &sample in samples.iter() {
            writer.write_sample(sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(STANDARD.encode(output.into_inner()))
}

fn import_wav(bytes: Vec<u8>) -> Result<String, String> {
    let mut reader =
        hound::WavReader::new(Cursor::new(&bytes)).map_err(|e| format!("Invalid WAV: {e}"))?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err("Invalid WAV format".into());
    }
    if spec.sample_format == hound::SampleFormat::Float {
        for sample in reader.samples::<f32>() {
            sample.map_err(|e| e.to_string())?;
        }
    } else {
        for sample in reader.samples::<i32>() {
            sample.map_err(|e| e.to_string())?;
        }
    }
    Ok(STANDARD.encode(bytes))
}

fn time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

impl Recording {
    fn snapshot(&self) -> CaptureSnapshot {
        CaptureSnapshot {
            callbacks: self.stats.callbacks.load(Ordering::Relaxed),
            samples: self.stats.samples.load(Ordering::Relaxed),
            peak: self.stats.peak.load(Ordering::Relaxed),
            seconds: self.started.elapsed().as_secs_f64(),
            rate: self.spec.sample_rate,
            channels: self.spec.channels,
        }
    }
}

impl CaptureSnapshot {
    fn summary(&self) -> String {
        format!(
            "Callbacks: {}, samples: {}, peak: {:.1}%, elapsed: {:.1}s, format: {} Hz / {} channel(s)",
            self.callbacks,
            self.samples,
            self.peak as f64 * 100.0 / 32768.0,
            self.seconds,
            self.rate,
            self.channels
        )
    }
}

impl Editor {
    pub(crate) fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    #[cfg(test)]
    pub(crate) fn for_test(target: (usize, usize), original: crate::Entry) -> Self {
        let duration = original
            .audio
            .as_deref()
            .and_then(|clip| clip_duration(clip).ok())
            .unwrap_or(0.0);
        Self {
            target,
            clip: original.audio.clone(),
            original,
            systems: Vec::new(),
            selected_system: 0,
            inputs: Vec::new(),
            selected: 0,
            recording: None,
            error: None,
            playback: None,
            position: 0.0,
            duration,
            diagnostics: Vec::new(),
            last_capture: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn clip_for_test(&self) -> Option<&str> {
        self.clip.as_deref()
    }

    pub fn new(target: (usize, usize), original: crate::Entry) -> Self {
        let (systems, selected_system) = crate::audio_devices::systems();
        let mut error = None;
        let inputs = match systems
            .get(selected_system)
            .copied()
            .map(crate::audio_devices::inputs)
        {
            Some(Ok(inputs)) => inputs,
            Some(Err(e)) => {
                error = Some(e);
                Vec::new()
            }
            None => {
                error = Some("No audio systems are available.".into());
                Vec::new()
            }
        };
        let selected = 0;
        let mut diagnostics = vec![
            format!(
                "Audio editor opened; CPAL host: {:?}; OS: {}",
                systems.get(selected_system),
                std::env::consts::OS
            ),
            format!("Available audio systems: {systems:?}"),
            format!("Input devices found: {}", inputs.len()),
        ];
        for (index, input) in inputs.iter().enumerate() {
            diagnostics.push(format!("  {index}: {} ({:?})", input.label, input.id));
        }
        if let Some(error) = &error {
            diagnostics.push(error.clone());
        }
        for line in &diagnostics {
            eprintln!("[audio] {line}");
        }
        let duration = original
            .audio
            .as_deref()
            .and_then(|clip| clip_duration(clip).ok())
            .unwrap_or(0.0);
        Self {
            target,
            clip: original.audio.clone(),
            original,
            systems,
            selected_system,
            inputs,
            selected,
            recording: None,
            error,
            playback: None,
            position: 0.0,
            duration,
            diagnostics,
            last_capture: None,
        }
    }

    fn refresh_inputs(&mut self) {
        self.selected = 0;
        let Some(system) = self.systems.get(self.selected_system).copied() else {
            self.inputs.clear();
            self.error = Some("No audio systems are available.".into());
            return;
        };
        match crate::audio_devices::inputs(system) {
            Ok(inputs) => {
                self.inputs = inputs;
                self.error = None;
                self.log(format!("{system}: {} inputs found", self.inputs.len()));
                for input in self.inputs.clone() {
                    self.log(format!("  {} ({:?})", input.label, input.id));
                }
            }
            Err(error) => {
                self.inputs.clear();
                self.log(error.clone());
                self.error = Some(error);
            }
        }
    }

    fn log(&mut self, message: impl Into<String>) {
        let message = message.into();
        eprintln!("[audio] {message}");
        self.diagnostics.push(message);
        if self.diagnostics.len() > 200 {
            self.diagnostics.remove(0);
        }
    }

    fn start(&mut self) -> Result<(), String> {
        self.playback = None;
        self.last_capture = None;
        self.log(format!(
            "Starting recording using input {}: {}",
            self.selected,
            self.inputs
                .get(self.selected)
                .map(|input| input.label.as_str())
                .unwrap_or("unavailable")
        ));
        let system = *self
            .systems
            .get(self.selected_system)
            .ok_or("No audio system available")?;
        let input = self
            .inputs
            .get(self.selected)
            .ok_or("No input device available")?;
        let (_host, device) = crate::audio_devices::open(system, input)?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("Could not configure input: {e}"))?;
        let config_description = format!("Default input configuration: {config:?}");
        eprintln!("[audio] {config_description}");
        self.diagnostics.push(config_description);
        let spec = hound::WavSpec {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples = Arc::new(Mutex::new(Vec::new()));
        let error = Arc::new(Mutex::new(None));
        let stats = Arc::new(CaptureStats::default());
        let errors = error.clone();
        let on_error = move |e| {
            if let Ok(mut error) = errors.lock() {
                *error = Some(format!("Recording failed: {e}"));
            }
        };
        let max_samples = spec.sample_rate as usize * spec.channels as usize * 600;
        macro_rules! build {
            ($type:ty, $convert:expr) => {{
                let samples = samples.clone();
                let errors = error.clone();
                let stats = stats.clone();
                device.build_input_stream(
                    config.clone().into(),
                    move |data: &[$type], _| {
                        stats.callbacks.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut samples) = samples.lock() {
                            if samples.len().saturating_add(data.len()) > max_samples {
                                if let Ok(mut error) = errors.lock() {
                                    *error = Some("Recording exceeded the 10 minute limit.".into());
                                }
                                return;
                            }
                            let mut peak = 0usize;
                            samples.extend(data.iter().map(|sample| {
                                let value: i16 = ($convert)(sample);
                                peak = peak.max(value.unsigned_abs() as usize);
                                value
                            }));
                            stats.samples.fetch_add(data.len(), Ordering::Relaxed);
                            stats.peak.fetch_max(peak, Ordering::Relaxed);
                        } else if let Ok(mut error) = errors.lock() {
                            *error = Some("Recording sample buffer lock was poisoned.".into());
                        }
                    },
                    on_error,
                    None,
                )
            }};
        }
        let stream = match config.sample_format() {
            cpal::SampleFormat::I8 => build!(i8, |&s: &i8| (s as i16) << 8),
            cpal::SampleFormat::I16 => build!(i16, |&s: &i16| s),
            cpal::SampleFormat::I24 => build!(cpal::I24, |&s: &cpal::I24| (s.inner() >> 8) as i16),
            cpal::SampleFormat::I32 => build!(i32, |&s: &i32| (s >> 16) as i16),
            cpal::SampleFormat::I64 => build!(i64, |&s: &i64| (s >> 48) as i16),
            cpal::SampleFormat::U8 => build!(u8, |&s: &u8| ((s as i16) - 128) << 8),
            cpal::SampleFormat::U16 => build!(u16, |&s: &u16| (s as i32 - 32768) as i16),
            cpal::SampleFormat::U24 => {
                build!(cpal::U24, |&s: &cpal::U24| ((s.inner() >> 8) - 32768)
                    as i16)
            }
            cpal::SampleFormat::U32 => build!(u32, |&s: &u32| ((s >> 16) as i32 - 32768) as i16),
            cpal::SampleFormat::U64 => build!(u64, |&s: &u64| ((s >> 48) as i32 - 32768) as i16),
            cpal::SampleFormat::F32 => {
                build!(f32, |&s: &f32| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            }
            cpal::SampleFormat::F64 => {
                build!(f64, |&s: &f64| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            }
            format => return Err(format!("Unsupported input sample format: {format:?}")),
        }
        .map_err(|e| format!("Could not open input: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("Could not start recording: {e}"))?;
        self.recording = Some(Recording {
            stream,
            samples,
            error,
            spec,
            stats,
            started: Instant::now(),
        });
        self.log("Input stream created and started.");
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        let Some(recording) = self.recording.take() else {
            return Ok(());
        };
        self.last_capture = Some(recording.snapshot());
        self.log(format!(
            "Recording stopped. {}",
            self.last_capture.as_ref().unwrap().summary()
        ));
        drop(recording.stream);
        if let Some(error) = recording.error.lock().map_err(|e| e.to_string())?.take() {
            return Err(error);
        }
        let mut samples = recording.samples.lock().map_err(|e| e.to_string())?;
        if samples.is_empty() {
            return Err("No audio was recorded.".into());
        }
        self.clip = Some(encode_recording(&mut samples, recording.spec)?);
        self.duration = samples.len() as f64
            / recording.spec.channels as f64
            / recording.spec.sample_rate as f64;
        self.position = 0.0;
        self.log("Recorded WAV encoded successfully.");
        Ok(())
    }

    fn play(&mut self) -> Result<(), String> {
        self.playback = None;
        let data = STANDARD
            .decode(self.clip.as_ref().ok_or("No clip attached")?)
            .map_err(|e| format!("Invalid audio data: {e}"))?;
        let source = wav_decoder(data)?;
        let mut stream = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("Could not open output device: {e}"))?;
        stream.log_on_drop(false);
        let sink = rodio::Player::connect_new(stream.mixer());
        sink.append(source);
        if self.position > 0.0 {
            sink.try_seek(std::time::Duration::from_secs_f64(self.position))
                .map_err(|e| format!("Could not seek audio: {e}"))?;
        }
        self.playback = Some((stream, sink));
        Ok(())
    }

    pub fn sync_entry(&mut self, entry: &crate::Entry) {
        if self.original.audio != entry.audio {
            self.playback = None;
            self.clip = entry.audio.clone();
            self.duration = self
                .clip
                .as_deref()
                .and_then(|clip| clip_duration(clip).ok())
                .unwrap_or(0.0);
            self.position = 0.0;
        }
        self.original = entry.clone();
    }

    pub fn block_history_while_recording(&mut self) -> bool {
        if self.recording.is_some() {
            self.error = Some("Stop recording before undo or redo.".into());
            true
        } else {
            false
        }
    }

    fn seek(&mut self, position: f64) {
        if self.recording.is_some() || self.clip.is_none() {
            return;
        }
        let position = position.clamp(0.0, self.duration);
        if let Some((_, player)) = &self.playback {
            if let Err(error) = player.try_seek(std::time::Duration::from_secs_f64(position)) {
                self.error = Some(format!("Could not seek audio: {error}"));
                return;
            }
        }
        self.position = position;
    }

    fn handle_action(
        &mut self,
        action: crate::keybinds::Action,
        changed: &mut Option<Option<String>>,
        project_actions: &mut Vec<crate::keybinds::Action>,
    ) {
        use crate::keybinds::Action;
        match action {
            Action::Undo | Action::Redo | Action::RedoAlternative => {
                if self.recording.is_some() {
                    self.error = Some("Stop recording before undo or redo.".into());
                } else {
                    project_actions.push(action);
                }
            }
            Action::SaveProject | Action::SaveProjectAs => {
                if self.recording.is_some() {
                    match self.stop() {
                        Ok(()) => *changed = Some(self.clip.clone()),
                        Err(error) => {
                            self.error = Some(format!("Could not finish recording: {error}"));
                            return;
                        }
                    }
                }
                project_actions.push(action);
            }
            Action::RecorderRecord => {
                if self.recording.is_some() {
                    match self.stop() {
                        Ok(()) => {
                            *changed = Some(self.clip.clone());
                            self.error = None;
                        }
                        Err(error) => self.error = Some(format!("Recording error: {error}")),
                    }
                } else if !self.inputs.is_empty() {
                    self.error = self.start().err();
                }
            }
            Action::RecorderPlayPause if self.recording.is_none() && self.clip.is_some() => {
                if let Some((_, player)) = &self.playback {
                    if player.is_paused() {
                        player.play();
                    } else {
                        player.pause();
                    }
                } else {
                    self.error = self.play().err();
                }
            }
            Action::RecorderStopPlayback if self.recording.is_none() => {
                self.playback = None;
                self.position = 0.0;
            }
            Action::RecorderSeekBack => self.seek(self.position - 5.0),
            Action::RecorderSeekForward => self.seek(self.position + 5.0),
            Action::RecorderSeekStart => self.seek(0.0),
            Action::RecorderSeekEnd => self.seek(self.duration),
            _ => {}
        }
    }

    fn update_audio(&mut self, ctx: &egui::Context) {
        if let Some((_, player)) = &self.playback {
            if player.empty() {
                self.playback = None;
                self.position = 0.0;
                self.log("Playback finished.");
            } else {
                self.position = player.get_pos().as_secs_f64().min(self.duration);
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        if let Some(recording) = &self.recording {
            let failure = recording.error.lock().ok().and_then(|error| error.clone());
            if let Some(error) = failure {
                self.last_capture = Some(recording.snapshot());
                let summary = self.last_capture.as_ref().unwrap().summary();
                self.recording = None;
                self.log(format!("Stream error: {error}. {summary}"));
                self.error = Some(error);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        l1_name: &str,
        keybinds: &crate::keybinds::Registry,
    ) -> Outcome {
        let mut cancel = false;
        let mut result = None;
        let mut project_actions = Vec::new();
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("entry-audio"),
            egui::ViewportBuilder::default()
                .with_title("Entry audio")
                .with_inner_size([780.0, 380.0])
                .with_min_inner_size([340.0, 300.0]),
            |ui, _class| {
                if ui.input(|input| input.viewport().close_requested()) {
                    cancel = true;
                    return;
                }
                self.update_audio(ui.ctx());
                let popup_open = egui::Popup::is_any_open(ui.ctx());
                let actions = ui.input_mut(|input| if popup_open { keybinds.take_recorder_project_actions(input) } else { keybinds.take_recorder_actions(input) });
                for action in actions { self.handle_action(action, &mut result, &mut project_actions); }
                ui.visuals_mut().panel_fill = egui::Color32::BLACK;
                ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                egui::Frame::new().fill(egui::Color32::BLACK).inner_margin(20.0).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("audio_window_content").show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(self.recording.is_none(), |ui| {
                    ui.horizontal(|ui| {
                    let previous = self.selected_system;
                    egui::ComboBox::from_id_salt("audio_system")
                        .selected_text(self.systems.get(self.selected_system).map(|system| system.name()).unwrap_or("No audio systems"))
                        .show_ui(ui, |ui| {
                            for (index, system) in self.systems.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_system, index, system.name());
                            }
                        });
                    if self.selected_system != previous { self.refresh_inputs(); }
                    egui::ComboBox::from_id_salt("audio_input")
                        .selected_text(
                            self.inputs
                                .get(self.selected)
                                .map(|input| input.label.as_str())
                                .unwrap_or("No input devices"),
                        )
                        .show_ui(ui, |ui| {
                            for (index, input) in self.inputs.iter().enumerate() {
                                ui.selectable_value(&mut self.selected, index, &input.label);
                            }
                        });
                    if ui.button("Refresh devices").clicked() { self.refresh_inputs(); }
                    });
                    #[cfg(target_os = "linux")]
                    if self.systems.get(self.selected_system) == Some(&cpal::HostId::Jack) {
                        ui.label("JACK uses its system capture ports; configure routing in your JACK tools.");
                    }
                });
                });
                ui.horizontal(|ui| {
                    if ui.button("Save project").clicked() { self.handle_action(crate::keybinds::Action::SaveProject, &mut result, &mut project_actions); }
                    if ui.button("Undo").clicked() { self.handle_action(crate::keybinds::Action::Undo, &mut result, &mut project_actions); }
                    if ui.button("Redo").clicked() { self.handle_action(crate::keybinds::Action::Redo, &mut result, &mut project_actions); }
                    if self.recording.is_none() {
                        if ui.button("Import WAV…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("WAV audio", &["wav"])
                                .pick_file()
                            {
                                match std::fs::metadata(&path)
                                    .map_err(|e| e.to_string())
                                    .and_then(|metadata| {
                                        if metadata.len() > 100 * 1024 * 1024 {
                                            return Err(
                                                "WAV file exceeds the 100 MiB import limit".into(),
                                            );
                                        }
                                        std::fs::read(&path).map_err(|e| e.to_string())
                                    })
                                    .and_then(import_wav) {
                                    Ok(clip) => {
                                        self.playback = None;
                                        self.clip = Some(clip);
                                        self.duration = self.clip.as_deref().and_then(|clip| clip_duration(clip).ok()).unwrap_or(0.0);
                                        self.position = 0.0;
                                        result = Some(self.clip.clone());
                                        self.error = None;
                                        self.log("WAV import succeeded.");
                                    }
                                    Err(e) => {
                                        self.log(format!("WAV import failed: {e}"));
                                        self.error = Some(format!("Could not import audio: {e}"))
                                    }
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.clip.is_some(), egui::Button::new("Delete clip")).clicked() {
                                self.playback = None;
                                self.clip = None;
                                self.position = 0.0;
                                self.duration = 0.0;
                                result = Some(None);
                            }
                        });
                    }
                });
                ui.add_space(18.0);
                let recording_time = self.recording.as_ref().map(|recording| recording.started.elapsed().as_secs_f64());
                let position = recording_time.unwrap_or(self.position);
                ui.horizontal(|ui| {
                    let fraction = if self.duration > 0.0 { (position / self.duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
                    ui.add_space((ui.available_width() - 65.0).max(0.0) * fraction);
                    ui.label(egui::RichText::new(time_label(position)).size(22.0).strong());
                });
                let mut seek = self.position;
                let response = ui.add_enabled_ui(self.recording.is_none() && self.clip.is_some() && self.duration > 0.0, |ui| {
                    ui.spacing_mut().slider_width = ui.available_width();
                    ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::RED;
                    ui.visuals_mut().widgets.active.bg_fill = egui::Color32::RED;
                    ui.visuals_mut().widgets.hovered.bg_fill = egui::Color32::RED;
                    ui.add(egui::Slider::new(&mut seek, 0.0..=self.duration.max(0.001)).show_value(false))
                }).inner;
                if response.changed() {
                    self.seek(seek);
                }
                ui.horizontal(|ui| {
                    ui.label(time_label(0.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(time_label(recording_time.unwrap_or(self.duration)));
                    });
                });
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() { cancel = true; }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let playing = self.playback.as_ref().is_some_and(|(_, player)| !player.is_paused());
                        let play_text = if playing { "❚❚" } else { "▶" };
                        if ui.add_enabled(self.clip.is_some() && self.recording.is_none(), egui::Button::new(egui::RichText::new(play_text).size(24.0))).on_hover_text(if playing { "Pause" } else { "Play" }).clicked() {
                            self.handle_action(crate::keybinds::Action::RecorderPlayPause, &mut result, &mut project_actions);
                        }
                        if ui.add_enabled(self.clip.is_some() && self.recording.is_none(), egui::Button::new(egui::RichText::new("|◀").size(24.0))).on_hover_text("Back to start").clicked() {
                            self.handle_action(crate::keybinds::Action::RecorderSeekStart, &mut result, &mut project_actions);
                        }
                        let recording = self.recording.is_some();
                        let symbol = if recording { "■" } else { "●" };
                        if ui.add_enabled(recording || !self.inputs.is_empty(), egui::Button::new(egui::RichText::new(symbol).size(27.0).color(egui::Color32::RED))).on_hover_text(if recording { "Stop recording" } else { "Record" }).clicked() {
                            self.handle_action(crate::keybinds::Action::RecorderRecord, &mut result, &mut project_actions);
                            if let Some(error) = self.error.clone() { self.log(format!("Recording error: {error}")); }
                        }
                    });
                });
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                egui::CollapsingHeader::new("Audio diagnostics")
                    .default_open(false)
                    .show(ui, |ui| {
                        let capture = self.recording.as_ref().map(Recording::snapshot);
                        if let Some(snapshot) = capture.as_ref().or(self.last_capture.as_ref()) {
                            ui.label(snapshot.summary());
                            if self.recording.is_some()
                                && snapshot.callbacks == 0
                                && snapshot.seconds > 2.0
                            {
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "The stream started but has not received any audio callbacks.",
                                );
                            } else if snapshot.samples > 0 && snapshot.peak == 0 {
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "Audio samples are arriving, but they are silent.",
                                );
                            }
                        }
                        ui.label("Diagnostics include device names, but not audio or entry text.");
                        if ui.button("Copy diagnostics").clicked() {
                            let mut report = self.diagnostics.join("\n");
                            if let Some(snapshot) = capture.as_ref().or(self.last_capture.as_ref())
                            {
                                report.push_str(&format!(
                                    "\nCurrent/last capture: {}",
                                    snapshot.summary()
                                ));
                            }
                            ui.ctx().copy_text(report);
                        }
                        egui::ScrollArea::vertical()
                            .id_salt("audio_diagnostics")
                            .max_height(150.0)
                            .show(ui, |ui| {
                                for line in &self.diagnostics {
                                    ui.label(line);
                                }
                            });
                    });
                ui.add_space(16.0);
                show_entry_context(ui, &self.original, l1_name);
                });
                });
            },
        );
        if cancel {
            if self.recording.is_some() {
                match self.stop() {
                    Ok(()) => result = Some(self.clip.clone()),
                    Err(error) => {
                        self.log(format!("Stop failed: {error}"));
                        self.error = Some(error);
                        ctx.send_viewport_cmd_to(
                            egui::ViewportId::from_hash_of("entry-audio"),
                            egui::ViewportCommand::CancelClose,
                        );
                        cancel = false;
                    }
                }
            }
        }
        if result.is_some() || cancel || !project_actions.is_empty() {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        Outcome {
            closed: cancel,
            changed: result,
            actions: project_actions,
        }
    }
}

fn show_entry_context(ui: &mut egui::Ui, entry: &crate::Entry, l1_name: &str) {
    let (background, accent, language) = match entry.language {
        crate::Language::L1 => (crate::L1_COLOR, crate::L1_ACCENT, l1_name),
        crate::Language::Latin => (crate::LATIN_COLOR, crate::LATIN_ACCENT, "Latin"),
    };
    let participant = match entry.participant {
        crate::Participant::Teacher => "Teacher",
        crate::Participant::Student => "Student",
    };
    egui::Frame::new()
        .fill(background)
        .stroke(egui::Stroke::new(1.0, egui::Color32::DARK_GRAY))
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 20.0).max(0.0));
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(participant)
                        .size(12.0)
                        .color(accent)
                        .strong(),
                );
                ui.label(egui::RichText::new("·").size(12.0).color(accent));
                ui.label(
                    egui::RichText::new(language)
                        .font(crate::entry_font(entry.language, 12.0))
                        .color(accent)
                        .strong(),
                );
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&entry.text)
                    .font(crate::entry_font(entry.language, 15.0))
                    .color(egui::Color32::WHITE),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recordings_are_faded_per_frame_and_imports_are_unchanged() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let original: Vec<i16> = (0..8).flat_map(|_| [12000, -12000]).collect();
        let mut input = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut input, spec).unwrap();
            for &sample in &original {
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        let imported_bytes = input.into_inner();
        assert_eq!(
            STANDARD
                .decode(import_wav(imported_bytes.clone()).unwrap())
                .unwrap(),
            imported_bytes
        );

        let mut expected = original.clone();
        high_pass_recording(&mut expected, spec.sample_rate, spec.channels);
        fade_recording(&mut expected, spec.sample_rate, spec.channels);
        let encoded = encode_recording(&mut original.clone(), spec).unwrap();
        let data = STANDARD.decode(encoded).unwrap();
        let reader = hound::WavReader::new(Cursor::new(data)).unwrap();
        let samples = reader
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, expected);
    }

    #[test]
    fn high_pass_removes_dc_and_preserves_speech_frequencies_per_channel() {
        let rate = 48_000;
        let mut samples: Vec<i16> = (0..rate)
            .flat_map(|frame| {
                let speech = (f64::from(frame) * std::f64::consts::TAU * 1000.0 / f64::from(rate))
                    .sin()
                    * 8000.0;
                [(12000.0 + speech) as i16, -15000]
            })
            .collect();
        high_pass_recording(&mut samples, rate, 2);
        let left: Vec<f64> = samples[rate as usize..]
            .chunks_exact(2)
            .map(|frame| f64::from(frame[0]))
            .collect();
        let right = samples[rate as usize..]
            .chunks_exact(2)
            .map(|frame| frame[1]);
        assert!(left.iter().sum::<f64>().abs() / (left.len() as f64) < 2.0);
        assert!(left.iter().map(|v| v.abs()).fold(0.0, f64::max) > 7900.0);
        assert!(right.into_iter().all(|sample| sample == 0));
    }

    #[test]
    fn high_pass_attenuates_subsonic_drift_and_preserves_length() {
        let rate = 1000;
        let mut samples: Vec<i16> = (0..rate * 3)
            .map(|frame| {
                (10000.0 * (f64::from(frame) * std::f64::consts::TAU / f64::from(rate)).sin())
                    as i16
            })
            .collect();
        let len = samples.len();
        high_pass_recording(&mut samples, rate, 1);
        assert_eq!(samples.len(), len);
        assert!(
            samples[rate as usize..]
                .iter()
                .map(|&s| i32::from(s).abs())
                .max()
                .unwrap()
                < 1100
        );
        high_pass_recording(&mut [], rate, 1);
        high_pass_recording(&mut [1], 0, 1);
    }

    #[test]
    fn short_recordings_have_overlapping_fades() {
        let mut mono = [10000, 10000, 10000];
        fade_recording(&mut mono, 8, 1);
        assert_eq!(mono, [0, 5000, 0]);
        let mut one_frame = [-32768, 32767];
        fade_recording(&mut one_frame, 48000, 2);
        assert_eq!(one_frame, [0, 0]);
        fade_recording(&mut [], 48000, 2);
    }

    #[test]
    fn fade_reaches_full_volume_after_quarter_second() {
        let mut samples = vec![12000; 48000];
        fade_recording(&mut samples, 48000, 1);
        assert_eq!(samples[0], 0);
        assert_eq!(samples[6000], 6000);
        assert_eq!(samples[12000], 12000);
        assert_eq!(samples[35999], 12000);
        assert_eq!(samples[41999], 6000);
        assert_eq!(samples[47999], 0);
    }

    #[test]
    fn recorder_shortcuts_seek_and_dispatch_project_actions() {
        use crate::keybinds::Action;
        let entry = crate::Entry {
            participant: crate::Participant::Teacher,
            language: crate::Language::Latin,
            text: "salvē".into(),
            audio: Some("clip".into()),
        };
        let mut editor = Editor::for_test((0, 0), entry);
        editor.duration = 12.0;
        let mut changed = None;
        let mut actions = Vec::new();
        editor.handle_action(Action::RecorderSeekForward, &mut changed, &mut actions);
        assert_eq!(editor.position, 5.0);
        editor.handle_action(Action::RecorderSeekEnd, &mut changed, &mut actions);
        assert_eq!(editor.position, 12.0);
        editor.handle_action(Action::RecorderSeekForward, &mut changed, &mut actions);
        assert_eq!(editor.position, 12.0);
        editor.handle_action(Action::RecorderSeekBack, &mut changed, &mut actions);
        assert_eq!(editor.position, 7.0);
        editor.handle_action(Action::RecorderSeekStart, &mut changed, &mut actions);
        assert_eq!(editor.position, 0.0);
        editor.handle_action(Action::Undo, &mut changed, &mut actions);
        editor.handle_action(Action::SaveProject, &mut changed, &mut actions);
        assert_eq!(actions, vec![Action::Undo, Action::SaveProject]);
        assert!(changed.is_none());
    }

    #[test]
    fn recorder_viewport_dispatches_save_and_undo() {
        let entry = crate::Entry {
            participant: crate::Participant::Teacher,
            language: crate::Language::Latin,
            text: "salvē".into(),
            audio: None,
        };
        let mut editor = Editor::for_test((0, 0), entry);
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let input = egui::RawInput {
            events: [egui::Key::Z, egui::Key::S]
                .into_iter()
                .map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                })
                .collect(),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            actions = editor
                .show(ui.ctx(), "English", &crate::keybinds::Registry::default())
                .actions;
        });
        output.textures_delta.clear();
        assert_eq!(
            actions,
            vec![
                crate::keybinds::Action::Undo,
                crate::keybinds::Action::SaveProject
            ]
        );
    }

    #[test]
    fn capture_summary_reports_counts_format_and_level() {
        let snapshot = CaptureSnapshot {
            callbacks: 12,
            samples: 48000,
            peak: 16384,
            seconds: 1.25,
            rate: 48000,
            channels: 1,
        };
        let summary = snapshot.summary();
        assert!(summary.contains("Callbacks: 12"));
        assert!(summary.contains("samples: 48000"));
        assert!(summary.contains("peak: 50.0%"));
        assert!(summary.contains("48000 Hz / 1 channel(s)"));
    }

    #[test]
    fn duration_counts_frames_not_interleaved_samples() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut wav, spec).unwrap();
            for _ in 0..32 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        assert_eq!(
            clip_duration(&STANDARD.encode(wav.into_inner())).unwrap(),
            2.0
        );
        assert_eq!(time_label(83.9), "01:23");
        assert!(clip_duration("not base64!").is_err());
    }

    #[test]
    fn recorded_wav_seeks_forward_and_backward_by_frame() {
        use rodio::Source;
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(
                &mut wav,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 100,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for frame in 0..200i16 {
                writer.write_sample(frame * 100).unwrap();
                writer.write_sample(-frame * 100).unwrap();
            }
            writer.finalize().unwrap();
        }
        let mut decoder = wav_decoder(wav.into_inner()).unwrap();
        decoder
            .try_seek(std::time::Duration::from_millis(1500))
            .unwrap();
        assert!((decoder.next().unwrap() - 15000.0 / 32768.0).abs() < 0.0001);
        assert!((decoder.next().unwrap() + 15000.0 / 32768.0).abs() < 0.0001);
        decoder
            .try_seek(std::time::Duration::from_millis(500))
            .unwrap();
        assert!((decoder.next().unwrap() - 5000.0 / 32768.0).abs() < 0.0001);
        decoder.try_seek(std::time::Duration::ZERO).unwrap();
        assert_eq!(decoder.next(), Some(0.0));
    }

    #[test]
    fn imported_float_and_24_bit_wav_seek() {
        use rodio::Source;
        for spec in [
            hound::WavSpec {
                channels: 1,
                sample_rate: 10,
                bits_per_sample: 24,
                sample_format: hound::SampleFormat::Int,
            },
            hound::WavSpec {
                channels: 1,
                sample_rate: 10,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        ] {
            let mut wav = Cursor::new(Vec::new());
            {
                let mut writer = hound::WavWriter::new(&mut wav, spec).unwrap();
                for frame in 0..20 {
                    if spec.sample_format == hound::SampleFormat::Float {
                        writer.write_sample(frame as f32 / 20.0).unwrap();
                    } else {
                        writer.write_sample(frame * 100_000).unwrap();
                    }
                }
                writer.finalize().unwrap();
            }
            let mut decoder = wav_decoder(wav.into_inner()).unwrap();
            decoder.try_seek(std::time::Duration::from_secs(1)).unwrap();
            let expected = if spec.sample_format == hound::SampleFormat::Float {
                0.5
            } else {
                1_000_000.0 / 8_388_608.0
            };
            assert!((decoder.next().unwrap() - expected).abs() < 0.0001);
            decoder
                .try_seek(std::time::Duration::from_millis(200))
                .unwrap();
            assert!(decoder.next().is_some());
        }
        assert!(wav_decoder(b"not a WAV".to_vec()).is_err());
    }
}
