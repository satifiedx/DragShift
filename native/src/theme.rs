//! The matte palette: espresso + terracotta. Dark or light follows Windows ("Apps" theme)
//! unless Settings pins one. Same tokens as dragshift/glass.py on the Qt side.

use crate::gfx::Color;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub panel: Color,
    pub sector: Color,
    pub raised: Color,
    pub line: Color,
    pub text: Color,
    pub dim: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub good: Color,
    pub bad: Color,
    pub shadow: f32,
}

const fn hex(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const DARK: Palette = Palette {
    bg: hex(0x211D1B),
    panel: hex(0x2D2826),
    sector: hex(0x383230),
    raised: hex(0x443D3A),
    line: hex(0x4A423E),
    text: hex(0xEDE6DF),
    dim: hex(0xA89D94),
    accent: hex(0xC98B6B),
    on_accent: hex(0x211D1B),
    good: hex(0x9DB38A),
    bad: hex(0xC47A7A),
    shadow: 0.40,
};

pub const LIGHT: Palette = Palette {
    bg: hex(0xF4EFEA),
    panel: hex(0xEBE4DD),
    sector: hex(0xE2D9D0),
    raised: hex(0xD8CDC2),
    line: hex(0xD2C6BA),
    text: hex(0x2B2420),
    dim: hex(0x74675E),
    accent: hex(0xB8724F),
    on_accent: hex(0xFFF8F2),
    good: hex(0x6F8A5B),
    bad: hex(0xB05A5A),
    shadow: 0.20,
};

/// Is Windows set to light mode for apps?
fn system_light() -> bool {
    use crate::util::wide;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::*;
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut v: u32 = 0;
    let mut n = 4u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None,
                     Some(&mut v as *mut u32 as *mut _), Some(&mut n)).is_ok() && v == 1
    }
}

pub fn current() -> Palette {
    match crate::settings::get().theme.as_str() {
        "dark" => DARK,
        "light" => LIGHT,
        _ if system_light() => LIGHT,
        _ => DARK,
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

/// Exponential approach that looks the same at 60 Hz and 240 Hz: `rate` is the share of the
/// remaining distance covered per 1/60 s.
pub fn approach(v: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let k = 1.0 - (1.0 - rate).powf(dt * 60.0);
    let nv = v + (target - v) * k;
    if (nv - target).abs() < 0.002 { target } else { nv }
}
