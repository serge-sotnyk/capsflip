//! The tray icon, drawn in code: sharp at every display scale and with no
//! resource compiler in the build.

use crate::tray::Icon;

/// The art is a square of this many cells.
const GRID: usize = 32;
/// Points sampled along each axis of a pixel, for smooth edges.
const SAMPLES: usize = 4;

/// The Caps Lock symbol on a rounded square: `.` is transparent, `#` the
/// background, `o` the white symbol.
const ART: [&str; GRID] = [
    "....########################....",
    "..############################..",
    ".##############################.",
    ".##############################.",
    "################################",
    "################################",
    "###############oo###############",
    "##############oooo##############",
    "#############oooooo#############",
    "############oooooooo############",
    "###########oooooooooo###########",
    "##########oooooooooooo##########",
    "#########oooooooooooooo#########",
    "########oooooooooooooooo########",
    "#######oooooooooooooooooo#######",
    "######oooooooooooooooooooo######",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "################################",
    "################################",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "###########oooooooooo###########",
    "################################",
    ".##############################.",
    ".##############################.",
    "..############################..",
    "....########################....",
];

/// The Windows 11 accent blue, and grey while paused, as red, green, blue.
const ACTIVE: [u8; 3] = [0x00, 0x67, 0xc0];
const PAUSED: [u8; 3] = [0x76, 0x76, 0x76];

/// Draws `icon` as `size` × `size` pixels, row by row from the top. Each
/// pixel is `0xAARRGGBB` with straight, not premultiplied, alpha.
#[must_use]
pub fn pixels(icon: Icon, size: usize) -> Vec<u32> {
    let background = match icon {
        Icon::Active => ACTIVE,
        Icon::Paused => PAUSED,
    };
    (0..size * size)
        .map(|index| pixel(background, size, index % size, index / size))
        .collect()
}

/// Samples the art inside the pixel at (`x`, `y`) and counts how many
/// points hit a transparent, a background and a symbol cell.
fn pixel(background: [u8; 3], size: usize, x: usize, y: usize) -> u32 {
    let cell = |point: usize, sample: usize| (point * SAMPLES + sample) * GRID / (size * SAMPLES);
    let mut cover = [0u32; 3];
    for sample in 0..SAMPLES * SAMPLES {
        let row = ART[cell(y, sample / SAMPLES)].as_bytes();
        cover[class(row[cell(x, sample % SAMPLES)])] += 1;
    }
    blend(background, cover)
}

const fn class(cell: u8) -> usize {
    match cell {
        b'#' => 1,
        b'o' => 2,
        _ => 0,
    }
}

/// Mixes the background with white by the points that hit each; points
/// that hit nothing make the pixel transparent.
fn blend(background: [u8; 3], [clear, plain, symbol]: [u32; 3]) -> u32 {
    let opaque = plain + symbol;
    if opaque == 0 {
        return 0;
    }
    let channel = |base: u8| byte((u32::from(base) * plain + 0xff * symbol) / opaque);
    let [red, green, blue] = background.map(channel);
    let alpha = byte(0xff * opaque / (clear + opaque));
    u32::from_be_bytes([alpha, red, green, blue])
}

/// A channel value, which the arithmetic above keeps within 0..=255.
fn byte(value: u32) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: u32 = 0xff00_67c0;
    const GREY: u32 = 0xff76_7676;
    const WHITE: u32 = 0xffff_ffff;

    fn at(pixels: &[u32], size: usize, x: usize, y: usize) -> u32 {
        pixels[y * size + x]
    }

    #[test]
    fn an_icon_has_one_pixel_per_point_of_its_size() {
        assert_eq!(pixels(Icon::Active, 16).len(), 256);
        assert_eq!(pixels(Icon::Active, 24).len(), 576);
    }

    #[test]
    fn at_full_size_the_art_is_copied_cell_by_cell() {
        let icon = pixels(Icon::Active, 32);
        assert_eq!(at(&icon, 32, 3, 0), 0, "rounded corner");
        assert_eq!(at(&icon, 32, 4, 0), BLUE);
        assert_eq!(at(&icon, 32, 14, 6), BLUE);
        assert_eq!(at(&icon, 32, 15, 6), WHITE, "tip of the arrow");
        assert_eq!(at(&icon, 32, 11, 24), WHITE, "the bar under the arrow");
        assert_eq!(at(&icon, 32, 24, 11), BLUE);
    }

    #[test]
    fn at_half_size_each_pixel_averages_four_cells() {
        let icon = pixels(Icon::Active, 16);
        assert_eq!(at(&icon, 16, 0, 0), 0);
        assert_eq!(at(&icon, 16, 1, 0), 0x7f00_67c0, "half covered");
        assert_eq!(at(&icon, 16, 5, 8), 0xff7f_b3df, "half arrow, half blue");
        assert_eq!(at(&icon, 16, 7, 3), 0xffbf_d9ef, "three cells of arrow");
    }

    #[test]
    fn paused_differs_from_active_only_in_the_background() {
        let icon = pixels(Icon::Paused, 32);
        assert_eq!(at(&icon, 32, 3, 0), 0);
        assert_eq!(at(&icon, 32, 4, 0), GREY);
        assert_eq!(at(&icon, 32, 15, 6), WHITE);
    }

    #[test]
    fn every_size_the_tray_uses_keeps_the_shape() {
        for size in [16, 20, 24, 32, 40, 48] {
            let icon = pixels(Icon::Active, size);
            let middle = size / 2;
            assert_eq!(at(&icon, size, 0, 0), 0, "corner at {size}");
            assert_eq!(at(&icon, size, middle, middle), WHITE, "arrow at {size}");
        }
    }
}
