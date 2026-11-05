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
    pub fn dimmed(self, dark: bool) -> Self {
        let background = Self::hex(if dark { 0x161618 } else { 0xf7f7f9 });
        for step in (0..=20).rev() {
            let t = step as f64 * 0.02;
            let color = Self {
                r: self.r + (background.r - self.r) * t,
                g: self.g + (background.g - self.g) * t,
                b: self.b + (background.b - self.b) * t,
            };
            if color.contrast(self.ink()) >= 4.5 {
                return color;
            }
        }
        self
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
    fn lab(self) -> [f64; 3] {
        let (r, g, b) = (
            Self::linear(self.r),
            Self::linear(self.g),
            Self::linear(self.b),
        );
        let f = |v: f64| {
            if v > 0.008856 {
                v.cbrt()
            } else {
                7.787 * v + 16. / 116.
            }
        };
        let x = f((0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047);
        let y = f(0.2126 * r + 0.7152 * g + 0.0722 * b);
        let z = f((0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883);
        [116. * y - 16., 500. * (x - y), 200. * (y - z)]
    }
    pub fn delta_e(self, other: Self) -> f64 {
        self.lab()
            .into_iter()
            .zip(other.lab())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}
impl Lch {
    pub fn tuned(self, dark: bool) -> Self {
        Self {
            l: self.l.clamp(
                if dark { 0.56 } else { 0.68 },
                if dark { 0.76 } else { 0.80 },
            ),
            c: (self.c * 0.8).min(0.17),
            h: self.h,
        }
    }
    pub fn rgb(self) -> Rgb {
        let mut chroma = self.c;
        let linear = loop {
            let (a, b) = (
                chroma * self.h.to_radians().cos(),
                chroma * self.h.to_radians().sin(),
            );
            let x = (self.l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
            let y = (self.l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
            let z = (self.l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
            let linear = [
                4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z,
                -1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z,
                -0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z,
            ];
            chroma = (chroma - 0.004).max(0.);
            if chroma <= 0. || linear.iter().all(|v| *v >= -0.0005 && *v <= 1.0005) {
                break linear;
            }
        };
        let [r, g, b] = linear.map(|v| {
            let v = v.clamp(0., 1.);
            if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            }
        });
        Rgb { r, g, b }
    }
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
pub fn separate(
    seeds: impl IntoIterator<Item = (String, Lch)>,
    dark: bool,
) -> HashMap<String, Rgb> {
    let mut placed = Vec::new();
    let mut result = HashMap::new();
    for (key, seed) in seeds {
        if seed.c == 0. {
            result.insert(key, seed.rgb());
            continue;
        }
        let mut best = seed.rgb();
        let mut best_distance = -1.;
        let (lower, upper) = if dark { (0.56, 0.76) } else { (0.68, 0.80) };
        for l in [
            seed.l,
            seed.l + 0.09,
            seed.l - 0.09,
            seed.l + 0.17,
            seed.l - 0.17,
            lower,
            upper,
        ] {
            if l < lower || l > upper {
                continue;
            }
            let candidate = Lch { l, ..seed }.rgb();
            let distance = placed
                .iter()
                .map(|c| candidate.delta_e(*c))
                .reduce(f64::min)
                .unwrap_or(99.);
            if distance > best_distance {
                best = candidate;
                best_distance = distance;
            }
            if distance >= 20. {
                break;
            }
        }
        result.insert(key, best);
        placed.push(best);
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
        assert_eq!(
            Lch {
                l: 1.,
                c: 1.,
                h: 25.
            }
            .tuned(true),
            Lch {
                l: 0.76,
                c: 0.17,
                h: 25.
            }
        );
        assert_eq!(
            Lch {
                l: 0.,
                c: 1.,
                h: 25.
            }
            .tuned(false),
            Lch {
                l: 0.68,
                c: 0.17,
                h: 25.
            }
        );
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
        let neutral = Lch {
            l: 0.32,
            c: 0.,
            h: 0.,
        };
        assert_eq!(
            separate([("neutral".into(), neutral)], true)["neutral"],
            neutral.rgb()
        );
        let same = Lch {
            l: 0.56,
            c: 0.12,
            h: 250.,
        };
        let separated = separate([("large".into(), same), ("small".into(), same)], true);
        assert_eq!(separated["large"], same.rgb());
        assert!(separated["large"].delta_e(separated["small"]) >= 20.);
        for dark in [true, false] {
            let crowded = separate(
                (0..12).map(|i| {
                    (
                        i.to_string(),
                        Lch {
                            l: if dark { 0.65 } else { 0.74 },
                            c: 0.12,
                            h: 250.,
                        },
                    )
                }),
                dark,
            );
            assert_eq!(crowded.len(), 12);
            assert!(crowded.values().all(|c| {
                let l = c.lch().l;
                if dark {
                    (0.559..=0.761).contains(&l)
                } else {
                    (0.679..=0.801).contains(&l)
                }
            }));
        }
        assert_eq!(
            Lch {
                l: 1.,
                c: 0.1,
                h: 250.
            }
            .tuned(false)
            .l,
            0.80
        );
    }
    #[test]
    fn all_labels_pass_at_rest_and_dimmed() {
        for dark in [true, false] {
            for hue in (0..360).step_by(3) {
                for lightness in 0..=13 {
                    let color = Lch {
                        l: 0.3 + lightness as f64 * 0.05,
                        c: 0.17,
                        h: hue as f64,
                    }
                    .tuned(dark)
                    .rgb();
                    assert!(
                        color.contrast(color.ink()) >= 4.5
                            && color.dimmed(dark).contrast(color.ink()) >= 4.5,
                        "{dark} {hue} {lightness}"
                    );
                }
            }
        }
    }
}
