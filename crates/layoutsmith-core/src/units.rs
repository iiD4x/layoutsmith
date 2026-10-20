//! Android value types: dimensions, colors and gravity.

use std::fmt;

/// A `layout_width` / `layout_height` value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dim {
    Match,
    Wrap,
    /// A fixed size in dp. `Dp(0.0)` is "match constraint" inside a
    /// ConstraintLayout and "use the weight" inside a LinearLayout.
    Dp(f32),
}

impl Dim {
    pub fn to_attr(self) -> String {
        match self {
            Dim::Match => "match_parent".into(),
            Dim::Wrap => "wrap_content".into(),
            Dim::Dp(v) => format_dp(v),
        }
    }

    pub fn is_zero(self) -> bool {
        matches!(self, Dim::Dp(v) if v == 0.0)
    }
}

/// Parses a dimension such as `16dp`, `14sp` or `2px` and returns it in dp.
/// `density` is the preview screen density (pixels per dp).
pub fn parse_dimension(s: &str, density: f32) -> Option<f32> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f32 = num.parse().ok()?;
    let factor = match unit.trim() {
        "dp" | "dip" | "sp" => 1.0,
        "px" => 1.0 / density,
        "in" => 160.0,
        "mm" => 160.0 / 25.4,
        "pt" => 160.0 / 72.0,
        _ => return None,
    };
    Some(v * factor)
}

/// Formats a dp value the way people write them: `16dp`, `0.5dp`.
pub fn format_dp(v: f32) -> String {
    format!("{}dp", format_number(v))
}

pub fn format_sp(v: f32) -> String {
    format!("{}sp", format_number(v))
}

/// Up to two decimals, without trailing zeros.
pub fn format_number(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        format!("{}", rounded as i64)
    } else {
        let s = format!("{rounded:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parses `#RGB`, `#ARGB`, `#RRGGBB` and `#AARRGGBB`.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let hex = s.trim().strip_prefix('#')?;
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        match hex.len() {
            3 => Some(Self::rgb(nibble(0)?, nibble(1)?, nibble(2)?)),
            4 => Some(Self::argb(nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?)),
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::argb(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// `#RRGGBB` when opaque, `#AARRGGBB` otherwise.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }

    /// Relative luminance in 0..=1, used to pick readable text colors.
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Alignment along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Fill,
}

/// A parsed `gravity` / `layout_gravity` value, split into its horizontal and
/// vertical parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gravity {
    pub h: Option<Align>,
    pub v: Option<Align>,
}

impl Gravity {
    pub fn parse(s: &str) -> Self {
        let mut g = Gravity::default();
        for token in s.split('|').map(str::trim) {
            match token {
                "left" | "start" => g.h = Some(Align::Start),
                "right" | "end" => g.h = Some(Align::End),
                "center_horizontal" => g.h = Some(Align::Center),
                "fill_horizontal" => g.h = Some(Align::Fill),
                "top" => g.v = Some(Align::Start),
                "bottom" => g.v = Some(Align::End),
                "center_vertical" => g.v = Some(Align::Center),
                "fill_vertical" => g.v = Some(Align::Fill),
                "center" => {
                    g.h = Some(Align::Center);
                    g.v = Some(Align::Center);
                }
                "fill" => {
                    g.h = Some(Align::Fill);
                    g.v = Some(Align::Fill);
                }
                _ => {}
            }
        }
        g
    }

    /// The part for axis 0 (horizontal) or 1 (vertical).
    pub fn axis(self, axis: usize) -> Option<Align> {
        if axis == 0 { self.h } else { self.v }
    }

    pub fn set_axis(&mut self, axis: usize, align: Option<Align>) {
        if axis == 0 {
            self.h = align;
        } else {
            self.v = align;
        }
    }

    pub fn is_empty(self) -> bool {
        self.h.is_none() && self.v.is_none()
    }

    pub fn to_attr(self) -> String {
        if self.h == Some(Align::Center) && self.v == Some(Align::Center) {
            return "center".into();
        }
        if self.h == Some(Align::Fill) && self.v == Some(Align::Fill) {
            return "fill".into();
        }
        let h = self.h.map(|a| match a {
            Align::Start => "start",
            Align::Center => "center_horizontal",
            Align::End => "end",
            Align::Fill => "fill_horizontal",
        });
        let v = self.v.map(|a| match a {
            Align::Start => "top",
            Align::Center => "center_vertical",
            Align::End => "bottom",
            Align::Fill => "fill_vertical",
        });
        [h, v].into_iter().flatten().collect::<Vec<_>>().join("|")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions() {
        assert_eq!(parse_dimension("16dp", 2.0), Some(16.0));
        assert_eq!(parse_dimension("14sp", 2.0), Some(14.0));
        assert_eq!(parse_dimension("10px", 2.0), Some(5.0));
        assert_eq!(parse_dimension("-4dip", 2.0), Some(-4.0));
        assert_eq!(parse_dimension("1in", 2.0), Some(160.0));
        assert_eq!(parse_dimension("wrap_content", 2.0), None);
        assert_eq!(parse_dimension("12", 2.0), None);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_dp(16.0), "16dp");
        assert_eq!(format_dp(0.5), "0.5dp");
        assert_eq!(format_dp(12.004), "12dp");
        assert_eq!(format_number(0.333_333), "0.33");
    }

    #[test]
    fn colors() {
        assert_eq!(Color::parse_hex("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(Color::parse_hex("#8000"), Some(Color::argb(0x88, 0, 0, 0)));
        assert_eq!(
            Color::parse_hex("#3F51B5"),
            Some(Color::rgb(0x3F, 0x51, 0xB5))
        );
        assert_eq!(
            Color::parse_hex("#803F51B5"),
            Some(Color::argb(0x80, 0x3F, 0x51, 0xB5))
        );
        assert_eq!(Color::parse_hex("red"), None);
        assert_eq!(Color::argb(0x80, 1, 2, 3).to_hex(), "#80010203");
        assert_eq!(Color::rgb(1, 2, 3).to_hex(), "#010203");
    }

    #[test]
    fn gravity() {
        let g = Gravity::parse("center_vertical|end");
        assert_eq!(g.h, Some(Align::End));
        assert_eq!(g.v, Some(Align::Center));
        assert_eq!(g.to_attr(), "end|center_vertical");
        assert_eq!(Gravity::parse("center").to_attr(), "center");
        assert_eq!(Gravity::parse("left|top").to_attr(), "start|top");
    }
}
