//! Colour themes: bundled presets plus user TOML files.
//!
//! A theme file is a flat TOML table; every key is optional and falls back to
//! the bundled `default` theme. Colours are `#rrggbb`, a named ANSI colour
//! (`red`, `darkgray`, …) or `default` for the terminal's own colour.

use ratatui::style::Color;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    pub fg: Color,
    pub title: Color,
    pub hi: Color,
    pub border: Color,
    pub border_focus: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub inactive: Color,
    pub graph_text: Color,
    pub meter_bg: Color,
    pub grad_low: Color,
    pub grad_mid: Color,
    pub grad_high: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub accent: Color,
    /// Colours are suppressed (`NO_COLOR`): widgets switch to distinct glyphs
    /// and reverse video so nothing relies on colour alone.
    pub no_color: bool,
}

/// On-disk theme representation; all keys optional.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    pub name: Option<String>,
    pub main_bg: Option<String>,
    pub main_fg: Option<String>,
    pub title: Option<String>,
    pub hi_fg: Option<String>,
    pub border: Option<String>,
    pub border_focus: Option<String>,
    pub selected_bg: Option<String>,
    pub selected_fg: Option<String>,
    pub inactive_fg: Option<String>,
    pub graph_text: Option<String>,
    pub meter_bg: Option<String>,
    pub gradient_low: Option<String>,
    pub gradient_mid: Option<String>,
    pub gradient_high: Option<String>,
    pub ok: Option<String>,
    pub warn: Option<String>,
    pub err: Option<String>,
    pub accent: Option<String>,
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[allow(clippy::too_many_arguments)]
fn mk(
    name: &str,
    bg: Color,
    fg: u32,
    title: u32,
    hi: u32,
    border: u32,
    border_focus: u32,
    selected_bg: u32,
    inactive: u32,
    meter_bg: u32,
    grad: [u32; 3],
    status: [u32; 3],
    accent: u32,
) -> Theme {
    Theme {
        name: name.into(),
        bg,
        fg: rgb(fg),
        title: rgb(title),
        hi: rgb(hi),
        border: rgb(border),
        border_focus: rgb(border_focus),
        selected_bg: rgb(selected_bg),
        selected_fg: rgb(title),
        inactive: rgb(inactive),
        graph_text: rgb(inactive),
        meter_bg: rgb(meter_bg),
        grad_low: rgb(grad[0]),
        grad_mid: rgb(grad[1]),
        grad_high: rgb(grad[2]),
        ok: rgb(status[0]),
        warn: rgb(status[1]),
        err: rgb(status[2]),
        accent: rgb(accent),
        no_color: false,
    }
}

/// Names of the compiled-in themes, in cycle order.
pub const BUILTIN: [&str; 7] = [
    "default",
    "light",
    "gruvbox",
    "nord",
    "tokyo-night",
    "dracula",
    "monochrome",
];

/// Look up a compiled-in theme by name.
pub fn builtin(name: &str) -> Option<Theme> {
    Some(match name {
        "default" => mk(
            "default",
            Color::Reset,
            0xcccccc,
            0xeeeeee,
            0xb54040,
            0x3d7b46,
            0x50f095,
            0x6a2f2f,
            0x606060,
            0x303030,
            [0x50f095, 0xf2e266, 0xfa1e1e],
            [0x50f095, 0xf2e266, 0xfa4040],
            0x4897d4,
        ),
        "light" => {
            let mut t = mk(
                "light",
                rgb(0xf6f5f0),
                0x2a2a2a,
                0x101010,
                0xc03030,
                0x8a9a8a,
                0x2e7d4f,
                0xd6e4f0,
                0x8c8c8c,
                0xdedede,
                [0x2e8b57, 0xc99a00, 0xd03030],
                [0x2e8b57, 0xb07800, 0xc62828],
                0x1f6fb2,
            );
            t.selected_fg = rgb(0x101010);
            t
        }
        "gruvbox" => mk(
            "gruvbox",
            rgb(0x282828),
            0xebdbb2,
            0xfbf1c7,
            0xfe8019,
            0x665c54,
            0xd79921,
            0x504945,
            0x7c6f64,
            0x3c3836,
            [0xb8bb26, 0xfabd2f, 0xfb4934],
            [0xb8bb26, 0xfabd2f, 0xfb4934],
            0x83a598,
        ),
        "nord" => mk(
            "nord",
            rgb(0x2e3440),
            0xd8dee9,
            0xeceff4,
            0x88c0d0,
            0x4c566a,
            0x88c0d0,
            0x434c5e,
            0x616e88,
            0x3b4252,
            [0xa3be8c, 0xebcb8b, 0xbf616a],
            [0xa3be8c, 0xebcb8b, 0xbf616a],
            0x81a1c1,
        ),
        "tokyo-night" => mk(
            "tokyo-night",
            rgb(0x1a1b26),
            0xc0caf5,
            0xc0caf5,
            0x7aa2f7,
            0x3b4261,
            0x7aa2f7,
            0x283457,
            0x565f89,
            0x24283b,
            [0x9ece6a, 0xe0af68, 0xf7768e],
            [0x9ece6a, 0xe0af68, 0xf7768e],
            0xbb9af7,
        ),
        "dracula" => mk(
            "dracula",
            rgb(0x282a36),
            0xf8f8f2,
            0xf8f8f2,
            0xff79c6,
            0x6272a4,
            0xbd93f9,
            0x44475a,
            0x6272a4,
            0x343746,
            [0x50fa7b, 0xf1fa8c, 0xff5555],
            [0x50fa7b, 0xf1fa8c, 0xff5555],
            0xbd93f9,
        ),
        "monochrome" => mk(
            "monochrome",
            Color::Reset,
            0xc8c8c8,
            0xffffff,
            0xffffff,
            0x5a5a5a,
            0xbbbbbb,
            0x3a3a3a,
            0x6e6e6e,
            0x2a2a2a,
            [0x707070, 0xa8a8a8, 0xffffff],
            [0xd0d0d0, 0xa0a0a0, 0xffffff],
            0xdddddd,
        ),
        _ => return None,
    })
}

impl Default for Theme {
    fn default() -> Self {
        builtin("default").expect("default theme exists")
    }
}

/// Parse a colour string.
pub fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("default") || s.eq_ignore_ascii_case("reset") || s.is_empty() {
        return Ok(Color::Reset);
    }
    if let Some(hex) = s.strip_prefix('#') {
        let expanded: String = if hex.len() == 3 {
            hex.chars().flat_map(|c| [c, c]).collect()
        } else {
            hex.to_string()
        };
        if expanded.len() == 6 {
            let v = u32::from_str_radix(&expanded, 16).map_err(|_| format!("bad colour {s:?}"))?;
            return Ok(rgb(v));
        }
        return Err(format!("bad colour {s:?}"));
    }
    s.parse::<Color>().map_err(|_| format!("bad colour {s:?}"))
}

impl ThemeFile {
    /// Apply this file on top of a base theme.
    pub fn apply(&self, base: &Theme, fallback_name: &str) -> Result<Theme, String> {
        let mut t = base.clone();
        t.name = self
            .name
            .clone()
            .unwrap_or_else(|| fallback_name.to_string());
        let set = |slot: &mut Color, v: &Option<String>| -> Result<(), String> {
            if let Some(v) = v {
                *slot = parse_color(v)?;
            }
            Ok(())
        };
        set(&mut t.bg, &self.main_bg)?;
        set(&mut t.fg, &self.main_fg)?;
        set(&mut t.title, &self.title)?;
        set(&mut t.hi, &self.hi_fg)?;
        set(&mut t.border, &self.border)?;
        set(&mut t.border_focus, &self.border_focus)?;
        set(&mut t.selected_bg, &self.selected_bg)?;
        set(&mut t.selected_fg, &self.selected_fg)?;
        set(&mut t.inactive, &self.inactive_fg)?;
        set(&mut t.graph_text, &self.graph_text)?;
        set(&mut t.meter_bg, &self.meter_bg)?;
        set(&mut t.grad_low, &self.gradient_low)?;
        set(&mut t.grad_mid, &self.gradient_mid)?;
        set(&mut t.grad_high, &self.gradient_high)?;
        set(&mut t.ok, &self.ok)?;
        set(&mut t.warn, &self.warn)?;
        set(&mut t.err, &self.err)?;
        set(&mut t.accent, &self.accent)?;
        Ok(t)
    }
}

/// Serialise a theme back to the TOML file format (used by `cpatop themes --dump`).
pub fn to_toml(t: &Theme) -> String {
    fn c(c: Color) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("\"#{r:02x}{g:02x}{b:02x}\""),
            Color::Reset => "\"default\"".into(),
            other => format!("\"{other}\"").to_lowercase(),
        }
    }
    format!(
        "# cpatop theme\nname = \"{}\"\nmain_bg = {}\nmain_fg = {}\ntitle = {}\nhi_fg = {}\nborder = {}\nborder_focus = {}\nselected_bg = {}\nselected_fg = {}\ninactive_fg = {}\ngraph_text = {}\nmeter_bg = {}\ngradient_low = {}\ngradient_mid = {}\ngradient_high = {}\nok = {}\nwarn = {}\nerr = {}\naccent = {}\n",
        t.name,
        c(t.bg),
        c(t.fg),
        c(t.title),
        c(t.hi),
        c(t.border),
        c(t.border_focus),
        c(t.selected_bg),
        c(t.selected_fg),
        c(t.inactive),
        c(t.graph_text),
        c(t.meter_bg),
        c(t.grad_low),
        c(t.grad_mid),
        c(t.grad_high),
        c(t.ok),
        c(t.warn),
        c(t.err),
        c(t.accent),
    )
}

/// Load every `*.toml` theme in `dir`. Broken files are reported, not fatal.
pub fn load_user_themes(dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (themes, errors);
    };
    let mut paths: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    let base = Theme::default();
    for p in paths {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parsed = std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|s| toml::from_str::<ThemeFile>(&s).map_err(|e| e.to_string()))
            .and_then(|f| f.apply(&base, &stem));
        match parsed {
            Ok(t) => themes.push(t),
            Err(e) => errors.push(format!("theme {}: {e}", p.display())),
        }
    }
    (themes, errors)
}

/// Every available theme: built-ins first, then user themes (which may override by name).
pub fn all_themes(user_dir: Option<&Path>) -> (Vec<Theme>, Vec<String>) {
    let mut out: Vec<Theme> = BUILTIN.iter().filter_map(|n| builtin(n)).collect();
    let mut errors = Vec::new();
    if let Some(dir) = user_dir {
        let (user, errs) = load_user_themes(dir);
        errors = errs;
        for t in user {
            if let Some(slot) = out.iter_mut().find(|b| b.name == t.name) {
                *slot = t;
            } else {
                out.push(t);
            }
        }
    }
    (out, errors)
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Color::Rgb(f(r1, r2), f(g1, g2), f(b1, b2))
        }
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

impl Theme {
    /// Style of the selected row / active tab: colours, or reverse video
    /// when colours are off.
    pub fn selected(&self) -> ratatui::style::Style {
        use ratatui::style::{Modifier, Style};
        if self.no_color {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(self.selected_bg).fg(self.selected_fg)
        }
    }

    /// Low → mid → high gradient at position `t` in `[0, 1]`.
    pub fn gradient(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        if t < 0.5 {
            lerp(self.grad_low, self.grad_mid, t * 2.0)
        } else {
            lerp(self.grad_mid, self.grad_high, (t - 0.5) * 2.0)
        }
    }

    /// Colour for a "higher is better" percentage (success rate, quota left).
    pub fn good_pct(&self, pct: f64, warn_below: f64, err_below: f64) -> Color {
        if pct < err_below {
            self.err
        } else if pct < warn_below {
            self.warn
        } else {
            self.ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_exist() {
        for n in BUILTIN {
            assert_eq!(builtin(n).unwrap().name, n);
        }
        assert!(builtin("nope").is_none());
    }

    #[test]
    fn colours_parse() {
        assert_eq!(parse_color("#ff0000"), Ok(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#f00"), Ok(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("default"), Ok(Color::Reset));
        assert_eq!(parse_color("red"), Ok(Color::Red));
        assert!(parse_color("#12").is_err());
    }

    #[test]
    fn theme_file_round_trip() {
        let t = builtin("nord").unwrap();
        let s = to_toml(&t);
        let f: ThemeFile = toml::from_str(&s).unwrap();
        let back = f.apply(&Theme::default(), "x").unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn gradient_endpoints() {
        let t = Theme::default();
        assert_eq!(t.gradient(0.0), t.grad_low);
        assert_eq!(t.gradient(0.5), t.grad_mid);
        assert_eq!(t.gradient(1.0), t.grad_high);
    }
}
