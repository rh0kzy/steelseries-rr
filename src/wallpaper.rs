use std::path::PathBuf;
use std::process::Command;

/// Try to locate the current desktop wallpaper (GNOME, XFCE, or feh/nitrogen fallback).
pub fn detect() -> Option<PathBuf> {
    // GNOME / Ubuntu
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(p) = parse_uri_or_path(&s) {
            if p.exists() {
                return Some(p);
            }
        }
    }
    // GNOME dark variant
    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri-dark"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(p) = parse_uri_or_path(&s) {
            if p.exists() {
                return Some(p);
            }
        }
    }
    // XFCE
    if let Ok(out) = Command::new("xfconf-query")
        .args(["-c", "xfce4-desktop", "-p", "/backdrop/screen0/monitor0/last-image"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let p = PathBuf::from(&s);
        if p.exists() {
            return Some(p);
        }
    }
    // Omarchy / Hyprland
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home)
            .join(".local/state/omarchy/current/background");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn parse_uri_or_path(s: &str) -> Option<PathBuf> {
    let s = s.trim().trim_matches('\'');
    if let Some(rest) = s.strip_prefix("file://") {
        Some(PathBuf::from(percent_decode(rest)))
    } else if s.starts_with('/') {
        Some(PathBuf::from(s))
    } else {
        None
    }
}

fn percent_decode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Downscale an image to a grid and return RGB colors per cell, row-major.
pub fn sample_grid(path: &PathBuf, cols: u32, rows: u32) -> Option<Vec<Vec<[u8; 3]>>> {
    let img = image::open(path).ok()?.to_rgb8();
    let small = image::imageops::resize(&img, cols, rows, image::imageops::FilterType::Triangle);
    let mut out = Vec::with_capacity(rows as usize);
    for y in 0..rows {
        let mut row = Vec::with_capacity(cols as usize);
        for x in 0..cols {
            let p = small.get_pixel(x, y);
            row.push([p[0], p[1], p[2]]);
        }
        out.push(row);
    }
    Some(out)
}
