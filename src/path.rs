use svgtypes::{SimplePathSegment, SimplifyingPathParser, Transform};

use crate::geom::apply;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt(pub f64, pub f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    M(Pt),
    L(Pt),
    C(Pt, Pt, Pt),
    Q(Pt, Pt),
}

/// One contour of a path. `closed` mirrors SVG `Z`.
#[derive(Debug, Clone, Default)]
pub struct SubPath {
    pub cmds: Vec<Cmd>,
    pub closed: bool,
}

impl SubPath {
    pub fn is_degenerate(&self) -> bool {
        self.cmds.len() < 2
    }
}

fn tp(t: &Transform, x: f64, y: f64) -> Pt {
    let (a, b) = apply(t, x, y);
    Pt(a, b)
}

/// Parse an SVG `d` attribute into transformed subpaths. Arcs are already
/// flattened to cubics by `SimplifyingPathParser`; quadratics are preserved
/// because CeTZ `svg-path` supports them natively.
pub fn from_path_d(d: &str, t: &Transform) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut cur: Option<SubPath> = None;
    let mut last = Pt(0.0, 0.0);

    let ensure = |cur: &mut Option<SubPath>, last: Pt| {
        if cur.is_none() {
            *cur = Some(SubPath {
                cmds: vec![Cmd::M(last)],
                closed: false,
            });
        }
    };

    let parser = SimplifyingPathParser::from(d);
    for seg in parser {
        let seg = match seg {
            Ok(s) => s,
            Err(e) => {
                log::warn!("bad path segment: {}", e);
                continue;
            }
        };
        match seg {
            SimplePathSegment::MoveTo { x, y } => {
                if let Some(sp) = cur.take() {
                    if !sp.is_degenerate() {
                        out.push(sp);
                    }
                }
                last = tp(t, x, y);
                cur = Some(SubPath {
                    cmds: vec![Cmd::M(last)],
                    closed: false,
                });
            }
            SimplePathSegment::LineTo { x, y } => {
                ensure(&mut cur, last);
                let p = tp(t, x, y);
                cur.as_mut().unwrap().cmds.push(Cmd::L(p));
                last = p;
            }
            SimplePathSegment::CurveTo { x1, y1, x2, y2, x, y } => {
                ensure(&mut cur, last);
                let c1 = tp(t, x1, y1);
                let c2 = tp(t, x2, y2);
                let p = tp(t, x, y);
                cur.as_mut().unwrap().cmds.push(Cmd::C(c1, c2, p));
                last = p;
            }
            SimplePathSegment::Quadratic { x1, y1, x, y } => {
                ensure(&mut cur, last);
                let c = tp(t, x1, y1);
                let p = tp(t, x, y);
                cur.as_mut().unwrap().cmds.push(Cmd::Q(c, p));
                last = p;
            }
            SimplePathSegment::ClosePath => {
                if let Some(mut sp) = cur.take() {
                    sp.closed = true;
                    if !sp.is_degenerate() {
                        out.push(sp);
                    }
                }
            }
        }
    }
    if let Some(sp) = cur {
        if !sp.is_degenerate() {
            out.push(sp);
        }
    }
    out
}

const KAPPA: f64 = 0.5522847498307936;

/// Ellipse as four cubic Béziers (max radial error ~0.02%), transformed so
/// rotation/skew are honoured. CeTZ cannot take an (rx, ry) radius tuple.
pub fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64, t: &Transform) -> SubPath {
    let kx = KAPPA * rx;
    let ky = KAPPA * ry;
    let p = |x: f64, y: f64| tp(t, x, y);
    SubPath {
        cmds: vec![
            Cmd::M(p(cx + rx, cy)),
            Cmd::C(p(cx + rx, cy + ky), p(cx + kx, cy + ry), p(cx, cy + ry)),
            Cmd::C(p(cx - kx, cy + ry), p(cx - rx, cy + ky), p(cx - rx, cy)),
            Cmd::C(p(cx - rx, cy - ky), p(cx - kx, cy - ry), p(cx, cy - ry)),
            Cmd::C(p(cx + kx, cy - ry), p(cx + rx, cy - ky), p(cx + rx, cy)),
        ],
        closed: true,
    }
}

pub fn rect(x: f64, y: f64, w: f64, h: f64, t: &Transform) -> SubPath {
    let p = |x: f64, y: f64| tp(t, x, y);
    SubPath {
        cmds: vec![
            Cmd::M(p(x, y)),
            Cmd::L(p(x + w, y)),
            Cmd::L(p(x + w, y + h)),
            Cmd::L(p(x, y + h)),
        ],
        closed: true,
    }
}

pub fn polyline(pts: &[(f64, f64)], t: &Transform, close: bool) -> Option<SubPath> {
    let mut it = pts.iter();
    let first = it.next()?;
    let mut cmds = vec![Cmd::M(tp(t, first.0, first.1))];
    for (x, y) in it {
        cmds.push(Cmd::L(tp(t, *x, *y)));
    }
    if cmds.len() < 2 {
        return None;
    }
    Some(SubPath { cmds, closed: close })
}

#[cfg(test)]
mod tests {
    use super::*;
    use svgtypes::Transform;

    #[test]
    fn splits_subpaths_and_closes() {
        let t = Transform::default();
        let sp = from_path_d("M0 0 L10 0 L10 10 Z M20 20 L30 20", &t);
        assert_eq!(sp.len(), 2);
        assert!(sp[0].closed);
        assert!(!sp[1].closed);
        assert_eq!(sp[0].cmds.len(), 3); // M L L, closed via flag
    }

    #[test]
    fn quadratic_is_preserved() {
        let t = Transform::default();
        let sp = from_path_d("M0 0 Q 5 5 10 0", &t);
        assert_eq!(sp.len(), 1);
        assert!(matches!(sp[0].cmds[1], Cmd::Q(_, _)));
    }

    #[test]
    fn arc_is_flattened_to_cubics() {
        let t = Transform::default();
        let sp = from_path_d("M0 0 A 5 5 0 0 1 10 0", &t);
        assert_eq!(sp.len(), 1);
        assert!(sp[0].cmds.iter().skip(1).all(|c| matches!(c, Cmd::C(_, _, _))));
    }

    #[test]
    fn ellipse_has_four_cubics() {
        let t = Transform::default();
        let sp = ellipse(0.0, 0.0, 2.0, 1.0, &t);
        assert!(sp.closed);
        assert_eq!(sp.cmds.len(), 5); // M + 4 C
    }
}
