use std::str::FromStr;

use log::debug;
use svgtypes::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    None,
    Solid(Rgb),
    Gradient(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// A resolved SVG paint/style set. Values are in SVG user units; scaling to
/// CeTZ/Typst units happens at emit time.
#[derive(Debug, Clone)]
pub struct Style {
    pub fill: Paint,
    pub fill_opacity: f64,
    pub fill_rule: FillRule,
    pub stroke: Paint,
    pub stroke_opacity: f64,
    /// user units; `None` means the SVG default of 1
    pub stroke_width: Option<f64>,
    pub dash: Option<String>,
    pub linecap: Option<String>,
    pub linejoin: Option<String>,
    pub miterlimit: Option<f64>,
    pub font_family: Option<String>,
    /// user units
    pub font_size: Option<f64>,
    pub text_anchor: TextAnchor,
    pub dominant_baseline: Option<String>,
    /// element-level opacity; NOT inherited per SVG spec
    pub opacity: f64,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill: Paint::Solid(Rgb { r: 0, g: 0, b: 0 }),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            stroke: Paint::None,
            stroke_opacity: 1.0,
            stroke_width: None,
            dash: None,
            linecap: None,
            linejoin: None,
            miterlimit: None,
            font_family: None,
            font_size: None,
            text_anchor: TextAnchor::Start,
            dominant_baseline: None,
            opacity: 1.0,
        }
    }
}

impl Style {
    /// Child style: everything inherits except element-level `opacity`.
    pub fn inherit(&self) -> Style {
        let mut c = self.clone();
        c.opacity = 1.0;
        c
    }

    /// Apply a single `key: value` pair from either a `style=""` attribute or a
    /// presentation attribute. Returns true when the key was recognised.
    pub fn apply(&mut self, key: &str, value: &str) -> bool {
        let value = value.trim();
        match key {
            "fill" => self.fill = parse_paint(value),
            "fill-opacity" => self.fill_opacity = parse_frac(value),
            "fill-rule" => {
                self.fill_rule = if value == "evenodd" {
                    FillRule::EvenOdd
                } else {
                    FillRule::NonZero
                }
            }
            "stroke" => self.stroke = parse_paint(value),
            "stroke-opacity" => self.stroke_opacity = parse_frac(value),
            "stroke-width" => self.stroke_width = parse_number(value),
            "stroke-dasharray" => {
                if value != "none" {
                    self.dash = Some(value.to_string())
                }
            }
            "stroke-linecap" => self.linecap = Some(value.to_string()),
            "stroke-linejoin" => self.linejoin = Some(value.to_string()),
            "stroke-miterlimit" => self.miterlimit = parse_number(value),
            "font-family" => self.font_family = Some(value.to_string()),
            "font-size" => self.font_size = parse_number(value),
            "text-anchor" => {
                self.text_anchor = match value {
                    "middle" => TextAnchor::Middle,
                    "end" => TextAnchor::End,
                    _ => TextAnchor::Start,
                }
            }
            "dominant-baseline" => self.dominant_baseline = Some(value.to_string()),
            "opacity" => self.opacity = parse_frac(value),
            _ => {
                debug!("unprocessed style key: {}={}", key, value);
                return false;
            }
        }
        true
    }

    /// Parse a `style="k:v;k:v"` declaration list. `style=` wins over
    /// presentation attributes, so call this after applying those.
    pub fn apply_style_str(&mut self, s: &str) {
        for kv in s.split(';') {
            let kv = kv.trim();
            if kv.is_empty() {
                continue;
            }
            match kv.split_once(':') {
                Some((k, v)) => {
                    self.apply(k.trim(), v);
                }
                None => debug!("malformed style pair: {}", kv),
            }
        }
    }
}

pub fn parse_paint(value: &str) -> Paint {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") {
        return Paint::None;
    }
    if let Some(id) = v
        .strip_prefix("url(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let id = id.trim().trim_start_matches('#').trim_matches('\'').trim_matches('"');
        return Paint::Gradient(id.to_string());
    }
    match Color::from_str(v) {
        Ok(c) => Paint::Solid(Rgb {
            r: c.red,
            g: c.green,
            b: c.blue,
        }),
        Err(e) => {
            debug!("unparseable paint {:?}: {}", v, e);
            Paint::None
        }
    }
}

/// Read the leading numeric part of a CSS length, ignoring any unit suffix
/// (`px`, `pt`, `%`, ...). Values are kept in user units.
pub fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let end = s
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+' || *c == 'e' || *c == 'E')
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return None;
    }
    f64::from_str(&s[..end]).ok()
}

fn parse_frac(s: &str) -> f64 {
    parse_number(s).unwrap_or(1.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paints() {
        assert_eq!(parse_paint("none"), Paint::None);
        assert_eq!(
            parse_paint("#ff0000"),
            Paint::Solid(Rgb { r: 255, g: 0, b: 0 })
        );
        assert_eq!(parse_paint("red"), Paint::Solid(Rgb { r: 255, g: 0, b: 0 }));
        assert_eq!(
            parse_paint("rgb(35,31,32)"),
            Paint::Solid(Rgb { r: 35, g: 31, b: 32 })
        );
        assert_eq!(parse_paint("url(#g1)"), Paint::Gradient("g1".into()));
    }

    #[test]
    fn numbers_ignore_units() {
        assert_eq!(parse_number("12px"), Some(12.0));
        assert_eq!(parse_number("1.5pt"), Some(1.5));
        assert_eq!(parse_number("2"), Some(2.0));
        assert_eq!(parse_number("-3.25"), Some(-3.25));
        assert_eq!(parse_number("abc"), None);
    }

    #[test]
    fn defaults_match_svg() {
        let s = Style::default();
        assert_eq!(s.fill, Paint::Solid(Rgb { r: 0, g: 0, b: 0 }));
        assert_eq!(s.stroke, Paint::None);
        assert_eq!(s.fill_rule, FillRule::NonZero);
    }

    #[test]
    fn opacity_is_not_inherited() {
        let mut s = Style::default();
        s.opacity = 0.5;
        s.fill_opacity = 0.25;
        let child = s.inherit();
        assert_eq!(child.opacity, 1.0);
        assert_eq!(child.fill_opacity, 0.25);
    }

    #[test]
    fn style_str_wins_and_parses() {
        let mut s = Style::default();
        s.apply("fill", "blue");
        s.apply_style_str("fill:#00ff00;stroke-width:2px;fill-rule:evenodd");
        assert_eq!(s.fill, Paint::Solid(Rgb { r: 0, g: 255, b: 0 }));
        assert_eq!(s.stroke_width, Some(2.0));
        assert_eq!(s.fill_rule, FillRule::EvenOdd);
    }
}
