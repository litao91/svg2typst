use svgtypes::Transform;

/// Compose two transforms so that `child` is applied first, then `parent`.
/// Equivalent to the matrix product `parent * child`.
pub fn compose(parent: &Transform, child: &Transform) -> Transform {
    Transform {
        a: parent.a * child.a + parent.c * child.b,
        b: parent.b * child.a + parent.d * child.b,
        c: parent.a * child.c + parent.c * child.d,
        d: parent.b * child.c + parent.d * child.d,
        e: parent.a * child.e + parent.c * child.f + parent.e,
        f: parent.b * child.e + parent.d * child.f + parent.f,
    }
}

pub fn apply(t: &Transform, x: f64, y: f64) -> (f64, f64) {
    (t.a * x + t.c * y + t.e, t.b * x + t.d * y + t.f)
}

/// Uniform scale magnitude of the linear part (ignores rotation/shear direction).
pub fn scale_mag(t: &Transform) -> f64 {
    (t.a * t.a + t.b * t.b).sqrt()
}

/// True when the linear part maps circles to circles: a uniform scale
/// composed with a rotation (conformal) or with a reflection (anti-conformal,
/// e.g. the SVG→CeTZ y-flip).
pub fn is_similarity(t: &Transform) -> bool {
    let conformal = (t.a - t.d).abs() < 1e-12 && (t.c + t.b).abs() < 1e-12;
    let anti_conformal = (t.a + t.d).abs() < 1e-12 && (t.c - t.b).abs() < 1e-12;
    conformal || anti_conformal
}

/// True when the linear part is axis-aligned scaling + translation (no rotation/skew).
pub fn is_axis_aligned(t: &Transform) -> bool {
    t.b.abs() < 1e-12 && t.c.abs() < 1e-12
}

/// Root transform mapping SVG user space to CeTZ space.
///
/// CeTZ is y-up while SVG is y-down, and the viewBox origin must be normalised to
/// (0, 0). Given viewBox `(minx, miny, w, h)` and scale `s`:
///   X = (x - minx) * s
///   Y = (miny + h - y) * s
pub fn root_transform(minx: f64, miny: f64, h: f64, s: f64) -> Transform {
    Transform {
        a: s,
        b: 0.0,
        c: 0.0,
        d: -s,
        e: -minx * s,
        f: (miny + h) * s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewBox_origin_is_normalised() {
        let t = root_transform(500.0, 500.0, 100.0, 0.01);
        assert_eq!(apply(&t, 500.0, 500.0), (0.0, 1.0));
        assert_eq!(apply(&t, 600.0, 600.0), (1.0, 0.0));
    }

    #[test]
    fn zero_origin_viewbox_only_scales_and_flips() {
        let t = root_transform(0.0, 0.0, 100.0, 0.003);
        assert_eq!(apply(&t, 0.0, 0.0), (0.0, 0.3));
        assert_eq!(apply(&t, 10.0, 10.0), (0.03, 0.27));
    }

    #[test]
    fn compose_applies_child_first() {
        let parent = Transform::new(1.0, 0.0, 0.0, 1.0, 10.0, 10.0); // translate
        let child = Transform::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0); // scale
        let both = compose(&parent, &child);
        assert_eq!(apply(&both, 1.0, 1.0), (12.0, 12.0));
    }

    #[test]
    fn similarity_detection() {
        assert!(is_similarity(&Transform::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0)));
        assert!(!is_similarity(&Transform::new(2.0, 0.0, 0.0, 3.0, 0.0, 0.0)));
        assert!(is_axis_aligned(&Transform::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0)));
        assert!(!is_axis_aligned(&Transform::new(1.0, 1.0, 0.0, 1.0, 0.0, 0.0)));
    }
}
