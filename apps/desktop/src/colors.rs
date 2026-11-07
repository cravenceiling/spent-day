use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}
impl Rgb {
    pub const BLACK: Self = Self {
        r: 0.,
        g: 0.,
        b: 0.,
    };
    pub const WHITE: Self = Self {
        r: 1.,
        g: 1.,
        b: 1.,
    };
    pub fn hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 255) as f64 / 255.,
            g: ((hex >> 8) & 255) as f64 / 255.,
            b: (hex & 255) as f64 / 255.,
        }
    }
    fn linear(v: f64) -> f64 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(self) -> f64 {
        0.2126 * Self::linear(self.r)
            + 0.7152 * Self::linear(self.g)
            + 0.0722 * Self::linear(self.b)
    }
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    pub fn ink(self) -> Self {
        if self.contrast(Self::BLACK) >= self.contrast(Self::WHITE) {
            Self::BLACK
        } else {
            Self::WHITE
        }
    }
    pub fn lch(self) -> Lch {
        let (r, g, b) = (
            Self::linear(self.r),
            Self::linear(self.g),
            Self::linear(self.b),
        );
        let l = (0.4122214708 * r + 0.5363015264 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
        let q = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
        Lch {
            l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            c: a.hypot(q),
            h: q.atan2(a).to_degrees(),
        }
    }
}
impl Lch {
    pub fn rgb(self) -> Rgb {
        let fitted = fit(self);
        let [r, g, b] = fitted.linear().map(|v| {
            let v = v.clamp(0., 1.);
            if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            }
        });
        Rgb { r, g, b }
    }

    fn linear(self) -> [f64; 3] {
        let (sin, cos) = self.h.to_radians().sin_cos();
        let (a, b) = (self.c * cos, self.c * sin);
        let x = (self.l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
        let y = (self.l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
        let z = (self.l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
        [
            4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z,
            -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z,
            -0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z,
        ]
    }
}

pub fn lch(l: f64, c: f64, h: f64) -> Lch {
    Lch {
        l,
        c,
        h: h.rem_euclid(360.),
    }
}

pub fn in_gamut(color: Lch) -> bool {
    color.linear().iter().all(|v| (-1e-4..=1.0001).contains(v))
}

pub fn max_chroma(l: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0., 0.5);
    for _ in 0..30 {
        let middle = (lo + hi) / 2.;
        if in_gamut(lch(l, middle, h)) {
            lo = middle;
        } else {
            hi = middle;
        }
    }
    lo
}

pub fn cusp(h: f64) -> (f64, f64) {
    let mut best = (0.5, 0.);
    for i in 20..=98 {
        let l = f64::from(i) / 100.;
        let c = max_chroma(l, h);
        if c > best.1 {
            best = (l, c);
        }
    }
    best
}

/// Cut chroma at fixed lightness and hue instead of clipping RGB channels.
pub fn fit(color: Lch) -> Lch {
    Lch {
        c: color.c.min(max_chroma(color.l, color.h)),
        ..color
    }
}

pub fn ok_distance(a: Lch, b: Lch) -> f64 {
    let coordinates = |color| {
        let color = fit(color);
        [
            color.l,
            color.c * color.h.to_radians().cos(),
            color.c * color.h.to_radians().sin(),
        ]
    };
    coordinates(a)
        .into_iter()
        .zip(coordinates(b))
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

pub fn hashed(key: &str) -> Lch {
    let hash = key
        .bytes()
        .fold(5381u64, |h, b| h.wrapping_mul(33).wrapping_add(b as u64));
    Lch {
        l: 0.67,
        c: 0.16,
        h: (hash % 360) as f64,
    }
}
pub fn dominant(pixels: impl IntoIterator<Item = (Rgb, f64)>) -> Option<Lch> {
    let mut bins = [[0.; 4]; 36];
    for (pixel, alpha) in pixels {
        if alpha < 0.5 {
            continue;
        }
        let high = pixel.r.max(pixel.g).max(pixel.b);
        let low = pixel.r.min(pixel.g).min(pixel.b);
        if high <= 0.12 || low >= 0.92 || high - low <= 0.08 {
            continue;
        }
        let saturation = (high - low) / high;
        let hue = if high == pixel.r {
            (pixel.g - pixel.b) / (high - low)
        } else if high == pixel.g {
            (pixel.b - pixel.r) / (high - low) + 2.
        } else {
            (pixel.r - pixel.g) / (high - low) + 4.
        };
        let bin = ((hue * 60. + 360.) % 360. / 10.) as usize;
        for (sum, v) in bins[bin].iter_mut().zip([1., pixel.r, pixel.g, pixel.b]) {
            *sum += v * saturation;
        }
    }
    let best = bins.iter().max_by(|a, b| a[0].total_cmp(&b[0]))?;
    (best[0] > 0.).then(|| {
        Rgb {
            r: best[1] / best[0],
            g: best[2] / best[0],
            b: best[3] / best[0],
        }
        .lch()
    })
}
/// Input order is the day's total-time rank. Each list is ordered by preference.
pub fn separate(candidates: impl IntoIterator<Item = (String, Vec<Lch>)>) -> HashMap<String, Lch> {
    let mut placed: Vec<Lch> = Vec::new();
    let mut result = HashMap::new();
    for (key, candidates) in candidates {
        let score = |candidate: &Lch| {
            placed
                .iter()
                .map(|p| ok_distance(*candidate, *p))
                .fold(9., f64::min)
        };
        let pick = candidates
            .iter()
            .find(|c| score(c) >= 0.10)
            .copied()
            .unwrap_or_else(|| {
                *candidates
                    .iter()
                    .max_by(|a, b| score(a).total_cmp(&score(b)))
                    .expect("palette candidates")
            });
        placed.push(pick);
        result.insert(key, pick);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_colors() {
        assert!((Rgb::BLACK.contrast(Rgb::WHITE) - 21.).abs() < 0.00001);
        let red = Rgb::hex(0xff0000);
        let roundtrip = red.lch().rgb();
        assert!((roundtrip.r - 1.).abs() < 0.001 && roundtrip.g < 0.001 && roundtrip.b < 0.001);
        assert_eq!(hashed("site:example.org").h, 110.);
        let sampled = dominant([
            (Rgb::WHITE, 1.),
            (Rgb::BLACK, 1.),
            (red, 1.),
            (red, 1.),
            (Rgb::hex(0x0000ff), 0.1),
        ])
        .unwrap();
        assert!((sampled.h - red.lch().h).abs() < 0.001);
        assert_eq!(dominant([(Rgb::WHITE, 1.), (Rgb::BLACK, 1.)]), None);
        assert_eq!(cusp(258.).0, 0.60);
        let fitted = fit(lch(0.53, 0.5, 258.));
        assert!((fitted.c - 0.19640571530908346).abs() < 1e-12);
        assert_eq!((fitted.l, fitted.h), (0.53, 258.));
        assert!(in_gamut(fitted));
        assert!(!in_gamut(lch(0.53, fitted.c + 0.001, 258.)));
        assert!((ok_distance(lch(0.5, 0., 0.), lch(0.7, 0., 0.)) - 0.2).abs() < 1e-8);
    }

    #[test]
    fn separation_prefers_first_clear_candidate_then_best_available() {
        let a = lch(0.5, 0.08, 268.);
        let b = lch(0.5, 0.075, 205.);
        let c = lch(0.74, 0.07, 268.);
        let separated = separate([
            ("first".into(), vec![a, b, c]),
            ("second".into(), vec![a, b, c]),
            ("third".into(), vec![a, b, c]),
        ]);
        assert_eq!(separated["first"], a);
        assert_eq!(separated["second"], c);
        assert_eq!(separated["third"], b);
        assert!(ok_distance(a, c) >= 0.10);
        assert!(ok_distance(a, b) < 0.10);
    }
}
