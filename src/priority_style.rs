// SPDX-License-Identifier: MPL-2.0
//! Small semantic markers; surfaces and selected/focus styling remain native.
use crate::tasks::Priority;
use cosmic::{Element, Theme, iced::Color, widget};

pub fn marker<'a, Message: 'a>(priority: Priority) -> Element<'a, Message> {
    widget::text::caption(priority.short_label())
        .class(color(&cosmic::theme::active(), priority))
        .into()
}

fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    )
}
fn luminance(color: Color) -> f32 {
    fn linear(v: f32) -> f32 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}
fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn surfaces(theme: &Theme) -> [Color; 3] {
    let t = theme.cosmic();
    [
        t.bg_color().into(),
        t.primary_container_color().into(),
        t.secondary_container_color().into(),
    ]
}
fn color(theme: &Theme, priority: Priority) -> Color {
    let t = theme.cosmic();
    let palette = &t.palette;
    let base: Color = match priority {
        Priority::P3 => palette.accent_red,
        Priority::P2 => palette.accent_blue,
        Priority::P1 => palette.accent_yellow,
        Priority::P0 => palette.neutral_5,
    }
    .into();
    let foreground: Color = t.on_bg_color().into();
    let muted = mix(base, foreground, 0.2);
    let backgrounds = surfaces(theme);
    // Retain semantic hue while adjusting only the small marker for contrast.
    let target = if t.is_dark {
        Color::WHITE
    } else {
        Color::BLACK
    };
    for step in 0..=100 {
        let candidate = mix(muted, target, step as f32 / 100.0);
        if backgrounds.iter().all(|&bg| contrast(candidate, bg) >= 4.5) {
            return candidate;
        }
    }
    foreground
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_markers_contrast_with_standard_light_and_dark_surfaces() {
        for theme in [
            Theme::light(),
            Theme::dark(),
            Theme::light_hc(),
            Theme::dark_hc(),
        ] {
            for priority in Priority::ALL {
                let marker = color(&theme, priority);
                for bg in surfaces(&theme) {
                    assert!(
                        contrast(marker, bg) >= 4.5,
                        "{priority:?}: {}",
                        contrast(marker, bg)
                    );
                }
            }
        }
    }
}
