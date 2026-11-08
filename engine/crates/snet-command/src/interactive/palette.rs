use console::Style;

/// A colour given as RGB. It is written as 24-bit colour where the terminal
/// reports support for it, and as the nearest 256-colour entry elsewhere,
/// because terminals without 24-bit colour misread its escape codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hue {
    red: u8,
    green: u8,
    blue: u8,
}

pub const APPLE_BLUE: Hue = Hue::rgb(0x0a, 0x84, 0xff);
pub const APPLE_GREEN: Hue = Hue::rgb(0x30, 0xd1, 0x58);
pub const APPLE_ORANGE: Hue = Hue::rgb(0xff, 0x9f, 0x0a);
pub const APPLE_RED: Hue = Hue::rgb(0xff, 0x45, 0x3a);
pub const APPLE_GREY: Hue = Hue::rgb(0x8e, 0x8e, 0x93);
pub const ANTHROPIC_CLAY: Hue = Hue::rgb(0xd9, 0x77, 0x57);
pub const MELIOUS_BLUE: Hue = Hue::rgb(0x1f, 0x8b, 0xe0);

/// The six levels each channel takes in the 256-colour cube (entries 16–231).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
const CUBE_START: u8 = 16;
/// The grey ramp (entries 232–255) runs from 8 to 238 in steps of 10.
const GREY_RAMP_START: u8 = 232;
const GREY_RAMP_LENGTH: u8 = 24;

impl Hue {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn style(self) -> Style {
        if console::true_colors_enabled() {
            Style::new().true_color(self.red, self.green, self.blue)
        } else {
            Style::new().color256(self.nearest_256())
        }
    }

    /// The colour in the dotted form spinner templates take.
    pub fn dotted(self) -> String {
        if console::true_colors_enabled() {
            format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
        } else {
            self.nearest_256().to_string()
        }
    }

    /// The closest entry of the colour cube or grey ramp. Entries 0–15 are
    /// left out because every terminal theme redefines them.
    pub fn nearest_256(self) -> u8 {
        let cube_red = nearest_cube_level(self.red);
        let cube_green = nearest_cube_level(self.green);
        let cube_blue = nearest_cube_level(self.blue);
        let mut best_index = CUBE_START + 36 * cube_red + 6 * cube_green + cube_blue;
        let mut best_distance = self.distance(
            CUBE_LEVELS[usize::from(cube_red)],
            CUBE_LEVELS[usize::from(cube_green)],
            CUBE_LEVELS[usize::from(cube_blue)],
        );
        for step in 0..GREY_RAMP_LENGTH {
            let level = 8 + 10 * step;
            let distance = self.distance(level, level, level);
            if distance < best_distance {
                best_distance = distance;
                best_index = GREY_RAMP_START + step;
            }
        }
        best_index
    }

    fn distance(self, red: u8, green: u8, blue: u8) -> u32 {
        let channel = |own: u8, other: u8| u32::from(own.abs_diff(other)).pow(2);
        channel(self.red, red) + channel(self.green, green) + channel(self.blue, blue)
    }
}

/// The index of the cube level closest to `value`.
fn nearest_cube_level(value: u8) -> u8 {
    let mut best_index = 0;
    for (index, level) in CUBE_LEVELS.iter().enumerate() {
        if value.abs_diff(*level) < value.abs_diff(CUBE_LEVELS[best_index]) {
            best_index = index;
        }
    }
    best_index as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_falls_back_to_its_nearest_256_colour_entry() {
        assert_eq!(APPLE_BLUE.nearest_256(), 33);
        assert_eq!(APPLE_GREEN.nearest_256(), 77);
        assert_eq!(APPLE_ORANGE.nearest_256(), 214);
        assert_eq!(APPLE_RED.nearest_256(), 203);
        assert_eq!(APPLE_GREY.nearest_256(), 246);
        assert_eq!(ANTHROPIC_CLAY.nearest_256(), 173);
        assert_eq!(MELIOUS_BLUE.nearest_256(), 32);
    }

    #[test]
    fn exact_cube_and_ramp_colours_map_to_themselves() {
        assert_eq!(Hue::rgb(0, 0, 0).nearest_256(), 16);
        assert_eq!(Hue::rgb(255, 255, 255).nearest_256(), 231);
        assert_eq!(Hue::rgb(8, 8, 8).nearest_256(), 232);
        assert_eq!(Hue::rgb(238, 238, 238).nearest_256(), 255);
    }
}
