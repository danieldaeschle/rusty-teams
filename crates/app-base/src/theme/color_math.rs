//! Colour helpers for building palettes. Colours are `0xrrggbb`.

fn channels(hex: u32) -> [f64; 3] {
    [16, 8, 0].map(|shift| f64::from((hex >> shift) & 0xff) / 255.)
}

fn from_channels([red, green, blue]: [f64; 3]) -> u32 {
    let byte = |value: f64| (value.clamp(0., 1.) * 255.).round() as u32;
    byte(red) << 16 | byte(green) << 8 | byte(blue)
}

fn to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

fn luminance(hex: u32) -> f64 {
    let [red, green, blue] = channels(hex).map(to_linear);
    0.2126 * red + 0.7152 * green + 0.0722 * blue
}

/// WCAG contrast ratio of two colours.
pub fn contrast(first: u32, second: u32) -> f64 {
    let (first, second) = (luminance(first), luminance(second));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

fn linear_from_oklch(lightness: f64, chroma: f64, hue: f64) -> [f64; 3] {
    let (a, b) = (
        chroma * hue.to_radians().cos(),
        chroma * hue.to_radians().sin(),
    );
    let long = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let medium = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let short = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    [
        4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
        -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
        -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701 * short,
    ]
}

/// The sRGB colour for an OKLCH value, lowering chroma until it fits the gamut.
pub fn oklch(lightness: f64, chroma: f64, hue: f64) -> u32 {
    let mut chroma = chroma;
    loop {
        let linear = linear_from_oklch(lightness, chroma, hue);
        if chroma <= 0. || linear.iter().all(|value| (0. ..=1.).contains(value)) {
            return from_channels(linear.map(|value| to_gamma(value.clamp(0., 1.))));
        }
        chroma -= 0.002;
    }
}

/// The colour at `chroma` and `hue` closest to `start` lightness that reaches `target`
/// contrast on every ground. Dark shades search lighter, light shades darker.
pub fn readable(
    start: f64,
    chroma: f64,
    hue: f64,
    grounds: &[u32],
    target: f64,
    dark: bool,
) -> u32 {
    let step = if dark { 0.005 } else { -0.005 };
    let mut value = start;
    while (0. ..=1.).contains(&value) {
        let color = oklch(value, chroma, hue);
        if grounds
            .iter()
            .all(|ground| contrast(color, *ground) >= target)
        {
            return color;
        }
        value += step;
    }
    if dark { 0xffffff } else { 0x000000 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!((contrast(0x000000, 0xffffff) - 21.).abs() < 0.01);
    }

    #[test]
    fn oklch_stays_in_gamut_and_keeps_grey_neutral() {
        assert_eq!(oklch(1., 0., 0.), 0xffffff);
        assert_eq!(oklch(0., 0., 0.), 0x000000);
        let grey = oklch(0.5, 0., 120.);
        assert_eq!(grey >> 16 & 0xff, grey & 0xff);
    }

    #[test]
    fn readable_reaches_the_target_on_every_ground() {
        let grounds = [0x161617, 0x2c2c2f];
        let color = readable(0.4, 0.1, 40., &grounds, 4.5, true);
        assert!(grounds.iter().all(|ground| contrast(color, *ground) >= 4.5));
    }
}
