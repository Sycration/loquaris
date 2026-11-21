//! Print-friendly, two-column transcript export.
use loquaris_core::{Entry, Language, Lesson, Paper, Participant, PdfSettings, Project};
use printpdf::{
    Color, LinePoint, Mm, Op, PaintMode, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb, TextItem, WindingOrder,
};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const SANS: &[u8] = include_bytes!("../../../assets/IBMPlexSans-Regular.ttf");
const LATIN: &[u8] = include_bytes!("../../../assets/SourceSerif4-Regular.ttf");
const MARGIN: f32 = 36.0;
/// The size the layout was designed at; every size scales by `font_size / 15`.
const DEFAULT_FONT_SIZE: f32 = 15.0;

/// A PDF script of the given lessons (all lessons when `None`), in script
/// order, using the project's paper and font size. Lessons keep the number
/// of their position in the whole project, so a single exported lesson is
/// labelled as it is in the editor.
pub fn render(project: &Project, lessons: Option<&[String]>) -> Result<Vec<u8>, String> {
    let selected = crate::select_lessons(project, lessons)?;
    build(&project.name, &project.l1_name, &selected, project.pdf)
}

fn dimensions(paper: Paper) -> (f32, f32) {
    match paper {
        Paper::A4 => (210.0, 297.0),
        Paper::Letter => (215.9, 279.4),
        Paper::Legal => (215.9, 355.6),
        Paper::A3 => (297.0, 420.0),
    }
}

fn font(bytes: &[u8]) -> Result<ParsedFont, String> {
    ParsedFont::from_bytes(bytes, 0, &mut Vec::new())
        .ok_or_else(|| "Could not load bundled PDF font.".into())
}

fn text_width(font: &ParsedFont, text: &str, size: f32) -> f32 {
    text.chars()
        .map(|c| {
            font.lookup_glyph_index(c as u32)
                .and_then(|id| font.get_glyph_width(id))
                .unwrap_or(font.units_per_em / 2) as f32
        })
        .sum::<f32>()
        * size
        / font.units_per_em as f32
}

fn check_text(font: &ParsedFont, text: &str, field: &str) -> Result<(), String> {
    for character in text.nfc() {
        if !matches!(character, '\n' | '\r' | '\t')
            && font.lookup_glyph_index(character as u32).is_none()
        {
            return Err(format!(
                "Cannot export {field}: the bundled font does not support '{character}' (U+{:04X}).",
                character as u32
            ));
        }
    }
    Ok(())
}

/// Wrap at whitespace when possible and at grapheme boundaries for long words.
fn wrap(text: &str, font: &ParsedFont, size: f32, width: f32) -> Vec<String> {
    let text: String = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .nfc()
        .collect();
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for grapheme in paragraph.graphemes(true) {
            if !line.is_empty()
                && text_width(font, &line, size) + text_width(font, grapheme, size) > width
            {
                if let Some(split) = line.rfind(char::is_whitespace) {
                    let rest = line[split..].trim_start().to_owned();
                    result.push(line[..split].trim_end().to_owned());
                    line = rest;
                } else {
                    result.push(std::mem::take(&mut line));
                }
            }
            line.push_str(grapheme);
        }
        result.push(line);
    }
    result
}

fn color(r: f32, g: f32, b: f32) -> Color {
    Color::Rgb(Rgb::new(r, g, b, None))
}

struct Layout {
    pages: Vec<PdfPage>,
    ops: Vec<Op>,
    width: f32,
    height: f32,
    mm: (f32, f32),
    y: f32,
    sans_id: PdfFontHandle,
    latin_id: PdfFontHandle,
    sans: ParsedFont,
    latin: ParsedFont,
    project_name: String,
    scale: f32,
}

impl Layout {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: Color) {
        let points = [
            (x, y),
            (x + width, y),
            (x + width, y + height),
            (x, y + height),
        ]
        .into_iter()
        .map(|(x, y)| LinePoint {
            p: Point {
                x: Pt(x),
                y: Pt(self.height - y),
            },
            bezier: false,
        })
        .collect();
        self.ops.push(Op::SetFillColor { col: fill });
        self.ops.push(Op::DrawPolygon {
            polygon: Polygon {
                rings: vec![PolygonRing { points }],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            },
        });
    }

    fn text(&mut self, x: f32, baseline: f32, text: &str, size: f32, latin: bool, fill: Color) {
        if text.is_empty() {
            return;
        }
        self.ops.extend([
            Op::StartTextSection,
            Op::SetFillColor { col: fill },
            Op::SetFont {
                font: if latin {
                    self.latin_id.clone()
                } else {
                    self.sans_id.clone()
                },
                size: Pt(size),
            },
            Op::SetTextCursor {
                pos: Point {
                    x: Pt(x),
                    y: Pt(self.height - baseline),
                },
            },
            Op::ShowText {
                items: vec![TextItem::Text(text.to_owned())],
            },
            Op::EndTextSection,
        ]);
    }

    /// Finish the current page, if anything is on it, and start the next one
    /// with the project name and column headings.
    fn new_page(&mut self) {
        let scale = self.scale;
        if !self.ops.is_empty() {
            self.pages.push(PdfPage::new(
                Mm(self.mm.0),
                Mm(self.mm.1),
                std::mem::take(&mut self.ops),
            ));
        }
        self.y = MARGIN;
        let project = self.project_name.clone();
        if !project.is_empty() {
            for line in wrap(
                &project,
                &self.sans,
                14.0 * scale,
                self.width - 2.0 * MARGIN,
            )
            .into_iter()
            .take(2)
            {
                self.text(
                    MARGIN,
                    self.y + 14.0 * scale,
                    &line,
                    14.0 * scale,
                    false,
                    color(0.0, 0.0, 0.0),
                );
                self.y += 18.0 * scale;
            }
            self.y += 4.0 * scale;
        }
        self.text(
            MARGIN,
            self.y + 18.0 * scale,
            "Teacher",
            18.0 * scale,
            false,
            color(0.0, 0.0, 0.0),
        );
        self.text(
            self.width / 2.0 + 8.0,
            self.y + 18.0 * scale,
            "Student",
            18.0 * scale,
            false,
            color(0.0, 0.0, 0.0),
        );
        self.y += 30.0 * scale;
    }

    fn lesson_header(&mut self, title: &str) {
        let scale = self.scale;
        for line in wrap(
            title,
            &self.sans,
            12.0 * scale,
            self.width - 2.0 * MARGIN - 16.0,
        ) {
            if self.y + 24.0 * scale > self.height - MARGIN {
                self.new_page();
            }
            self.rect(
                MARGIN,
                self.y,
                self.width - 2.0 * MARGIN,
                22.0 * scale,
                color(0.9, 0.9, 0.9),
            );
            self.text(
                MARGIN + 8.0,
                self.y + 15.0 * scale,
                &line,
                12.0 * scale,
                false,
                color(0.0, 0.0, 0.0),
            );
            self.y += 22.0 * scale;
        }
        self.y += 6.0 * scale;
    }

    /// A filled box in the speaker's column; one that does not fit splits
    /// across pages, each continuation under a "Lesson N (continued)" bar.
    fn entry(&mut self, entry: &Entry, l1: &str, lesson: &str) {
        let scale = self.scale;
        let line_height = 19.0 * scale;
        let bottom_padding = 8.0 * scale;
        let latin = entry.language == Language::Latin;
        let column = (self.width - 2.0 * MARGIN) / 2.0;
        let x = MARGIN
            + if entry.participant == Participant::Student {
                column
            } else {
                0.0
            }
            + 8.0;
        let width = column - 16.0;
        let lines = wrap(
            &entry.text,
            if latin { &self.latin } else { &self.sans },
            15.0 * scale,
            width - 16.0,
        );
        let mut labels = wrap(
            if latin { "Latin" } else { l1 },
            if latin { &self.latin } else { &self.sans },
            12.0 * scale,
            width - 16.0,
        );
        if labels.len() > 3 {
            labels.truncate(3);
            labels[2].push('…');
        }
        let continued = format!(
            "{} (continued)",
            lesson.split(':').next().unwrap_or("Lesson")
        );
        let mut offset = 0;
        while offset < lines.len() {
            let header = (labels.len() as f32 * 15.0 + 14.0) * scale;
            let available = self.height - MARGIN - self.y;
            if available < header + line_height + bottom_padding {
                self.new_page();
                self.lesson_header(&continued);
                continue;
            }
            let count = (((available - header - bottom_padding) / line_height).floor() as usize)
                .min(lines.len() - offset)
                .max(1);
            let height = header + count as f32 * line_height + bottom_padding;
            self.rect(
                x,
                self.y,
                width,
                height,
                if latin {
                    color(1.0, 0.91, 0.91)
                } else {
                    color(0.88, 0.96, 0.98)
                },
            );
            for (i, label) in labels.iter().enumerate() {
                self.text(
                    x + 8.0,
                    self.y + (15.0 + i as f32 * 15.0) * scale,
                    label,
                    12.0 * scale,
                    latin,
                    color(0.0, 0.0, 0.0),
                );
            }
            for (i, line) in lines[offset..offset + count].iter().enumerate() {
                self.text(
                    x + 8.0,
                    self.y + header + 15.0 * scale + i as f32 * line_height,
                    line,
                    15.0 * scale,
                    latin,
                    color(0.0, 0.0, 0.0),
                );
            }
            self.y += height + 7.0 * scale;
            offset += count;
            if offset < lines.len() {
                self.new_page();
                self.lesson_header(&continued);
            }
        }
    }
}

/// `lessons` pairs each lesson with its number in the project.
fn build(
    name: &str,
    l1: &str,
    lessons: &[(usize, &Lesson)],
    settings: PdfSettings,
) -> Result<Vec<u8>, String> {
    let PdfSettings { paper, font_size } = settings;
    if !font_size.is_finite()
        || !(PdfSettings::MIN_FONT_SIZE..=PdfSettings::MAX_FONT_SIZE).contains(&font_size)
    {
        return Err(format!(
            "Font size must be between {} and {} pt.",
            PdfSettings::MIN_FONT_SIZE,
            PdfSettings::MAX_FONT_SIZE
        ));
    }
    let sans = font(SANS)?;
    let latin = font(LATIN)?;
    check_text(&sans, name, "the project name")?;
    check_text(&sans, l1, "the L1 language name")?;
    for &(number, lesson) in lessons {
        check_text(&sans, &lesson.name, &format!("lesson {number} name"))?;
        for (entry_index, entry) in lesson.entries.iter().enumerate() {
            check_text(
                if entry.language == Language::Latin {
                    &latin
                } else {
                    &sans
                },
                &entry.text,
                &format!("lesson {number}, entry {}", entry_index + 1),
            )?;
        }
    }
    let mut doc = PdfDocument::new(name);
    let sans_id = PdfFontHandle::External(doc.add_font(&sans));
    let latin_id = PdfFontHandle::External(doc.add_font(&latin));
    let mm = dimensions(paper);
    let mut layout = Layout {
        pages: Vec::new(),
        ops: Vec::new(),
        width: mm.0 * 72.0 / 25.4,
        height: mm.1 * 72.0 / 25.4,
        mm,
        y: 0.0,
        sans_id,
        latin_id,
        sans,
        latin,
        project_name: name.nfc().collect(),
        scale: font_size / DEFAULT_FONT_SIZE,
    };
    layout.new_page();
    for (index, &(number, lesson)) in lessons.iter().enumerate() {
        let title: String = format!("Lesson {number}: {}", lesson.name).nfc().collect();
        // Every lesson starts at the top of a page.
        if index > 0 {
            layout.new_page();
        }
        layout.lesson_header(&title);
        for entry in &lesson.entries {
            layout.entry(entry, l1, &title);
        }
        layout.lesson_header(&title);
        layout.y += 6.0 * layout.scale;
    }
    layout
        .pages
        .push(PdfPage::new(Mm(mm.0), Mm(mm.1), layout.ops));
    Ok(doc
        .with_pages(layout.pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{entry, lesson, project};

    fn pdftotext(pdf: &[u8], args: &[&str]) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("loquaris-export-{}", crate::loqr::new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("test.pdf");
        std::fs::write(&path, pdf).unwrap();
        let output = std::process::Command::new("pdftotext")
            .args(args)
            .arg(&path)
            .arg("-")
            .output();
        std::fs::remove_dir_all(&dir).unwrap();
        let output = output.ok()?;
        assert!(output.status.success());
        Some(String::from_utf8(output.stdout).unwrap())
    }

    /// The `/Count` of the page tree, which printpdf writes uncompressed.
    fn page_count(pdf: &[u8]) -> usize {
        let text = String::from_utf8_lossy(pdf);
        let (_, rest) = text.split_once("/Type/Pages/Count ").unwrap();
        rest[..rest.find(|c: char| !c.is_ascii_digit()).unwrap()]
            .parse()
            .unwrap()
    }

    /// The first page's size in points.
    fn page_size(pdf: &[u8]) -> (f32, f32) {
        let text = String::from_utf8_lossy(pdf);
        let (_, rest) = text.split_once("/MediaBox[").unwrap();
        let numbers: Vec<f32> = rest[..rest.find(']').unwrap()]
            .split_whitespace()
            .map(|number| number.parse().unwrap())
            .collect();
        (numbers[2], numbers[3])
    }

    #[test]
    fn wrap_preserves_macrons_breaks_and_long_words() {
        let font = font(LATIN).unwrap();
        let lines = wrap("māter\n", &font, 15.0, 30.0);
        assert_eq!(lines.last().unwrap(), "");
        assert!(lines.join("").contains("māter"));
        let word = "antidisestablishmentarianism";
        let wrapped = wrap(word, &font, 15.0, 40.0);
        assert_eq!(wrapped.concat(), word);
        assert!(
            wrapped
                .iter()
                .all(|line| text_width(&font, line, 15.0) <= 40.0)
        );
        assert_eq!(wrap("a\tb\r\nc", &font, 15.0, 500.0), ["a    b", "c"]);
    }

    #[test]
    fn pdf_contains_long_entries_split_across_pages() {
        let mut project = project(vec![lesson(
            "Prīma",
            vec![
                entry(Participant::Teacher, Language::L1, "Teacher introduction"),
                entry(
                    Participant::Student,
                    Language::Latin,
                    &format!("{} fīnis", "māter ".repeat(700)),
                ),
            ],
        )]);
        project.pdf.paper = Paper::Letter;
        let pdf = render(&project, None).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(page_count(&pdf) > 1);
        if let Some(text) = pdftotext(&pdf, &["-layout"]) {
            assert!(text.contains("Teacher introduction"), "{text}");
            assert!(text.contains("fīnis"), "{text}");
            assert_eq!(text.matches("māter").count(), 700);
            assert!(text.contains("Lesson 1 (continued)"), "{text}");
            assert!(text.matches('\u{c}').count() > 1);
        }
        let (width, height) = page_size(&pdf);
        assert!((width - 612.0).abs() < 0.01 && (height - 792.0).abs() < 0.01);
    }

    #[test]
    fn each_lesson_starts_on_a_new_page() {
        let named = |name: &str| {
            lesson(
                name,
                vec![entry(
                    Participant::Teacher,
                    Language::Latin,
                    &format!("Salvē, {name}"),
                )],
            )
        };
        let project = project(vec![named("Prīma"), named("Secunda"), named("Tertia")]);
        let pdf = render(&project, None).unwrap();
        assert_eq!(page_count(&pdf), 3);
        let Some(text) = pdftotext(&pdf, &[]) else {
            return;
        };
        let pages: Vec<_> = text
            .split('\u{c}')
            .filter(|page| !page.trim().is_empty())
            .collect();
        assert_eq!(pages.len(), 3, "{text}");
        for (page, name) in pages.iter().zip(["Prīma", "Secunda", "Tertia"]) {
            assert!(page.contains(&format!("Salvē, {name}")), "{page}");
            assert_eq!(page.matches("Lesson ").count(), 2, "{page}");
            assert!(
                page.contains("Course") && page.contains("Student"),
                "{page}"
            );
        }
    }

    #[test]
    fn selected_lessons_keep_script_order_and_project_numbers() {
        let named = |name: &str| {
            lesson(
                name,
                vec![entry(Participant::Teacher, Language::Latin, name)],
            )
        };
        let project = project(vec![named("Prīma"), named("Secunda"), named("Tertia")]);
        let ids = [project.lessons[2].id.clone(), project.lessons[0].id.clone()];
        let pdf = render(&project, Some(&ids)).unwrap();
        assert_eq!(page_count(&pdf), 2);
        if let Some(text) = pdftotext(&pdf, &[]) {
            let first = text.find("Lesson 1: Prīma").unwrap();
            let third = text.find("Lesson 3: Tertia").unwrap();
            assert!(first < third, "{text}");
            assert!(!text.contains("Secunda"), "{text}");
        }
        let error = render(&project, Some(&["missing".into()])).unwrap_err();
        assert!(error.contains("no longer exists"), "{error}");
        assert_eq!(page_count(&render(&project, Some(&[])).unwrap()), 1);
    }

    #[test]
    fn export_rejects_missing_glyphs() {
        let mut project = project(vec![lesson(
            "First",
            vec![entry(Participant::Teacher, Language::L1, "\u{10ffff}")],
        )]);
        let error = render(&project, None).unwrap_err();
        assert_eq!(
            error,
            "Cannot export lesson 1, entry 1: the bundled font does not support '\u{10ffff}' (U+10FFFF)."
        );
        project.lessons[0].entries.clear();
        project.lessons[0].name = "\u{10ffff}".into();
        assert!(
            render(&project, None)
                .unwrap_err()
                .contains("lesson 1 name")
        );
        project.lessons[0].name.clear();
        project.name = "\u{10ffff}".into();
        assert!(
            render(&project, None)
                .unwrap_err()
                .contains("the project name")
        );
    }

    #[test]
    fn each_paper_size_produces_a_pdf() {
        for paper in [Paper::A4, Paper::Letter, Paper::Legal, Paper::A3] {
            let mut project = project(Vec::new());
            project.pdf.paper = paper;
            let pdf = render(&project, None).unwrap();
            assert!(pdf.starts_with(b"%PDF-"));
            let (width, height) = page_size(&pdf);
            let (mm_width, mm_height) = dimensions(paper);
            assert!((width - mm_width * 72.0 / 25.4).abs() < 0.01);
            assert!((height - mm_height * 72.0 / 25.4).abs() < 0.01);
        }
    }

    #[test]
    fn font_size_changes_text_geometry_and_keeps_long_entries() {
        let mut project = project(vec![lesson(
            "First",
            vec![entry(
                Participant::Student,
                Language::Latin,
                &format!("Probe {} fīnis", "māter ".repeat(150)),
            )],
        )]);
        project.pdf.paper = Paper::Letter;
        let mut heights = Vec::new();
        for size in [8.0, 30.0] {
            project.pdf.font_size = size;
            let pdf = render(&project, None).unwrap();
            if let Some(text) = pdftotext(&pdf, &["-bbox"]) {
                assert_eq!(text.matches(">māter</word>").count(), 150);
                assert!(text.contains(">fīnis</word>"));
                let probe = text
                    .lines()
                    .find(|line| line.contains(">Probe</word>"))
                    .unwrap();
                let attribute = |name: &str| -> f32 {
                    probe
                        .split(&format!("{name}=\""))
                        .nth(1)
                        .unwrap()
                        .split('"')
                        .next()
                        .unwrap()
                        .parse()
                        .unwrap()
                };
                heights.push(attribute("yMax") - attribute("yMin"));
            }
        }
        if heights.len() == 2 {
            assert!(heights[1] > heights[0] * 3.0);
        }
        for size in [f32::NAN, 7.5, 31.0] {
            project.pdf.font_size = size;
            assert!(render(&project, None).unwrap_err().contains("Font size"));
        }
    }
}
