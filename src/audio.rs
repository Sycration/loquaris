//! Entry audio is stored as WAV bytes, both in memory and in the project file.
use cpal::traits::{DeviceTrait, StreamTrait};
use eframe::egui;
use rodio::Source as _;
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
    clip: Option<Arc<[u8]>>,
    /// Messages shown in the recorder window.
    pub toasts: crate::toasts::Toasts,
    playback: Option<(rodio::MixerDeviceSink, rodio::Player)>,
    position: f64,
    duration: f64,
    trim: Option<crate::Trim>,
    frames: u32,
    rate: u32,
    peaks: Option<crate::waveform::Peaks>,
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
    peaks: crate::waveform::Peaks,
    /// How many captured samples `peaks` already covers.
    peaked_samples: usize,
}

pub struct Outcome {
    pub closed: bool,
    pub changed: Option<crate::AudioState>,
    pub actions: Vec<crate::keybinds::Action>,
}

pub(crate) fn clip_duration(clip: &[u8]) -> Result<f64, String> {
    let reader = hound::WavReader::new(Cursor::new(clip)).map_err(|e| e.to_string())?;
    let rate = reader.spec().sample_rate;
    if rate == 0 || reader.spec().channels == 0 {
        return Err("Invalid WAV format".into());
    }
    Ok(reader.duration() as f64 / rate as f64)
}

pub(crate) fn wav_decoder<T>(data: T) -> Result<rodio::Decoder<Cursor<T>>, String>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    let header = hound::WavReader::new(Cursor::new(data.as_ref()))
        .map_err(|e| format!("Could not decode audio: {e}"))?;
    if header.spec().channels == 0 || header.spec().sample_rate == 0 {
        return Err("Could not decode audio: invalid WAV channel count or sample rate.".into());
    }
    rodio::Decoder::try_from(Cursor::new(data)).map_err(|e| format!("Could not decode audio: {e}"))
}

/// The frames of a clip inside a crop. A crop that does not fit the clip is
/// clamped to it. A non-empty clip always has at least one selected frame.
pub(crate) fn selected_frames(trim: Option<crate::Trim>, total: u32) -> std::ops::Range<u32> {
    let Some(trim) = trim else {
        return 0..total;
    };
    let start = trim.start.min(total.saturating_sub(1));
    start..trim.end.max(start.saturating_add(1)).min(total)
}

pub(crate) fn selected_duration(clip: &[u8], trim: Option<crate::Trim>) -> Result<f64, String> {
    let reader = hound::WavReader::new(Cursor::new(clip)).map_err(|e| e.to_string())?;
    let rate = reader.spec().sample_rate;
    if rate == 0 || reader.spec().channels == 0 {
        return Err("Invalid WAV format".into());
    }
    let selected = selected_frames(trim, reader.duration());
    Ok(f64::from(selected.end - selected.start) / f64::from(rate))
}

/// Gain at `frame` that keeps a crop from clicking: a 10 ms ramp on each edge
/// that cuts into the clip. Edges at the clip's own ends are left untouched.
pub(crate) fn cut_gain(frame: u32, selected: &std::ops::Range<u32>, total: u32, rate: u32) -> f32 {
    let fade = (rate / 100).max(1);
    let mut distance = fade;
    if selected.start > 0 {
        distance = distance.min(frame.saturating_sub(selected.start));
    }
    if selected.end < total {
        distance = distance.min(selected.end.saturating_sub(frame + 1));
    }
    distance as f32 / fade as f32
}

/// A clip limited to its crop. Seek positions count from the start of the
/// whole clip, and are held inside the selection.
pub(crate) struct ClipSource<T>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    decoder: rodio::Decoder<Cursor<T>>,
    selected: std::ops::Range<u32>,
    total: u32,
    rate: u32,
    channels: u16,
    frame: u32,
    channel: u16,
}

pub(crate) fn clip_source<T>(clip: T, trim: Option<crate::Trim>) -> Result<ClipSource<T>, String>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    let (total, rate, channels) = {
        let reader = hound::WavReader::new(Cursor::new(clip.as_ref()))
            .map_err(|e| format!("Could not decode audio: {e}"))?;
        (
            reader.duration(),
            reader.spec().sample_rate,
            reader.spec().channels,
        )
    };
    let decoder = wav_decoder(clip)?;
    let selected = selected_frames(trim, total);
    let start = selected.start;
    let mut source = ClipSource {
        decoder,
        selected,
        total,
        rate,
        channels,
        frame: 0,
        channel: 0,
    };
    if start > 0 {
        source
            .seek_to_frame(start)
            .map_err(|e| format!("Could not seek audio: {e}"))?;
    }
    Ok(source)
}

impl<T> ClipSource<T>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    fn seek_to_frame(&mut self, frame: u32) -> Result<(), rodio::source::SeekError> {
        let frame = frame.clamp(self.selected.start, self.selected.end);
        self.decoder.try_seek(std::time::Duration::from_secs_f64(
            f64::from(frame) / f64::from(self.rate),
        ))?;
        self.frame = frame;
        self.channel = 0;
        Ok(())
    }
}

impl<T> Iterator for ClipSource<T>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.frame >= self.selected.end {
            return None;
        }
        let sample = self.decoder.next()?;
        let gain = cut_gain(self.frame, &self.selected, self.total, self.rate);
        self.channel += 1;
        if self.channel == self.channels {
            self.channel = 0;
            self.frame += 1;
        }
        Some(sample * gain)
    }
}

impl<T> rodio::Source for ClipSource<T>
where
    T: AsRef<[u8]> + Send + Sync + 'static,
{
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.decoder.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.decoder.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs_f64(
            f64::from(self.selected.end - self.selected.start) / f64::from(self.rate),
        ))
    }

    fn try_seek(&mut self, position: std::time::Duration) -> Result<(), rodio::source::SeekError> {
        let frame = (position.as_secs_f64() * f64::from(self.rate)).round() as u32;
        self.seek_to_frame(frame)
    }
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

fn encode_recording(samples: &mut [i16], spec: hound::WavSpec) -> Result<Arc<[u8]>, String> {
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
    Ok(output.into_inner().into())
}

fn import_wav(bytes: Vec<u8>) -> Result<Arc<[u8]>, String> {
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
    Ok(bytes.into())
}

fn time_label(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{:02}:{:02}.{:02}",
        hundredths / 6000,
        hundredths / 100 % 60,
        hundredths % 100
    )
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
        let (clip, trim) = (original.audio.clone(), original.trim);
        let mut editor = Self {
            target,
            clip: None,
            original,
            systems: Vec::new(),
            selected_system: 0,
            inputs: Vec::new(),
            selected: 0,
            recording: None,
            toasts: Default::default(),
            playback: None,
            position: 0.0,
            duration: 0.0,
            trim: None,
            frames: 0,
            rate: 0,
            peaks: None,
            diagnostics: Vec::new(),
            last_capture: None,
        };
        editor.set_clip(clip, trim);
        editor
    }

    #[cfg(test)]
    pub(crate) fn clip_for_test(&self) -> Option<&[u8]> {
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
        let (clip, trim) = (original.audio.clone(), original.trim);
        let mut editor = Self {
            target,
            clip: None,
            original,
            systems,
            selected_system,
            inputs,
            selected,
            recording: None,
            toasts: Default::default(),
            playback: None,
            position: 0.0,
            duration: 0.0,
            trim: None,
            frames: 0,
            rate: 0,
            peaks: None,
            diagnostics,
            last_capture: None,
        };
        editor.set_clip(clip, trim);
        if let Some(error) = error {
            editor.toasts.error(error);
        }
        editor
    }

    /// Replace the clip and everything derived from it.
    fn set_clip(&mut self, clip: Option<Arc<[u8]>>, trim: Option<crate::Trim>) {
        self.playback = None;
        let reader = clip
            .as_deref()
            .and_then(|clip| hound::WavReader::new(Cursor::new(clip)).ok());
        (self.frames, self.rate) = reader
            .map(|reader| (reader.duration(), reader.spec().sample_rate))
            .unwrap_or((0, 0));
        self.duration = clip
            .as_deref()
            .and_then(|clip| clip_duration(clip).ok())
            .unwrap_or(0.0);
        self.peaks = clip.as_deref().and_then(crate::waveform::Peaks::from_clip);
        self.clip = clip;
        self.trim = trim;
        self.position = self.selection().0;
    }

    fn audio_state(&self) -> crate::AudioState {
        (self.clip.clone(), self.trim)
    }

    /// The selected part of the clip, in seconds.
    fn selection(&self) -> (f64, f64) {
        if self.trim.is_none() || self.rate == 0 {
            return (0.0, self.duration);
        }
        let selected = selected_frames(self.trim, self.frames);
        let rate = f64::from(self.rate);
        (
            f64::from(selected.start) / rate,
            f64::from(selected.end) / rate,
        )
    }

    /// Move a crop handle to a fraction of the clip, keeping at least 50 ms
    /// selected. A selection of the whole clip is no crop at all.
    fn move_handle(&mut self, handle: crate::waveform::Handle, fraction: f32) {
        use crate::waveform::Handle;
        if self.frames == 0 {
            return;
        }
        let selected = selected_frames(self.trim, self.frames);
        let frame = (f64::from(fraction.clamp(0.0, 1.0)) * f64::from(self.frames)).round() as u32;
        let gap = (self.rate / 20).clamp(1, self.frames);
        let moved = match handle {
            Handle::Start => frame.min(selected.end.saturating_sub(gap))..selected.end,
            Handle::End => selected.start..frame.max(selected.start + gap).min(self.frames),
        };
        if moved == selected {
            return;
        }
        self.playback = None;
        self.trim = (moved != (0..self.frames)).then_some(crate::Trim {
            start: moved.start,
            end: moved.end,
        });
        let (start, end) = self.selection();
        self.position = self.position.clamp(start, end);
    }

    fn show_waveform(&mut self, ui: &mut egui::Ui, changed: &mut Option<crate::AudioState>) {
        // A short recording grows from the left until it has filled this many
        // seconds, then keeps the whole take in view.
        const RECORDING_WINDOW: f64 = 10.0;
        let view = if let Some(recording) = &self.recording {
            let seconds =
                recording.peaks.frames() as f64 / f64::from(recording.spec.sample_rate.max(1));
            let extent = (seconds / seconds.max(RECORDING_WINDOW)) as f32;
            crate::waveform::View {
                peaks: Some(&recording.peaks),
                extent,
                selection: None,
                playhead: Some((extent, time_label(seconds))),
            }
        } else {
            let selected = selected_frames(self.trim, self.frames);
            let total = self.frames.max(1) as f32;
            let (start, end) = self.selection();
            let usable = self.peaks.is_some() && self.frames > 0 && self.duration > 0.0;
            crate::waveform::View {
                peaks: self.peaks.as_ref(),
                extent: 1.0,
                selection: usable.then(|| crate::waveform::Selection {
                    start: selected.start as f32 / total,
                    end: selected.end as f32 / total,
                    start_time: time_label(start),
                    end_time: time_label(end),
                }),
                playhead: usable.then(|| {
                    (
                        (self.position / self.duration) as f32,
                        time_label(self.position),
                    )
                }),
            }
        };
        let interaction = crate::waveform::show(ui, view);
        if let Some((handle, fraction)) = interaction.moved {
            self.move_handle(handle, fraction);
        } else if let Some(fraction) = interaction.seek {
            self.seek(f64::from(fraction) * self.duration);
        }
        if interaction.released {
            *changed = Some(self.audio_state());
        }
    }

    fn refresh_inputs(&mut self) {
        self.selected = 0;
        let Some(system) = self.systems.get(self.selected_system).copied() else {
            self.inputs.clear();
            self.toasts.error("No audio systems are available.");
            return;
        };
        match crate::audio_devices::inputs(system) {
            Ok(inputs) => {
                self.inputs = inputs;
                self.log(format!("{system}: {} inputs found", self.inputs.len()));
                for input in self.inputs.clone() {
                    self.log(format!("  {} ({:?})", input.label, input.id));
                }
            }
            Err(error) => {
                self.inputs.clear();
                self.log(error.clone());
                self.toasts.error(error);
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
            peaks: crate::waveform::Peaks::new(spec.sample_rate, spec.channels),
            peaked_samples: 0,
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
        // Check for errors before dropping the stream: cpal's PipeWire backend reports
        // "Device disconnected" while tearing down streams on non-default devices.
        let error = recording.error.lock().map_err(|e| e.to_string())?.take();
        drop(recording.stream);
        if let Some(error) = error {
            return Err(error);
        }
        let mut samples = recording.samples.lock().map_err(|e| e.to_string())?;
        if samples.is_empty() {
            return Err("No audio was recorded.".into());
        }
        let clip = encode_recording(&mut samples, recording.spec)?;
        // A new take has nothing to do with the previous one's crop.
        self.set_clip(Some(clip), None);
        self.log("Recorded WAV encoded successfully.");
        Ok(())
    }

    fn play(&mut self) -> Result<(), String> {
        self.playback = None;
        let (start, end) = self.selection();
        if self.position < start || self.position >= end {
            self.position = start;
        }
        let source = clip_source(self.clip.clone().ok_or("No clip attached")?, self.trim)?;
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
            self.set_clip(entry.audio.clone(), entry.trim);
        } else if self.original.trim != entry.trim {
            self.playback = None;
            self.trim = entry.trim;
            let (start, end) = self.selection();
            self.position = self.position.clamp(start, end);
        }
        self.original = entry.clone();
    }

    fn seek(&mut self, position: f64) {
        if self.recording.is_some() || self.clip.is_none() {
            return;
        }
        let (start, end) = self.selection();
        let position = position.clamp(start, end);
        if let Some((_, player)) = &self.playback {
            if let Err(error) = player.try_seek(std::time::Duration::from_secs_f64(position)) {
                self.toasts.error(format!("Could not seek audio: {error}"));
                return;
            }
        }
        self.position = position;
    }

    fn handle_action(
        &mut self,
        action: crate::keybinds::Action,
        changed: &mut Option<crate::AudioState>,
        project_actions: &mut Vec<crate::keybinds::Action>,
    ) {
        use crate::keybinds::Action;
        match action {
            Action::Undo | Action::Redo | Action::RedoAlternative => {
                if self.recording.is_some() {
                    self.toasts.error("Stop recording before undo or redo.");
                } else {
                    project_actions.push(action);
                }
            }
            Action::SaveProject | Action::SaveProjectAs => {
                if self.recording.is_some() {
                    match self.stop() {
                        Ok(()) => *changed = Some(self.audio_state()),
                        Err(error) => {
                            self.toasts
                                .error(format!("Could not finish recording: {error}"));
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
                            *changed = Some(self.audio_state());
                        }
                        Err(error) => self.toasts.error(format!("Recording error: {error}")),
                    }
                } else if !self.inputs.is_empty() {
                    if let Err(error) = self.start() {
                        self.log(format!("Could not start recording: {error}"));
                        self.toasts.error(error);
                    }
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
                    if let Err(error) = self.play() {
                        self.toasts.error(error);
                    }
                }
            }
            Action::RecorderStopPlayback if self.recording.is_none() => {
                self.playback = None;
                self.position = self.selection().0;
            }
            Action::RecorderSeekBack => self.seek(self.position - 5.0),
            Action::RecorderSeekForward => self.seek(self.position + 5.0),
            Action::RecorderSeekStart => self.seek(self.selection().0),
            Action::RecorderSeekEnd => self.seek(self.selection().1),
            _ => {}
        }
    }

    fn update_audio(&mut self, ctx: &egui::Context) {
        if let Some((_, player)) = &self.playback {
            if player.empty() {
                self.playback = None;
                self.position = self.selection().0;
                self.log("Playback finished.");
            } else {
                self.position = player.get_pos().as_secs_f64().min(self.duration);
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
        if let Some(recording) = &mut self.recording {
            if let Ok(samples) = recording.samples.lock() {
                recording
                    .peaks
                    .extend_i16(&samples[recording.peaked_samples..]);
                recording.peaked_samples = samples.len();
            }
            let failure = recording.error.lock().ok().and_then(|error| error.clone());
            if let Some(error) = failure {
                self.last_capture = Some(recording.snapshot());
                let summary = self.last_capture.as_ref().unwrap().summary();
                self.recording = None;
                self.log(format!("Stream error: {error}. {summary}"));
                self.toasts.error(error);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
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
                .with_inner_size([780.0, 540.0])
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
                                        self.set_clip(Some(clip), None);
                                        result = Some(self.audio_state());
                                        self.log("WAV import succeeded.");
                                        self.toasts.success("Imported the WAV file.");
                                    }
                                    Err(e) => {
                                        self.log(format!("WAV import failed: {e}"));
                                        self.toasts.error(format!("Could not import audio: {e}"))
                                    }
                                }
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(self.clip.is_some(), egui::Button::new("Delete clip")).clicked() {
                                self.set_clip(None, None);
                                result = Some(self.audio_state());
                            }
                            if self.clip.is_some() {
                                let (start, end) = self.selection();
                                ui.label(time_label(end - start)).on_hover_text("Length of the cropped clip");
                            }
                        });
                    }
                });
                ui.add_space(18.0);
                self.show_waveform(ui, &mut result);
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
                        }
                    });
                });
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
                self.toasts.show(ui.ctx());
            },
        );
        if cancel {
            if self.recording.is_some() {
                match self.stop() {
                    Ok(()) => result = Some(self.audio_state()),
                    Err(error) => {
                        self.log(format!("Stop failed: {error}"));
                        self.toasts.error(error);
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
        crate::Language::L1 => (crate::ui::L1_COLOR, crate::ui::L1_ACCENT, l1_name),
        crate::Language::Latin => (crate::ui::LATIN_COLOR, crate::ui::LATIN_ACCENT, "Latin"),
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
                        .font(crate::ui::entry_font(entry.language, 12.0))
                        .color(accent)
                        .strong(),
                );
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(&entry.text)
                    .font(crate::ui::entry_font(entry.language, 15.0))
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
            &*import_wav(imported_bytes.clone()).unwrap(),
            imported_bytes
        );

        let mut expected = original.clone();
        high_pass_recording(&mut expected, spec.sample_rate, spec.channels);
        fade_recording(&mut expected, spec.sample_rate, spec.channels);
        let encoded = encode_recording(&mut original.clone(), spec).unwrap();
        let reader = hound::WavReader::new(Cursor::new(encoded)).unwrap();
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
            audio: Some(b"clip".as_slice().into()),
            trim: None,
            lemmas: Vec::new(),
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
            trim: None,
            lemmas: Vec::new(),
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

    fn test_clip(rate: u32, channels: u16, frames: u32) -> Arc<[u8]> {
        let mut wav = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut wav,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..frames * u32::from(channels) {
            writer.write_sample(16384i16).unwrap();
        }
        writer.finalize().unwrap();
        wav.into_inner().into()
    }

    #[test]
    fn selected_frames_fit_any_clip() {
        let trim = |start, end| Some(crate::Trim { start, end });
        assert_eq!(selected_frames(None, 10), 0..10);
        assert_eq!(selected_frames(trim(2, 6), 10), 2..6);
        assert_eq!(selected_frames(trim(2, 60), 10), 2..10);
        assert_eq!(selected_frames(trim(20, 60), 10), 9..10);
        assert_eq!(selected_frames(trim(4, 4), 10), 4..5);
        assert_eq!(selected_frames(trim(4, 8), 0), 0..0);
    }

    #[test]
    fn cut_gain_ramps_only_edges_that_cut_into_the_clip() {
        let selected = 1000..3000;
        assert_eq!(cut_gain(1000, &selected, 4000, 10_000), 0.0);
        assert_eq!(cut_gain(1050, &selected, 4000, 10_000), 0.5);
        assert_eq!(cut_gain(2000, &selected, 4000, 10_000), 1.0);
        assert_eq!(cut_gain(2999, &selected, 4000, 10_000), 0.0);
        assert_eq!(cut_gain(0, &(0..4000), 4000, 10_000), 1.0);
        assert_eq!(cut_gain(3999, &(0..4000), 4000, 10_000), 1.0);
    }

    #[test]
    fn clip_source_plays_only_the_selection_and_seeks_inside_it() {
        use rodio::Source;
        let clip = test_clip(1000, 2, 1000);
        let trim = Some(crate::Trim {
            start: 250,
            end: 750,
        });
        let source = clip_source(clip.clone(), trim).unwrap();
        assert_eq!(source.total_duration().unwrap().as_secs_f64(), 0.5);
        let samples: Vec<_> = source.collect();
        assert_eq!(samples.len(), 1000);
        assert_eq!(samples[0], 0.0);
        assert!((samples[500] - 0.5).abs() < 0.001);
        assert_eq!(*samples.last().unwrap(), 0.0);

        let mut source = clip_source(clip.clone(), trim).unwrap();
        source
            .try_seek(std::time::Duration::from_millis(500))
            .unwrap();
        assert_eq!(source.by_ref().count(), 500);
        source.try_seek(std::time::Duration::ZERO).unwrap();
        assert_eq!(source.frame, 250);
        source.try_seek(std::time::Duration::from_secs(9)).unwrap();
        assert_eq!(source.next(), None);

        assert_eq!(clip_source(clip, None).unwrap().count(), 2000);
    }

    #[test]
    fn crop_handles_keep_a_minimum_selection_and_bound_seeking() {
        use crate::keybinds::Action;
        use crate::waveform::Handle;
        let entry = crate::Entry {
            participant: crate::Participant::Teacher,
            language: crate::Language::Latin,
            text: String::new(),
            audio: Some(test_clip(1000, 1, 10_000)),
            trim: None,
            lemmas: Vec::new(),
        };
        let mut editor = Editor::for_test((0, 0), entry);
        assert_eq!(editor.selection(), (0.0, 10.0));
        editor.move_handle(Handle::Start, 0.2);
        editor.move_handle(Handle::End, 0.5);
        assert_eq!(
            editor.trim,
            Some(crate::Trim {
                start: 2000,
                end: 5000
            })
        );
        assert_eq!(editor.position, 2.0);
        let (mut changed, mut actions) = (None, Vec::new());
        editor.handle_action(Action::RecorderSeekEnd, &mut changed, &mut actions);
        assert_eq!(editor.position, 5.0);
        editor.handle_action(Action::RecorderSeekForward, &mut changed, &mut actions);
        assert_eq!(editor.position, 5.0);
        editor.handle_action(Action::RecorderSeekStart, &mut changed, &mut actions);
        assert_eq!(editor.position, 2.0);

        // Handles stop short of crossing each other.
        editor.move_handle(Handle::Start, 0.9);
        assert_eq!(editor.trim.unwrap().start, 4950);
        editor.move_handle(Handle::End, 0.0);
        assert_eq!(editor.trim.unwrap().end, 5000);

        // Back to the whole clip means no crop.
        editor.move_handle(Handle::Start, -1.0);
        editor.move_handle(Handle::End, 2.0);
        assert_eq!(editor.trim, None);
    }

    #[test]
    fn replacing_the_clip_clears_the_crop_and_undo_restores_it() {
        let trim = Some(crate::Trim {
            start: 100,
            end: 200,
        });
        let clip = test_clip(1000, 1, 1000);
        let mut entry = crate::Entry {
            participant: crate::Participant::Teacher,
            language: crate::Language::Latin,
            text: String::new(),
            audio: Some(clip.clone()),
            trim,
            lemmas: Vec::new(),
        };
        let mut editor = Editor::for_test((0, 0), entry.clone());
        assert_eq!(editor.position, 0.1);
        editor.set_clip(Some(test_clip(1000, 1, 500)), None);
        assert_eq!(editor.selection(), (0.0, 0.5));
        assert!(editor.peaks.is_some());

        let mut editor = Editor::for_test((0, 0), entry.clone());
        entry.trim = None;
        editor.sync_entry(&entry);
        assert_eq!(editor.selection(), (0.0, 1.0));
        assert_eq!(editor.position, 0.1);
        entry.trim = trim;
        editor.sync_entry(&entry);
        assert_eq!(editor.selection(), (0.1, 0.2));
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
        assert_eq!(clip_duration(&wav.into_inner()).unwrap(), 2.0);
        assert_eq!(time_label(83.9), "01:23.90");
        assert!(clip_duration(b"not a WAV").is_err());
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
