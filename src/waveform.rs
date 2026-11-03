//! The audio editor's waveform: a peak envelope that can grow during recording,
//! and the widget that paints it with its two crop handles.
use eframe::egui;
use std::io::Cursor;

/// Envelope resolution. Fine enough for short clips while keeping a ten minute
/// recording cheap to repaint.
const BLOCKS_PER_SECOND: u32 = 400;
/// 12.5 ms: one full cycle of an 80 Hz voice.
const MIN_COLUMN_BLOCKS: usize = 5;
const HANDLE_WIDTH: f32 = 20.0;
const HANDLE_OVERHANG: f32 = 8.0;
const HANDLE_TIME_HEIGHT: f32 = 18.0;
const CURSOR_TIME_HEIGHT: f32 = 24.0;
const WAVE_HEIGHT: f32 = 110.0;
const BACKGROUND: egui::Color32 = egui::Color32::from_gray(68);
const WAVE: egui::Color32 = egui::Color32::from_gray(176);
const OUTSIDE_SELECTION: egui::Color32 = egui::Color32::from_black_alpha(130);
const HANDLE: egui::Color32 = egui::Color32::from_gray(200);
const CURSOR: egui::Color32 = egui::Color32::RED;

/// Minimum and maximum amplitude of each block of frames, across all channels.
pub struct Peaks {
    channels: u64,
    samples_per_block: u64,
    samples: u64,
    blocks: Vec<(f32, f32)>,
    /// The loudest sample on each side of zero, as positive numbers.
    highest: f32,
    lowest: f32,
}

impl Peaks {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let channels = u64::from(channels.max(1));
        Self {
            channels,
            samples_per_block: u64::from((sample_rate / BLOCKS_PER_SECOND).max(1)) * channels,
            samples: 0,
            blocks: Vec::new(),
            highest: 0.0,
            lowest: 0.0,
        }
    }

    fn push(&mut self, sample: f32) {
        if self.samples.is_multiple_of(self.samples_per_block) {
            self.blocks.push((sample, sample));
        } else if let Some((min, max)) = self.blocks.last_mut() {
            *min = min.min(sample);
            *max = max.max(sample);
        }
        self.highest = self.highest.max(sample);
        self.lowest = self.lowest.max(-sample);
        self.samples += 1;
    }

    pub fn extend_i16(&mut self, samples: &[i16]) {
        for &sample in samples {
            self.push(f32::from(sample) / 32768.0);
        }
    }

    pub fn from_clip(clip: &[u8]) -> Option<Self> {
        let mut reader = hound::WavReader::new(Cursor::new(clip)).ok()?;
        let spec = reader.spec();
        let mut peaks = Self::new(spec.sample_rate, spec.channels);
        if spec.sample_format == hound::SampleFormat::Float {
            for sample in reader.samples::<f32>() {
                peaks.push(sample.ok()?);
            }
        } else {
            let scale = (2f64).powi(i32::from(spec.bits_per_sample) - 1) as f32;
            for sample in reader.samples::<i32>() {
                peaks.push(sample.ok()? as f32 / scale);
            }
        }
        Some(peaks)
    }

    pub fn frames(&self) -> u64 {
        self.samples / self.channels
    }

    /// An amplitude as a fraction of the display height from the centre. Each
    /// side of zero is stretched on its own, so that the waveform reaches both
    /// the top and the bottom even when the audio is lopsided.
    fn stretched(&self, amplitude: f32) -> f32 {
        let peak = if amplitude >= 0.0 {
            self.highest
        } else {
            self.lowest
        };
        if peak > 0.0 { amplitude / peak } else { 0.0 }
    }

    /// The range of amplitudes in one of `columns` equal slices of the clip.
    /// A slice always spans at least [`MIN_COLUMN_BLOCKS`], or a zoomed-in low
    /// voice would show stripes wherever a column covers only part of a cycle.
    fn column(&self, column: usize, columns: usize) -> (f32, f32) {
        let count = self.blocks.len();
        let mut first = column * count / columns;
        let mut last = ((column + 1) * count / columns).max(first + 1);
        if last - first < MIN_COLUMN_BLOCKS {
            let start = ((first + last) / 2).saturating_sub(MIN_COLUMN_BLOCKS / 2);
            last = (start + MIN_COLUMN_BLOCKS).min(count);
            first = last.saturating_sub(MIN_COLUMN_BLOCKS);
        }
        self.blocks[first..last]
            .iter()
            .fold((f32::MAX, f32::MIN), |(min, max), block| {
                (min.min(block.0), max.max(block.1))
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
}

/// What to draw. Positions are fractions of the widget's waveform width.
pub struct View<'a> {
    pub peaks: Option<&'a Peaks>,
    /// How much of the width the waveform fills; less than 1 while a short
    /// recording is still growing.
    pub extent: f32,
    /// The crop selection. Without one, no handles are shown and the waveform
    /// cannot be used to seek.
    pub selection: Option<Selection>,
    /// The cursor's position and the time to show below it.
    pub playhead: Option<(f32, String)>,
}

pub struct Selection {
    pub start: f32,
    pub end: f32,
    pub start_time: String,
    pub end_time: String,
}

#[derive(Default)]
pub struct Interaction {
    pub moved: Option<(Handle, f32)>,
    pub released: bool,
    /// Where the waveform was clicked or dragged.
    pub seek: Option<f32>,
}

pub fn show(ui: &mut egui::Ui, view: View<'_>) -> Interaction {
    let size = egui::vec2(
        ui.available_width(),
        HANDLE_TIME_HEIGHT + WAVE_HEIGHT + 2.0 * HANDLE_OVERHANG + CURSOR_TIME_HEIGHT,
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    // Each handle is outside the part it bounds, so the handles remain
    // grabbable and clear of each other at any selection.
    let wave = egui::Rect::from_min_size(
        rect.min + egui::vec2(HANDLE_WIDTH, HANDLE_TIME_HEIGHT + HANDLE_OVERHANG),
        egui::vec2(rect.width() - 2.0 * HANDLE_WIDTH, WAVE_HEIGHT),
    );
    let tall = wave.expand2(egui::vec2(0.0, HANDLE_OVERHANG));
    let x_at = |fraction: f32| wave.left() + wave.width() * fraction.clamp(0.0, 1.0);
    let fraction_at = |x: f32| (x - wave.left()) / wave.width();
    let mut interaction = Interaction::default();
    if view.selection.is_some() {
        let response = ui
            .interact(
                tall,
                ui.scope_id().with("waveform_seek"),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if (response.is_pointer_button_down_on() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            interaction.seek = Some(fraction_at(pointer.x).clamp(0.0, 1.0));
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(wave, 0.0, BACKGROUND);
    if let Some(peaks) = view.peaks.filter(|peaks| !peaks.blocks.is_empty()) {
        let area = egui::Rect::from_min_size(
            wave.min,
            egui::vec2(wave.width() * view.extent.clamp(0.0, 1.0), wave.height()),
        );
        painter.add(egui::Shape::mesh(wave_mesh(
            peaks,
            area,
            ui.ctx().pixels_per_point(),
        )));
    }
    if let Some(selection) = &view.selection {
        let (start, end) = (x_at(selection.start), x_at(selection.end));
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(wave.left()..=start, wave.y_range()),
            0.0,
            OUTSIDE_SELECTION,
        );
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(end..=wave.right(), wave.y_range()),
            0.0,
            OUTSIDE_SELECTION,
        );
        for (handle, edge, direction) in [(Handle::Start, start, -1.0), (Handle::End, end, 1.0)] {
            let outer = edge + direction * HANDLE_WIDTH;
            let area =
                egui::Rect::from_x_y_ranges(edge.min(outer)..=edge.max(outer), tall.y_range());
            let id = ui.scope_id().with(("crop_handle", handle == Handle::Start));
            let response = ui
                .interact(area, id, egui::Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text(match handle {
                    Handle::Start => "Drag to choose where the clip starts",
                    Handle::End => "Drag to choose where the clip ends",
                });
            let pointer_x = response.interact_pointer_pos().map(|pointer| pointer.x);
            if response.drag_started()
                && let Some(x) = pointer_x
            {
                ui.data_mut(|data| data.insert_temp(id, x - edge));
            }
            if response.dragged()
                && let Some(x) = pointer_x
            {
                let grab: f32 = ui.data(|data| data.get_temp(id)).unwrap_or(0.0);
                interaction.moved = Some((handle, fraction_at(x - grab)));
            }
            interaction.released |= response.drag_stopped();
            paint_handle(&painter, area, direction);
        }
        paint_handle_times(&painter, rect, selection, start, end);
    }
    if let Some((fraction, time)) = &view.playhead {
        paint_cursor(&painter, rect, wave, x_at(*fraction), time);
    }
    interaction
}

/// Each handle's time, centred above it where that fits. The times are pushed
/// apart when the handles are close, and kept inside the widget.
fn paint_handle_times(
    painter: &egui::Painter,
    rect: egui::Rect,
    selection: &Selection,
    start: f32,
    end: f32,
) {
    let galley = |text: &str| {
        painter.layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        )
    };
    let (first, second) = (galley(&selection.start_time), galley(&selection.end_time));
    let (first_left, second_left) = handle_time_positions(
        rect.x_range(),
        (start - HANDLE_WIDTH / 2.0, first.size().x),
        (end + HANDLE_WIDTH / 2.0, second.size().x),
    );
    painter.galley(
        egui::pos2(first_left, rect.top()),
        first,
        egui::Color32::WHITE,
    );
    painter.galley(
        egui::pos2(second_left, rect.top()),
        second,
        egui::Color32::WHITE,
    );
}

/// Left edges for two labels given as (centre, width).
fn handle_time_positions(
    bounds: egui::Rangef,
    first: (f32, f32),
    second: (f32, f32),
) -> (f32, f32) {
    const GAP: f32 = 8.0;
    let (first_center, first_width) = first;
    let (second_center, second_width) = second;
    let mut left = (first_center - first_width / 2.0).max(bounds.min);
    let mut right = (second_center - second_width / 2.0).min(bounds.max - second_width);
    let overlap = left + first_width + GAP - right;
    if overlap > 0.0 {
        left = (left - overlap / 2.0).max(bounds.min);
        right = (left + first_width + GAP).min(bounds.max - second_width);
        left = left.min(right - GAP - first_width);
    }
    (left, right)
}

/// A red line with a V at each end pointing along it, and the time below.
fn paint_cursor(painter: &egui::Painter, rect: egui::Rect, wave: egui::Rect, x: f32, time: &str) {
    let stroke = egui::Stroke::new(2.0, CURSOR);
    painter.vline(x, wave.y_range(), egui::Stroke::new(1.5, CURSOR));
    let (reach, depth) = (5.0, HANDLE_OVERHANG - 1.0);
    for (tip, direction) in [(wave.top(), -1.0), (wave.bottom(), 1.0)] {
        painter.add(egui::Shape::line(
            vec![
                egui::pos2(x - reach, tip + direction * depth),
                egui::pos2(x, tip),
                egui::pos2(x + reach, tip + direction * depth),
            ],
            stroke,
        ));
    }
    let galley = painter.layout_no_wrap(
        time.to_owned(),
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
    );
    let width = galley.size().x;
    let left = (x - width / 2.0).clamp(rect.left(), (rect.right() - width).max(rect.left()));
    painter.galley(
        egui::pos2(left, wave.bottom() + HANDLE_OVERHANG + 1.0),
        galley,
        egui::Color32::WHITE,
    );
}

fn wave_mesh(peaks: &Peaks, area: egui::Rect, pixels_per_point: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let columns = (area.width() * pixels_per_point).round().max(1.0) as usize;
    let column_width = area.width() / columns as f32;
    let (center, half_height) = (area.center().y, area.height() / 2.0);
    for column in 0..columns {
        let (min, max) = peaks.column(column, columns);
        let top = center - peaks.stretched(max) * half_height;
        let bottom = center - peaks.stretched(min) * half_height;
        // At least one pixel tall, so that silence stays visible.
        let pad = ((1.0 / pixels_per_point - (bottom - top)) / 2.0).max(0.0);
        let left = area.left() + column as f32 * column_width;
        mesh.add_colored_rect(
            egui::Rect::from_min_max(
                egui::pos2(left, top - pad),
                egui::pos2(left + column_width, bottom + pad),
            ),
            WAVE,
        );
    }
    mesh
}

/// A light bar with a black chevron pointing at the selection.
fn paint_handle(painter: &egui::Painter, area: egui::Rect, direction: f32) {
    painter.rect_filled(area, 0.0, HANDLE);
    let center = area.center();
    let (reach, spread) = (4.0, 9.0);
    painter.add(egui::Shape::line(
        vec![
            center + egui::vec2(direction * reach, -spread),
            center + egui::vec2(-direction * reach, 0.0),
            center + egui::vec2(direction * reach, spread),
        ],
        egui::Stroke::new(3.5, egui::Color32::BLACK),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_grow_incrementally_and_keep_extremes_across_channels() {
        // 800 Hz gives two frames, or four stereo samples, per block.
        let mut peaks = Peaks::new(800, 2);
        peaks.extend_i16(&[0, 16384, -8192]);
        peaks.extend_i16(&[0, 0, -32768]);
        assert_eq!(peaks.frames(), 3);
        assert_eq!(peaks.blocks, [(-0.25, 0.5), (-1.0, 0.0)]);
    }

    #[test]
    fn columns_cover_their_blocks_and_never_too_few() {
        // At 400 Hz, every sample is a block of its own.
        let mut peaks = Peaks::new(400, 1);
        let mut samples = vec![3276; 10];
        samples.extend([-3276; 10]);
        peaks.extend_i16(&samples);
        let (high, low) = (3276.0 / 32768.0, -3276.0 / 32768.0);
        assert_eq!(peaks.column(0, 2), (high, high));
        assert_eq!(peaks.column(1, 2), (low, low));
        // Zoomed in, the range of a column is widened around its position, even at the ends.
        assert_eq!(peaks.column(0, 200), (high, high));
        assert_eq!(peaks.column(100, 200), (low, high));
        assert_eq!(peaks.column(199, 200), (low, low));
        // A clip with fewer blocks than the minimum still works.
        let mut short = Peaks::new(400, 1);
        short.extend_i16(&[16384]);
        assert_eq!(short.column(3, 8), (0.5, 0.5));
    }

    #[test]
    fn a_low_voice_stays_solid_when_zoomed_in() {
        let rate = 48_000;
        let mut peaks = Peaks::new(rate, 1);
        let samples: Vec<i16> = (0..rate)
            .map(|frame| {
                let phase = frame as f32 / rate as f32 * std::f32::consts::TAU * 85.0;
                (phase.sin() * 20_000.0) as i16
            })
            .collect();
        peaks.extend_i16(&samples);
        // A second of audio on 2000 columns: half a millisecond each.
        for column in 0..2000 {
            let (min, max) = peaks.column(column, 2000);
            assert!(min < 0.0 && max > 0.0, "gap in column {column}");
        }
    }

    /// Run the widget for one frame with these pointer events.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> Interaction {
        let peaks = Peaks::new(400, 1);
        let mut interaction = Interaction::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 200.0)),
            ));
            interaction = show(
                &mut child,
                View {
                    peaks: Some(&peaks),
                    extent: 1.0,
                    selection: Some(Selection {
                        start: 0.25,
                        end: 1.0,
                        start_time: String::new(),
                        end_time: String::new(),
                    }),
                    playhead: None,
                },
            );
        });
        output.textures_delta.clear();
        interaction
    }

    #[test]
    fn dragging_a_handle_moves_its_edge_without_jumping_to_the_pointer() {
        let ctx = egui::Context::default();
        // The waveform spans x = 20..620, so the start edge is at x = 170 and
        // its handle at 150..170. Grab it off-centre.
        let grab = egui::pos2(155.0, 60.0);
        let pointer = |pressed| egui::Event::PointerButton {
            pos: grab,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![egui::Event::PointerMoved(grab)]);
        frame(&ctx, vec![pointer(true)]);
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(grab + egui::vec2(10.0, 0.0))],
        );
        let moved = frame(
            &ctx,
            vec![egui::Event::PointerMoved(grab + egui::vec2(60.0, 0.0))],
        );
        let (handle, fraction) = moved.moved.unwrap();
        assert_eq!(handle, Handle::Start);
        assert!((fraction - 0.35).abs() < 0.001, "{fraction}");
        assert!(!moved.released);
        let released = frame(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: grab + egui::vec2(60.0, 0.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(released.released);
    }

    #[test]
    fn pressing_the_waveform_seeks_but_grabbing_a_handle_does_not() {
        let ctx = egui::Context::default();
        let press = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        // The waveform spans x = 20..620, and 320 is its middle.
        let middle = egui::pos2(320.0, 80.0);
        frame(&ctx, vec![egui::Event::PointerMoved(middle)]);
        let pressed = frame(&ctx, vec![press(middle, true)]);
        assert_eq!(pressed.seek, Some(0.5));
        let dragged = frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(9000.0, 80.0))],
        );
        assert_eq!(dragged.seek, Some(1.0));
        frame(&ctx, vec![press(egui::pos2(9000.0, 80.0), false)]);

        let handle = egui::pos2(160.0, 80.0);
        frame(&ctx, vec![egui::Event::PointerMoved(handle)]);
        assert_eq!(frame(&ctx, vec![press(handle, true)]).seek, None);
    }

    #[test]
    fn waveform_is_stretched_to_both_edges_even_when_lopsided() {
        let mut peaks = Peaks::new(400, 1);
        peaks.extend_i16(&[8192, -16384, 4096]);
        assert_eq!(peaks.stretched(0.25), 1.0);
        assert_eq!(peaks.stretched(-0.5), -1.0);
        assert_eq!(peaks.stretched(0.125), 0.5);
        let mut silence = Peaks::new(400, 1);
        silence.extend_i16(&[0, 0]);
        assert_eq!(silence.stretched(0.0), 0.0);
    }

    #[test]
    fn handle_times_stay_apart_and_inside_the_widget() {
        let bounds = egui::Rangef::new(0.0, 500.0);
        // Far apart: centred on their handles.
        assert_eq!(
            handle_time_positions(bounds, (100.0, 40.0), (300.0, 40.0)),
            (80.0, 280.0)
        );
        // At the very edges: held inside.
        assert_eq!(
            handle_time_positions(bounds, (10.0, 40.0), (490.0, 40.0)),
            (0.0, 460.0)
        );
        // Close together: pushed apart.
        assert_eq!(
            handle_time_positions(bounds, (240.0, 40.0), (260.0, 40.0)),
            (206.0, 254.0)
        );
        // Close together at either end: still no overlap.
        assert_eq!(
            handle_time_positions(bounds, (10.0, 40.0), (30.0, 40.0)),
            (0.0, 48.0)
        );
        assert_eq!(
            handle_time_positions(bounds, (470.0, 40.0), (490.0, 40.0)),
            (412.0, 460.0)
        );
    }

    #[test]
    fn peaks_read_integer_and_float_clips() {
        for (spec, full_scale) in [
            (8, hound::SampleFormat::Int),
            (32, hound::SampleFormat::Float),
        ]
        .map(|(bits, format)| {
            (
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 400,
                    bits_per_sample: bits,
                    sample_format: format,
                },
                format,
            )
        }) {
            let mut wav = Cursor::new(Vec::new());
            let mut writer = hound::WavWriter::new(&mut wav, spec).unwrap();
            if full_scale == hound::SampleFormat::Float {
                writer.write_sample(-0.5f32).unwrap();
            } else {
                writer.write_sample(-64i8).unwrap();
            }
            writer.finalize().unwrap();
            let peaks = Peaks::from_clip(&wav.into_inner()).unwrap();
            assert_eq!(peaks.blocks, [(-0.5, -0.5)]);
        }
        assert!(Peaks::from_clip(b"not a WAV").is_none());
    }
}
