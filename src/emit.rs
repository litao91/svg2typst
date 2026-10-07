use crate::gradient::{resolve_paint, GradientMap};
use crate::path::{Cmd, Pt, SubPath};
use crate::style::{FillRule, Paint, Style, TextAnchor};

/// Escape SVG text for a Typst markup block. `/` and `&` are not special in
/// Typst markup and are left alone; `\` must go first to avoid double-escaping.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '#' | '[' | ']' | '$' | '_' | '*' | '<' | '~' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn num(v: f64) -> String {
    let v = if v.abs() < 5e-4 { 0.0 } else { v };
    let s = format!("{:.3}", v);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn point(p: Pt) -> String {
    format!("({}, {})", num(p.0), num(p.1))
}

fn cmd_tuple(c: &Cmd) -> String {
    match c {
        Cmd::M(p) => format!("(\"M\", {})", point(*p)),
        Cmd::L(p) => format!("(\"L\", {})", point(*p)),
        Cmd::C(a, b, p) => format!("(\"C\", {}, {}, {})", point(*a), point(*b), point(*p)),
        Cmd::Q(a, p) => format!("(\"Q\", {}, {})", point(*a), point(*p)),
    }
}

const CLOSE: &str = "(\"z\", )";

pub struct Emitter {
    pub out: String,
    pub gradients: GradientMap,
    pub px_scale: f64,
    pub font_scale: f64,
}

impl Emitter {
    pub fn new(gradients: GradientMap, px_scale: f64, font_scale: f64) -> Self {
        Emitter {
            out: String::new(),
            gradients,
            px_scale,
            font_scale,
        }
    }

    fn has_fill(style: &Style) -> bool {
        !matches!(style.fill, Paint::None)
    }

    fn has_stroke(style: &Style) -> bool {
        !matches!(style.stroke, Paint::None)
            || style.stroke_width.is_some()
            || style.dash.is_some()
    }

    fn fill_frag(&self, style: &Style) -> String {
        let alpha = style.opacity * style.fill_opacity;
        format!("fill: {}, ", resolve_paint(&style.fill, &self.gradients, alpha))
    }

    fn fill_rule_frag(&self, style: &Style) -> String {
        match style.fill_rule {
            FillRule::EvenOdd => "fill-rule: \"even-odd\", ".to_string(),
            FillRule::NonZero => String::new(),
        }
    }

    fn dash_frag(&self, style: &Style, m: f64) -> String {
        let dash = match &style.dash {
            Some(d) => d,
            None => return String::new(),
        };
        let mut vals = Vec::new();
        for tok in dash.split_whitespace() {
            match crate::style::parse_number(tok) {
                Some(n) => vals.push(format!("{}pt", num(n * self.px_scale * m))),
                None => return "dash: \"dashed\", ".to_string(),
            }
        }
        if vals.is_empty() {
            String::new()
        } else {
            format!("dash: ({}, ), ", vals.join(", "))
        }
    }

    fn cap_join_frag(&self, style: &Style) -> String {
        let mut s = String::new();
        if let Some(cap) = &style.linecap {
            let cap = match cap.as_str() {
                "butt" | "round" | "square" => cap,
                _ => "butt",
            };
            s.push_str(&format!("cap: \"{}\", ", cap));
        }
        if let Some(join) = &style.linejoin {
            let join = match join.as_str() {
                "miter" | "round" | "bevel" => join,
                _ => "miter",
            };
            s.push_str(&format!("join: \"{}\", ", join));
        }
        if let Some(ml) = style.miterlimit {
            s.push_str(&format!("miter-limit: {}, ", num(ml)));
        }
        s
    }

    fn stroke_frag(&self, style: &Style, m: f64) -> String {
        let alpha = style.opacity * style.stroke_opacity;
        let paint = resolve_paint(&style.stroke, &self.gradients, alpha);
        if paint == "none" && style.stroke_width.is_none() && style.dash.is_none() {
            return "stroke: none, ".to_string();
        }
        let mut inner = String::new();
        if paint != "none" {
            inner.push_str(&format!("paint: {}, ", paint));
        }
        let width = style.stroke_width.unwrap_or(1.0);
        inner.push_str(&format!("thickness: {}pt, ", num(width * self.px_scale * m)));
        inner.push_str(&self.dash_frag(style, m));
        inner.push_str(&self.cap_join_frag(style));
        format!("stroke: ({}), ", inner)
    }

    /// The `fill:`/`stroke:`/`fill-rule:` named arguments for a shape call.
    fn shape_style(&self, style: &Style, m: f64, want_fill: bool, want_stroke: bool) -> String {
        let mut s = String::new();
        if want_fill {
            s.push_str(&self.fill_frag(style));
            s.push_str(&self.fill_rule_frag(style));
        } else {
            s.push_str("fill: none, ");
        }
        if want_stroke {
            s.push_str(&self.stroke_frag(style, m));
        } else {
            s.push_str("stroke: none, ");
        }
        s
    }

    fn push_svg_path(&mut self, cmds: &[String], style_frag: &str) {
        self.out
            .push_str(&format!("svg-path({}, {})\n", cmds.join(", "), style_frag));
    }

    /// Emit CeTZ for a set of subpaths.
    ///
    /// Filled shapes get every subpath in a single `svg-path` (closed, per SVG
    /// fill semantics) so `fill-rule` holes render. Stroked shapes get one
    /// `svg-path` per subpath so open contours stay open and disjoint
    /// subpaths are not collapsed together by CeTZ's `M`-without-`z` quirk.
    pub fn emit_subpaths(&mut self, subpaths: &[SubPath], style: &Style, m: f64) {
        if subpaths.is_empty() {
            return;
        }
        let fill = Self::has_fill(style);
        let stroke = Self::has_stroke(style);
        let all_closed = subpaths.iter().all(|s| s.closed);

        if fill {
            let mut cmds = Vec::new();
            for sp in subpaths {
                cmds.extend(sp.cmds.iter().map(cmd_tuple));
                cmds.push(CLOSE.to_string());
            }
            let frag = self.shape_style(style, m, true, stroke && all_closed);
            self.push_svg_path(&cmds, &frag);
        }

        if stroke && !(fill && all_closed) {
            for sp in subpaths {
                let mut cmds: Vec<String> = sp.cmds.iter().map(cmd_tuple).collect();
                if sp.closed {
                    cmds.push(CLOSE.to_string());
                }
                let frag = self.shape_style(style, m, false, true);
                self.push_svg_path(&cmds, &frag);
            }
        } else if !fill && !stroke {
            let mut cmds = Vec::new();
            for sp in subpaths {
                cmds.extend(sp.cmds.iter().map(cmd_tuple));
                if sp.closed {
                    cmds.push(CLOSE.to_string());
                }
            }
            let frag = self.shape_style(style, m, false, false);
            self.push_svg_path(&cmds, &frag);
        }
    }

    pub fn emit_circle(&mut self, center: Pt, radius: f64, style: &Style, m: f64) {
        let frag = self.shape_style(style, m, Self::has_fill(style), Self::has_stroke(style));
        self.out.push_str(&format!(
            "circle({}, radius: {}, {})\n",
            point(center),
            num(radius * m),
            frag
        ));
    }

    pub fn emit_rect(&mut self, a: Pt, b: Pt, style: &Style, m: f64) {
        let frag = self.shape_style(style, m, Self::has_fill(style), Self::has_stroke(style));
        self.out
            .push_str(&format!("rect({}, {}, {})\n", point(a), point(b), frag));
    }

    fn anchor_for(style: &Style) -> &'static str {
        let mid = matches!(
            style.dominant_baseline.as_deref(),
            Some("central") | Some("middle")
        );
        match (style.text_anchor, mid) {
            (TextAnchor::Start, false) => "base-west",
            (TextAnchor::Middle, false) => "base",
            (TextAnchor::End, false) => "base-east",
            (TextAnchor::Start, true) => "mid-west",
            (TextAnchor::Middle, true) => "mid",
            (TextAnchor::End, true) => "mid-east",
        }
    }

    fn font_frag(&self, style: &Style) -> String {
        let family = match &style.font_family {
            Some(f) => f,
            None => return String::new(),
        };
        let names: Vec<String> = family
            .split(',')
            .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
            .filter(|s| !s.is_empty())
            .map(|s| format!("\"{}\"", s))
            .collect();
        if names.is_empty() {
            String::new()
        } else {
            format!("font: ({}, ), ", names.join(", "))
        }
    }

    pub fn emit_content(&mut self, pos: Pt, style: &Style, m: f64, text: &str) {
        let anchor = Self::anchor_for(style);
        let mut text_args = String::new();
        if let Some(fs) = style.font_size {
            text_args.push_str(&format!("size: {}pt, ", num(fs * self.font_scale * m)));
        }
        text_args.push_str(&self.font_frag(style));
        let alpha = style.opacity * style.fill_opacity;
        let fill = resolve_paint(&style.fill, &self.gradients, alpha);
        if fill != "none" {
            text_args.push_str(&format!("fill: {}, ", fill));
        }
        let body = if text_args.is_empty() {
            format!("[{}]", escape_text(text))
        } else {
            format!("text({})[{}]", text_args, escape_text(text))
        };
        self.out.push_str(&format!(
            "content(({}, {}), anchor: \"{}\", {})\n",
            num(pos.0),
            num(pos.1),
            anchor,
            body
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Rgb;

    fn emitter() -> Emitter {
        Emitter::new(GradientMap::new(), 30.0, 25.0)
    }

    #[test]
    fn escapes_markup_but_not_slash_or_amp() {
        assert_eq!(
            escape_text(r"a_b *c* ~d~ \e #f [g] $h <i> j/k &l"),
            r"a\_b \*c\* \~d\~ \\e \#f \[g\] \$h \<i> j/k &l"
        );
    }

    #[test]
    fn number_formatting_trims() {
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.1234), "0.123");
        assert_eq!(num(-0.0001), "0");
    }

    #[test]
    fn fill_only_shape_gets_stroke_none() {
        let e = emitter();
        let mut s = Style::default();
        s.fill = Paint::Solid(Rgb { r: 255, g: 0, b: 0 });
        let frag = e.shape_style(&s, 1.0, true, false);
        assert!(frag.contains("fill: rgb(\"#ff0000\")"));
        assert!(frag.contains("stroke: none"));
    }

    #[test]
    fn dash_becomes_length_array() {
        let e = emitter();
        let mut s = Style::default();
        s.dash = Some("4 2".to_string());
        let frag = e.dash_frag(&s, 1.0);
        assert!(frag.contains("dash: (120pt, 60pt, )"));
    }

    #[test]
    fn evenodd_is_emitted() {
        let e = emitter();
        let mut s = Style::default();
        s.fill_rule = FillRule::EvenOdd;
        assert!(e.fill_rule_frag(&s).contains("even-odd"));
    }

    #[test]
    fn anchor_mapping() {
        let mut s = Style::default();
        assert_eq!(Emitter::anchor_for(&s), "base-west");
        s.text_anchor = TextAnchor::Middle;
        assert_eq!(Emitter::anchor_for(&s), "base");
        s.text_anchor = TextAnchor::End;
        assert_eq!(Emitter::anchor_for(&s), "base-east");
        s.dominant_baseline = Some("central".into());
        assert_eq!(Emitter::anchor_for(&s), "mid-east");
    }

    #[test]
    fn open_stroke_subpaths_stay_separate() {
        let mut e = emitter();
        let mut s = Style::default();
        s.fill = Paint::None;
        s.stroke = Paint::Solid(Rgb { r: 0, g: 0, b: 0 });
        let subs = crate::path::from_path_d("M0 0 L1 1 M5 5 L6 6", &svgtypes::Transform::default());
        e.emit_subpaths(&subs, &s, 1.0);
        assert_eq!(e.out.matches("svg-path(").count(), 2);
        assert!(!e.out.contains("(\"z\""));
    }

    #[test]
    fn filled_holes_use_one_closed_path() {
        let mut e = emitter();
        let s = Style::default(); // default black fill
        let subs = crate::path::from_path_d(
            "M0 0 L4 0 L4 4 L0 4 Z M1 1 L1 3 L3 3 L3 1 Z",
            &svgtypes::Transform::default(),
        );
        e.emit_subpaths(&subs, &s, 1.0);
        assert_eq!(e.out.matches("svg-path(").count(), 1);
        assert_eq!(e.out.matches("(\"z\", )").count(), 2);
    }
}
