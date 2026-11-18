//! Port of iText TextField / BaseField appearance generation, plus the slice of ColumnText /
//! BidiLine / PdfDocument.writeLineToContent that those use for laying out plain text.
//!
//! Not ported: bidirectional reordering and Arabic shaping (TextField.checkRTL -> BidiOrder /
//! ArabicLigaturizer): right-to-left text is laid out in logical order.
use super::acro::*;
use super::fonts::{std_resource_name, Desc, Font};
use super::syntax::{Lexer, Token};
use super::util::*;
use crate::report;
use lopdf::{Dictionary, Document, Object};

pub const ALIGN_LEFT: i32 = 0;
pub const ALIGN_CENTER: i32 = 1;
pub const ALIGN_RIGHT: i32 = 2;

pub const STYLE_SOLID: i32 = 0;
pub const STYLE_DASHED: i32 = 1;
pub const STYLE_BEVELED: i32 = 2;
pub const STYLE_INSET: i32 = 3;
pub const STYLE_UNDERLINE: i32 = 4;

/// java.awt.Color and iText's GrayColor / CMYKColor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Gray(f32),
    Rgb(u8, u8, u8),
    Cmyk(f32, f32, f32, f32),
}

/// ExtendedColor.normalize()
fn normalize(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

impl Color {
    /// new java.awt.Color(float r, float g, float b)
    pub fn rgb_f(r: f32, g: f32, b: f32) -> Color {
        let c = |v: f32| (normalize(v) * 255.0 + 0.5) as i32 as u8;
        Color::Rgb(c(r), c(g), c(b))
    }
    pub fn gray(g: f32) -> Color {
        Color::Gray(normalize(g))
    }
    pub fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Color {
        Color::Cmyk(normalize(c), normalize(m), normalize(y), normalize(k))
    }
    /// the java.awt.Color RGB components behind any colour
    fn rgb(&self) -> (u8, u8, u8) {
        match *self {
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Gray(g) => match Color::rgb_f(g, g, g) {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => unreachable!(),
            },
            Color::Cmyk(c, m, y, k) => match Color::rgb_f(1.0 - c - k, 1.0 - m - k, 1.0 - y - k) {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => unreachable!(),
            },
        }
    }
    /// java.awt.Color.darker()
    fn darker(&self) -> Color {
        let (r, g, b) = self.rgb();
        let d = |v: u8| ((v as f64 * 0.7) as i32).max(0) as u8;
        Color::Rgb(d(r), d(g), d(b))
    }
}

/// A PdfAppearance being written (PdfContentByte with separator ' ').
pub struct Canvas {
    pub buf: Vec<u8>,
    /// (resource name, font) pairs registered through setFontAndSize()
    pub fonts: Vec<(Vec<u8>, Font)>,
}

impl Canvas {
    fn new() -> Self {
        Canvas { buf: Vec::new(), fonts: Vec::new() }
    }
    fn op(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(b' ');
    }
    fn nums(&mut self, v: &[f32], op: &str) {
        for x in v {
            self.buf.extend_from_slice(fmt(*x).as_bytes());
            self.buf.push(b' ');
        }
        self.op(op);
    }
    fn save_state(&mut self) {
        self.op("q");
    }
    fn restore_state(&mut self) {
        self.op("Q");
    }
    fn set_color_fill(&mut self, c: &Color) {
        match *c {
            Color::Gray(g) => self.nums(&[g], "g"),
            Color::Cmyk(c, m, y, k) => self.nums(&[c, m, y, k], "k"),
            Color::Rgb(r, g, b) => self.nums(&[r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0], "rg"),
        }
    }
    fn set_color_stroke(&mut self, c: &Color) {
        match *c {
            Color::Gray(g) => self.nums(&[g], "G"),
            Color::Cmyk(c, m, y, k) => self.nums(&[c, m, y, k], "K"),
            Color::Rgb(r, g, b) => self.nums(&[r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0], "RG"),
        }
    }
    fn set_gray_fill(&mut self, g: f32) {
        self.nums(&[g], "g");
    }
    fn rectangle(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.nums(&[x, y, w, h], "re");
    }
    fn move_to(&mut self, x: f32, y: f32) {
        self.nums(&[x, y], "m");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.nums(&[x, y], "l");
    }
    fn set_line_width(&mut self, w: f32) {
        self.nums(&[w], "w");
    }
    fn set_text_matrix(&mut self, x: f32, y: f32) {
        self.nums(&[1.0, 0.0, 0.0, 1.0, x, y], "Tm");
    }
    /// PdfAppearance.setFontAndSize()
    fn set_font_and_size(&mut self, font: &Font, size: f32) {
        let name: Vec<u8> = match std_resource_name(&font.ps_name) {
            Some(n) => n.as_bytes().to_vec(),
            None => string_to_name(&font.ps_name),
        };
        if !self.fonts.iter().any(|(n, _)| *n == name) {
            self.fonts.push((name.clone(), font.clone()));
        }
        self.buf.extend_from_slice(&report::itext_name_bytes(&name));
        self.buf.push(b' ');
        self.buf.extend_from_slice(fmt(size).as_bytes());
        self.op(" Tf");
    }
    fn show_text(&mut self, font: &Font, units: &[u16]) {
        self.buf.extend_from_slice(&escape_string(&font.convert_to_bytes(units)));
        self.op("Tj");
    }
}

/// A finished appearance (PdfAppearance / PdfTemplate).
pub struct Appearance {
    pub content: Vec<u8>,
    pub width: f32,
    pub height: f32,
    pub matrix: [f32; 6],
    pub fonts: Vec<(Vec<u8>, Font)>,
}

/// TextField (with the BaseField state)
pub struct TextField {
    pub border_width: f32,
    pub border_style: i32,
    pub border_color: Option<Color>,
    pub background_color: Option<Color>,
    pub text_color: Option<Color>,
    pub font: Option<Font>,
    pub font_size: f32,
    pub text_leading: f32,
    pub alignment: i32,
    pub text: Option<String>,
    pub box_w: f32,
    pub box_h: f32,
    pub rotation: i32,
    pub options: i32,
    pub max_character_length: i32,
    pub choices: Vec<String>,
    pub choice_exports: Vec<String>,
    pub choice_selections: Vec<i32>,
    pub top_first: i32,
    pub extra_margin_left: f32,
    pub extra_margin_top: f32,
}

/// PdfChunk.noPrint()
fn no_print(c: u16) -> bool {
    (0x200b..=0x200f).contains(&c) || (0x202a..=0x202e).contains(&c)
}

/// DefaultSplitCharacter.isSplitCharacter()
fn is_split_character(c: u16) -> bool {
    if c <= b' ' as u16 || c == b'-' as u16 || c == 0x2010 {
        return true;
    }
    if c < 0x2002 {
        return false;
    }
    (0x2002..=0x200b).contains(&c)
        || (0x2e80..0xd7a0).contains(&c)
        || (0xf900..0xfb00).contains(&c)
        || (0xfe30..0xfe50).contains(&c)
        || (0xff61..0xffa0).contains(&c)
}

fn is_ws(c: u16) -> bool {
    c <= b' ' as u16
}

/// Character.isWhitespace() for a UTF-16 unit
fn java_is_whitespace(c: u16) -> bool {
    matches!(c, 9..=13 | 0x1c..=0x1f | 0x20 | 0x1680 | 0x180e | 0x2000..=0x2006 | 0x2008..=0x200a | 0x2028 | 0x2029 | 0x205f | 0x3000)
}

/// PdfLine
struct Line {
    text: Vec<u16>,
    /// widthLeft()
    width: f32,
    newline_split: bool,
}

/// BidiLine over a single-chunk phrase
struct BidiLine<'a> {
    font: &'a Font,
    size: f32,
    src: Vec<u16>,
    index: usize,
    exhausted: bool,
    text: Vec<u16>,
    current_char: usize,
}

impl<'a> BidiLine<'a> {
    fn new(font: &'a Font, size: f32, src: &[u16]) -> Self {
        BidiLine { font, size, src: src.to_vec(), index: 0, exhausted: false, text: Vec::new(), current_char: 0 }
    }

    fn is_empty(&self) -> bool {
        self.current_char >= self.text.len() && self.exhausted
    }

    fn char_width(&self, c: u16) -> f32 {
        if no_print(c) {
            0.0
        } else {
            self.font.width_point(c, self.size)
        }
    }

    fn get_paragraph(&mut self) -> bool {
        self.current_char = 0;
        self.text.clear();
        let mut has_text = false;
        if !self.exhausted {
            let len = self.src.len();
            while self.index < len {
                let c = self.src[self.index];
                if c == b'\r' as u16 || c == b'\n' as u16 {
                    if c == b'\r' as u16 && self.index + 1 < len && self.src[self.index + 1] == b'\n' as u16 {
                        self.index += 1;
                    }
                    self.index += 1;
                    if self.index >= len {
                        self.exhausted = true;
                    }
                    has_text = true;
                    break;
                }
                self.text.push(c);
                self.index += 1;
            }
            if !has_text {
                self.exhausted = true;
            }
        }
        if self.text.is_empty() {
            return has_text;
        }
        // remove trailing WS
        while let Some(&c) = self.text.last() {
            if is_ws(c) {
                self.text.pop();
            } else {
                break;
            }
        }
        if self.text.is_empty() {
            return true;
        }
        let n = (self.trim_right_ex(0, self.text.len() as i64 - 1) + 1) as usize;
        self.text.truncate(n);
        true
    }

    fn trim_right_ex(&self, start: i64, end: i64) -> i64 {
        let mut idx = end;
        while idx >= start {
            let c = self.text[idx as usize];
            if !is_ws(c) && !no_print(c) {
                break;
            }
            idx -= 1;
        }
        idx
    }

    fn trim_left_ex(&self, start: usize, end: i64) -> usize {
        let mut idx = start;
        while (idx as i64) <= end {
            let c = self.text[idx];
            if !is_ws(c) && !no_print(c) {
                break;
            }
            idx += 1;
        }
        idx
    }

    fn get_width(&self, start: i64, last: i64) -> f32 {
        let mut width = 0f32;
        let mut i = start;
        while i <= last {
            let c = self.text[i as usize];
            if !no_print(c) {
                width += self.char_width(c);
            }
            i += 1;
        }
        width
    }

    fn slice(&self, a: usize, b: i64) -> Vec<u16> {
        if b < a as i64 {
            Vec::new()
        } else {
            self.text[a..=(b as usize)].to_vec()
        }
    }

    /// BidiLine.processLine()
    fn process_line(&mut self, width: f32) -> Option<Line> {
        let mut width = width;
        if self.current_char >= self.text.len() {
            if !self.get_paragraph() {
                return None;
            }
            if self.text.is_empty() {
                return Some(Line { text: Vec::new(), width: 0.0, newline_split: true });
            }
        }
        let total = self.text.len();
        let original_width = width;
        let mut last_split: i64 = -1;
        if self.current_char != 0 {
            self.current_char = self.trim_left_ex(self.current_char, total as i64 - 1);
        }
        let old_current_char = self.current_char;
        let mut any_valid = false;
        while self.current_char < total {
            let c = self.text[self.current_char];
            if no_print(c) {
                self.current_char += 1;
                continue;
            }
            let char_width = self.char_width(c);
            let split_char = is_split_character(c);
            if split_char && java_is_whitespace(c) {
                last_split = self.current_char as i64;
            }
            if width - char_width < 0.0 {
                break;
            }
            if split_char {
                last_split = self.current_char as i64;
            }
            width -= char_width;
            any_valid = true;
            self.current_char += 1;
        }
        if !any_valid {
            // not even a single char fit; must output the first char
            self.current_char += 1;
            let i = (self.current_char - 1).min(total - 1);
            return Some(Line { text: vec![self.text[i]], width: 0.0, newline_split: false });
        }
        if self.current_char >= total {
            // there was more line than text
            return Some(Line { text: self.slice(old_current_char, total as i64 - 1), width, newline_split: true });
        }
        let cc = self.current_char as i64;
        let mut new_current_char = self.trim_right_ex(old_current_char as i64, cc - 1);
        if new_current_char < old_current_char as i64 {
            // only WS
            return Some(Line { text: self.slice(old_current_char, cc - 1), width, newline_split: false });
        }
        if last_split == -1 || last_split >= new_current_char {
            // no split point or split point ahead of end
            return Some(Line {
                text: self.slice(old_current_char, new_current_char),
                width: width + self.get_width(new_current_char + 1, cc - 1),
                newline_split: false,
            });
        }
        // standard split
        self.current_char = (last_split + 1) as usize;
        new_current_char = self.trim_right_ex(old_current_char as i64, last_split);
        if new_current_char < old_current_char as i64 {
            // only WS again
            new_current_char = self.current_char as i64 - 1;
        }
        Some(Line {
            text: self.slice(old_current_char, new_current_char),
            width: original_width - self.get_width(old_current_char as i64, new_current_char),
            newline_split: false,
        })
    }
}

const NO_MORE_TEXT: i32 = 1;
const NO_MORE_COLUMN: i32 = 2;

/// ColumnText in rectangular mode holding one single-font phrase.
struct ColumnText<'a> {
    bidi: BidiLine<'a>,
    color: Color,
    left_x: f32,
    min_y: f32,
    max_y: f32,
    y_line: f32,
    rectangular_width: f32,
    fixed_leading: f32,
    alignment: i32,
}

impl<'a> ColumnText<'a> {
    fn new(font: &'a Font, size: f32, color: Color, text: &[u16]) -> Self {
        ColumnText {
            bidi: BidiLine::new(font, size, text),
            color,
            left_x: 0.0,
            min_y: 0.0,
            max_y: 0.0,
            y_line: 0.0,
            rectangular_width: 0.0,
            fixed_leading: 16.0,
            alignment: ALIGN_LEFT,
        }
    }

    fn set_simple_column(&mut self, llx: f32, lly: f32, urx: f32, ury: f32) {
        self.left_x = llx.min(urx);
        self.max_y = lly.max(ury);
        self.min_y = lly.min(ury);
        let right_x = llx.max(urx);
        self.y_line = self.max_y;
        self.rectangular_width = right_x - self.left_x;
        if self.rectangular_width < 0.0 {
            self.rectangular_width = 0.0;
        }
    }

    /// ColumnText.go(simulate); `canvas` None means simulate.
    fn go(&mut self, mut canvas: Option<&mut Canvas>) -> i32 {
        let mut dirty = false;
        let mut font_set = false;
        let status;
        loop {
            if self.rectangular_width <= 0.0 {
                status = NO_MORE_COLUMN | if self.bidi.is_empty() { NO_MORE_TEXT } else { 0 };
                break;
            }
            if self.bidi.is_empty() {
                status = NO_MORE_TEXT;
                break;
            }
            let line = match self.bidi.process_line(self.rectangular_width) {
                Some(l) => l,
                None => {
                    status = NO_MORE_TEXT;
                    break;
                }
            };
            let current_leading = self.fixed_leading;
            if self.y_line > self.max_y || self.y_line - current_leading < self.min_y {
                status = NO_MORE_COLUMN;
                break;
            }
            self.y_line -= current_leading;
            if let Some(cv) = canvas.as_deref_mut() {
                if !dirty {
                    cv.op("BT");
                    dirty = true;
                }
                // PdfLine.indentLeft()
                let indent = match self.alignment {
                    ALIGN_RIGHT => line.width,
                    ALIGN_CENTER => line.width / 2.0,
                    _ => 0.0,
                };
                cv.set_text_matrix(self.left_x + indent, self.y_line);
                // PdfDocument.writeLineToContent()
                if !font_set {
                    cv.set_font_and_size(self.bidi.font, self.bidi.size);
                    font_set = true;
                }
                cv.set_color_fill(&self.color);
                cv.show_text(self.bidi.font, &line.text);
                cv.set_gray_fill(0.0); // resetRGBColorFill()
            }
            let _ = line.newline_split;
        }
        if dirty {
            if let Some(cv) = canvas {
                cv.op("ET");
            }
        }
        status
    }
}

/// ColumnText.getWidth(phrase, runDirection, arabicOptions)
fn phrase_width(font: &Font, size: f32, text: &[u16]) -> f32 {
    let mut bidi = BidiLine::new(font, size, text);
    match bidi.process_line(20000.0) {
        Some(line) => 20000.0 - line.width,
        None => 0.0,
    }
}

/// ColumnText.showTextAligned(canvas, alignment, phrase, x, y, 0, runDirection, 0)
fn show_text_aligned(cv: &mut Canvas, alignment: i32, font: &Font, size: f32, color: Color, text: &[u16], x: f32, y: f32) {
    cv.save_state();
    let (mut llx, mut urx): (f32, f32) = match alignment {
        ALIGN_LEFT => (0.0, 20000.0),
        ALIGN_RIGHT => (-20000.0, 0.0),
        _ => (-20000.0, 20000.0),
    };
    let mut lly = -1f32;
    let mut ury = 2f32;
    llx += x;
    lly += y;
    urx += x;
    ury += y;
    let mut ct = ColumnText::new(font, size, color, text);
    ct.set_simple_column(llx, lly, urx, ury);
    ct.fixed_leading = 2.0;
    ct.alignment = alignment;
    ct.go(Some(cv));
    cv.restore_state();
}

/// TextField.removeCRLF()
fn remove_crlf(text: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(text.len());
    let mut k = 0;
    while k < text.len() {
        let c = text[k];
        if c == b'\n' as u16 {
            out.push(b' ' as u16);
        } else if c == b'\r' as u16 {
            out.push(b' ' as u16);
            if k + 1 < text.len() && text[k + 1] == b'\n' as u16 {
                k += 1;
            }
        } else {
            out.push(c);
        }
        k += 1;
    }
    out
}

impl TextField {
    /// `new TextField(writer, null, null)` followed by AcroFields' setBorderWidth(0)
    pub fn new() -> Self {
        TextField {
            border_width: 0.0,
            border_style: STYLE_SOLID,
            border_color: None,
            background_color: None,
            text_color: None,
            font: None,
            font_size: 0.0,
            text_leading: 0.0,
            alignment: ALIGN_LEFT,
            text: None,
            box_w: 0.0,
            box_h: 0.0,
            rotation: 0,
            options: 0,
            max_character_length: 0,
            choices: Vec::new(),
            choice_exports: Vec::new(),
            choice_selections: Vec::new(),
            top_first: 0,
            extra_margin_left: 0.0,
            extra_margin_top: 0.0,
        }
    }

    /// BaseField.getRealFont()
    fn real_font(&self) -> Font {
        match &self.font {
            Some(f) => f.clone(),
            None => Font::builtin("Helvetica").expect("built-in Helvetica metrics"),
        }
    }

    fn matrix(&self) -> [f32; 6] {
        match self.rotation {
            90 => [0.0, 1.0, -1.0, 0.0, self.box_h, 0.0],
            180 => [-1.0, 0.0, 0.0, -1.0, self.box_w, self.box_h],
            270 => [0.0, -1.0, 1.0, 0.0, 0.0, self.box_w],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    fn draw_top_frame(&self, app: &mut Canvas) {
        let (bw, w, h) = (self.border_width, self.box_w, self.box_h);
        app.move_to(bw, bw);
        app.line_to(bw, h - bw);
        app.line_to(w - bw, h - bw);
        app.line_to(w - 2.0 * bw, h - 2.0 * bw);
        app.line_to(2.0 * bw, h - 2.0 * bw);
        app.line_to(2.0 * bw, 2.0 * bw);
        app.line_to(bw, bw);
        app.op("f");
    }

    fn draw_bottom_frame(&self, app: &mut Canvas) {
        let (bw, w, h) = (self.border_width, self.box_w, self.box_h);
        app.move_to(bw, bw);
        app.line_to(w - bw, bw);
        app.line_to(w - bw, h - bw);
        app.line_to(w - 2.0 * bw, h - 2.0 * bw);
        app.line_to(w - 2.0 * bw, 2.0 * bw);
        app.line_to(2.0 * bw, 2.0 * bw);
        app.line_to(bw, bw);
        app.op("f");
    }

    /// BaseField.getBorderAppearance()
    fn border_appearance(&self) -> Canvas {
        let mut app = Canvas::new();
        let (bw, w, h) = (self.border_width, self.box_w, self.box_h);
        app.save_state();
        if let Some(bg) = &self.background_color {
            app.set_color_fill(bg);
            app.rectangle(0.0, 0.0, w, h);
            app.op("f");
        }
        let stroke_box = |app: &mut Canvas, bc: &Color| {
            app.set_color_stroke(bc);
            app.set_line_width(bw);
            app.rectangle(bw / 2.0, bw / 2.0, w - bw, h - bw);
            app.op("S");
        };
        match self.border_style {
            STYLE_UNDERLINE => {
                if let (true, Some(bc)) = (bw != 0.0, &self.border_color) {
                    app.set_color_stroke(bc);
                    app.set_line_width(bw);
                    app.move_to(0.0, bw / 2.0);
                    app.line_to(w, bw / 2.0);
                    app.op("S");
                }
            }
            STYLE_BEVELED => {
                if let (true, Some(bc)) = (bw != 0.0, &self.border_color) {
                    stroke_box(&mut app, bc);
                }
                let actual = self.background_color.unwrap_or(Color::Rgb(255, 255, 255));
                app.set_gray_fill(1.0);
                self.draw_top_frame(&mut app);
                app.set_color_fill(&actual.darker());
                self.draw_bottom_frame(&mut app);
            }
            STYLE_INSET => {
                if let (true, Some(bc)) = (bw != 0.0, &self.border_color) {
                    stroke_box(&mut app, bc);
                }
                app.set_gray_fill(0.5);
                self.draw_top_frame(&mut app);
                app.set_gray_fill(0.75);
                self.draw_bottom_frame(&mut app);
            }
            _ => {
                if let (true, Some(bc)) = (bw != 0.0, &self.border_color) {
                    if self.border_style == STYLE_DASHED {
                        app.op("[3] 0 d"); // setLineDash(3, 0)
                    }
                    stroke_box(&mut app, bc);
                    if (self.options & FF_COMB) != 0 && self.max_character_length > 1 {
                        let step = w / self.max_character_length as f32;
                        let yb = bw / 2.0;
                        let yt = h - bw / 2.0;
                        for k in 1..self.max_character_length {
                            let x = step * k as f32;
                            app.move_to(x, yb);
                            app.line_to(x, yt);
                        }
                        app.op("S");
                    }
                }
            }
        }
        app.restore_state();
        app
    }

    fn finish(&self, app: Canvas) -> Appearance {
        Appearance { content: app.buf, width: self.box_w, height: self.box_h, matrix: self.matrix(), fonts: app.fonts }
    }

    /// TextField.getAppearance()
    pub fn get_appearance(&self) -> Appearance {
        let mut app = self.border_appearance();
        app.op("/Tx BMC");
        let text: Vec<u16> = self.text.as_deref().unwrap_or("").encode_utf16().collect();
        if text.is_empty() {
            app.op("EMC");
            return self.finish(app);
        }
        let border_extra = self.border_style == STYLE_BEVELED || self.border_style == STYLE_INSET;
        let mut h = self.box_h - self.border_width * 2.0 - self.extra_margin_top;
        let mut bw2 = self.border_width;
        if border_extra {
            h -= self.border_width * 2.0;
            bw2 *= 2.0;
        }
        let offset_x = bw2.max(1.0);
        let off_x = bw2.min(offset_x);
        app.save_state();
        app.rectangle(off_x, off_x, self.box_w - 2.0 * off_x, self.box_h - 2.0 * off_x);
        app.op("W");
        app.op("n");
        let ptext: Vec<u16> = if (self.options & FF_PASSWORD) != 0 {
            vec![b'*' as u16; text.len()]
        } else if (self.options & FF_MULTILINE) == 0 {
            remove_crlf(&text)
        } else {
            text
        };
        let ufont = self.real_font();
        let fcolor = self.text_color.unwrap_or(Color::Gray(0.0));
        let mut usize = self.font_size;
        if (self.options & FF_MULTILINE) != 0 {
            let width = self.box_w - 4.0 * offset_x - self.extra_margin_left;
            let mut factor = ufont.descriptor(Desc::BboxUry, 1.0) - ufont.descriptor(Desc::BboxLly, 1.0);
            if usize == 0.0 {
                usize = h / factor;
                if usize > 4.0 {
                    if usize > 12.0 {
                        usize = 12.0;
                    }
                    let step = ((usize - 4.0) / 10.0).max(0.2);
                    while usize > 4.0 {
                        let mut ct = ColumnText::new(&ufont, usize, fcolor, &ptext);
                        ct.set_simple_column(0.0, -h, width, 0.0);
                        ct.alignment = self.alignment;
                        ct.y_line = 0.0;
                        ct.fixed_leading = factor * usize;
                        let status = ct.go(None);
                        if (status & NO_MORE_COLUMN) == 0 {
                            break;
                        }
                        usize -= step;
                    }
                }
                if usize < 4.0 {
                    usize = 4.0;
                }
            }
            if self.text_leading != 0.0 {
                factor = self.text_leading / usize;
            }
            let leading = usize * factor;
            let offset_y = offset_x + h - ufont.descriptor(Desc::BboxUry, usize);
            let mut ct = ColumnText::new(&ufont, usize, fcolor, &ptext);
            ct.set_simple_column(
                self.extra_margin_left + 2.0 * offset_x,
                -20000.0,
                self.box_w - 2.0 * offset_x,
                offset_y + leading,
            );
            ct.fixed_leading = leading;
            ct.alignment = self.alignment;
            ct.go(Some(&mut app));
        } else {
            if usize == 0.0 {
                let max_calculated_size = h / (ufont.descriptor(Desc::BboxUrx, 1.0) - ufont.descriptor(Desc::BboxLly, 1.0));
                let wd = phrase_width(&ufont, 1.0, &ptext);
                if wd == 0.0 {
                    usize = max_calculated_size;
                } else {
                    usize = max_calculated_size.min((self.box_w - self.extra_margin_left - 4.0 * offset_x) / wd);
                }
                if usize < 4.0 {
                    usize = 4.0;
                }
            }
            let ascent = ufont.descriptor(Desc::Ascent, usize);
            let descent = ufont.descriptor(Desc::Descent, usize);
            let mut offset_y = off_x + ((self.box_h - 2.0 * off_x) - ascent) / 2.0;
            if offset_y < off_x {
                offset_y = off_x;
            }
            if offset_y - off_x < -descent {
                let ny = -descent + off_x;
                let dy = self.box_h - off_x - ascent;
                offset_y = ny.min(offset_y.max(dy));
            }
            if (self.options & FF_COMB) != 0 && self.max_character_length > 0 {
                let text_len = self.max_character_length.min(ptext.len() as i32);
                let mut position = 0;
                if self.alignment == ALIGN_RIGHT {
                    position = self.max_character_length - text_len;
                } else if self.alignment == ALIGN_CENTER {
                    position = (self.max_character_length - text_len) / 2;
                }
                let step = (self.box_w - self.extra_margin_left) / self.max_character_length as f32;
                let mut start = step / 2.0 + position as f32 * step;
                match &self.text_color {
                    None => app.set_gray_fill(0.0),
                    Some(c) => app.set_color_fill(c),
                }
                app.op("BT");
                app.set_font_and_size(&ufont, usize);
                for &c in &ptext {
                    let wd = ufont.width_str(&[c]) as f32 * 0.001f32 * usize;
                    app.set_text_matrix(self.extra_margin_left + start - wd / 2.0, offset_y - self.extra_margin_top);
                    app.show_text(&ufont, &[c]);
                    start += step;
                }
                app.op("ET");
            } else {
                let x = match self.alignment {
                    ALIGN_RIGHT => self.extra_margin_left + self.box_w - (2.0 * offset_x),
                    ALIGN_CENTER => self.extra_margin_left + (self.box_w / 2.0),
                    _ => self.extra_margin_left + (2.0 * offset_x),
                };
                show_text_aligned(&mut app, self.alignment, &ufont, usize, fcolor, &ptext, x, offset_y - self.extra_margin_top);
            }
        }
        app.restore_state();
        app.op("EMC");
        self.finish(app)
    }

    /// TextField.getTopChoice()
    fn top_choice(&self) -> i32 {
        match self.choice_selections.first() {
            None => 0,
            Some(first) => (*first).min(self.choices.len() as i32).max(0),
        }
    }

    /// TextField.getListAppearance(); sets `top_first`.
    pub fn get_list_appearance(&mut self) -> Appearance {
        let mut app = self.border_appearance();
        if self.choices.is_empty() {
            return self.finish(app);
        }
        app.op("/Tx BMC");
        let top_choice = self.top_choice();
        let ufont = self.real_font();
        let mut usize = self.font_size;
        if usize == 0.0 {
            usize = 12.0;
        }
        let border_extra = self.border_style == STYLE_BEVELED || self.border_style == STYLE_INSET;
        let mut h = self.box_h - self.border_width * 2.0;
        let mut offset_x = self.border_width;
        if border_extra {
            h -= self.border_width * 2.0;
            offset_x *= 2.0;
        }
        let leading = ufont.descriptor(Desc::BboxUry, usize) - ufont.descriptor(Desc::BboxLly, usize);
        // (Java int arithmetic wraps)
        let max_fit = ((h / leading) as i32).wrapping_add(1);
        let mut first = top_choice;
        let mut last = first.wrapping_add(max_fit);
        if last > self.choices.len() as i32 {
            last = self.choices.len() as i32;
            first = last.wrapping_sub(max_fit).max(0);
        }
        self.top_first = first;
        app.save_state();
        app.rectangle(offset_x, offset_x, self.box_w - 2.0 * offset_x, self.box_h - 2.0 * offset_x);
        app.op("W");
        app.op("n");
        let fcolor = self.text_color.unwrap_or(Color::Gray(0.0));
        // background boxes for selected value[s]
        app.set_color_fill(&Color::Rgb(10, 36, 106));
        for &cur_choice in &self.choice_selections {
            if cur_choice >= first && cur_choice <= last {
                app.rectangle(
                    offset_x,
                    offset_x + h - cur_choice.wrapping_sub(first).wrapping_add(1) as f32 * leading,
                    self.box_w - 2.0 * offset_x,
                    leading,
                );
                app.op("f");
            }
        }
        let xp = offset_x * 2.0;
        let mut yp = offset_x + h - ufont.descriptor(Desc::BboxUry, usize);
        for idx in first..last {
            let ptext: Vec<u16> = self.choices[idx as usize].encode_utf16().collect();
            let ptext = remove_crlf(&ptext);
            // highlight selected values against their (presumably) darker background
            let text_col = if self.choice_selections.contains(&idx) { Color::Gray(1.0) } else { fcolor };
            show_text_aligned(&mut app, ALIGN_LEFT, &ufont, usize, text_col, &ptext, xp, yp);
            yp -= leading;
        }
        app.restore_state();
        app.op("EMC");
        self.finish(app)
    }

    /// TextField.setChoiceSelections()
    pub fn set_choice_selections(&mut self, selections: Vec<i32>) {
        self.choice_selections = selections;
        if self.choice_selections.len() > 1 && (self.options & FF_MULTISELECT) == 0 {
            // can't have multiple selections in a single-select field
            self.choice_selections.truncate(1);
        }
    }
}

/// AcroFields.splitDAelements(): (font name, size, colour, leading)
pub fn split_da_elements(da: &str) -> (Option<String>, Option<f32>, Option<Color>, Option<f32>) {
    let bytes: Vec<u8> = da.encode_utf16().map(|u| u as u8).collect();
    let mut lx = Lexer::new(&bytes);
    let mut stack: Vec<String> = Vec::new();
    let mut ret = (None, None, None, None);
    // `new Float(String)`; a malformed operand is a NumberFormatException in the Java
    let f = |s: &String| -> f32 { s.trim().parse::<f32>().unwrap_or(0.0) };
    loop {
        let tok = match lx.next_token() {
            Ok(Some(t)) => t,
            _ => break,
        };
        match tok {
            Token::Comment => continue,
            Token::Other(operator) => {
                let n = stack.len();
                match operator.as_str() {
                    "Tf" if n >= 2 => {
                        ret.0 = Some(stack[n - 2].clone());
                        ret.1 = Some(f(&stack[n - 1]));
                    }
                    "g" if n >= 1 => {
                        let gray = f(&stack[n - 1]);
                        if gray != 0.0 {
                            ret.2 = Some(Color::gray(gray));
                        }
                    }
                    "rg" if n >= 3 => {
                        ret.2 = Some(Color::rgb_f(f(&stack[n - 3]), f(&stack[n - 2]), f(&stack[n - 1])));
                    }
                    "k" if n >= 4 => {
                        ret.2 = Some(Color::cmyk(f(&stack[n - 4]), f(&stack[n - 3]), f(&stack[n - 2]), f(&stack[n - 1])));
                    }
                    "TL" if n >= 1 => {
                        ret.3 = Some(f(&stack[n - 1]));
                    }
                    _ => {}
                }
                stack.clear();
            }
            t => stack.push(t.string_value()),
        }
    }
    ret
}

/// AcroFields.getMKColor()
pub fn mk_color(doc: &Document, ar: Option<&Vec<Object>>) -> Option<Color> {
    let ar = ar?;
    let g = |i: usize| res(doc, Some(&ar[i])).map(float_value).unwrap_or(0.0);
    match ar.len() {
        1 => Some(Color::gray(g(0))),
        3 => Some(Color::rgb_f(g(0), g(1), g(2))),
        4 => Some(Color::cmyk(g(0), g(1), g(2), g(3))),
        _ => None,
    }
}

/// AcroFields.getDefaultAppearance()
pub fn default_appearance(doc: &Document, dict: &Dictionary) -> Option<String> {
    match dget(doc, dict, b"DA") {
        Some(Object::String(s, _)) => Some(text(s)),
        Some(Object::Name(n)) => {
            eprintln!("Warning: input does not conform to the PDF standard.");
            Some(name_to_string(n))
        }
        _ => None,
    }
}
