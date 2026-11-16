//! Reverse-engineered SteelSeries KLC protocol (MSI per-key RGB keyboards).
//!
//! Protocol originally documented/reverse-engineered by the msi-perkeyrgb project
//! (Askannz/TauAkiou) and OpenRGB. Packets are 524-byte feature reports sent
//! through `hid_send_feature_report`.

/// Region header codes (byte index 2 of the feature report).
pub const REGION_ALPHANUM: u8 = 0x2a;
pub const REGION_ENTER: u8 = 0x0b;
pub const REGION_MODIFIERS: u8 = 0x18;
pub const REGION_NUMPAD: u8 = 0x24;

/// Number of key slots per region packet.
pub const NB_KEYS: usize = 42;

/// Region keycodes (SteelSeries/MSI encoding). Mirrors msi-perkeyrgb's
/// `REGION_KEYCODES`.
pub const REGION_KEYCODES: &[(&str, &[u8])] = &[
    ("alphanum", &[
        4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,
        28,29,30,31,32,33,34,35,36,37,38,39,58,59,60,61,62,63,
    ]),
    ("enter", &[
        40,49,50,100,135,136,137,138,139,144,145,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    ]),
    ("modifiers", &[
        41,42,43,44,45,46,47,48,51,52,53,54,55,56,57,101,224,225,226,227,
        228,229,230,240,
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    ]),
    ("numpad", &[
        64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,85,
        86,87,88,89,90,91,92,93,94,95,96,97,98,99,
        0,0,0,0,0,0,
    ]),
];

pub fn region_code(name: &str) -> u8 {
    match name {
        "alphanum" => REGION_ALPHANUM,
        "enter" => REGION_ENTER,
        "modifiers" => REGION_MODIFIERS,
        "numpad" => REGION_NUMPAD,
        _ => REGION_ALPHANUM,
    }
}

/// Build a 524-byte feature report that sets one region's keys to `color`.
pub fn make_key_colors_packet(region: &str, color: [u8; 3]) -> Vec<u8> {
    let code = region_code(region);
    let keycodes = REGION_KEYCODES
        .iter()
        .find(|(n, _)| *n == region)
        .map(|(_, k)| *k)
        .unwrap_or(&[]);

    let mut packet = Vec::with_capacity(524);
    packet.extend_from_slice(&[0x0e, 0x00, code, 0x00]);

    // Each entry is 12 bytes: R,G,B, 5 zeros, mode(0x01), zero, keycode.
    for &keycode in keycodes {
        packet.extend_from_slice(&[color[0], color[1], color[2]]);
        packet.extend_from_slice(&[0, 0, 0, 0, 0]);
        packet.push(0x01);
        packet.push(0x00);
        packet.push(keycode);
    }
    // Pad to NB_KEYS entries (should already be NB_KEYS, but be safe).
    while packet.len() < 4 + NB_KEYS * 12 {
        packet.extend_from_slice(&[0; 12]);
    }

    // Trailing 16 bytes.
    packet.extend_from_slice(&[
        0,0,0,0,0,0,0,0,0,0,0,0,0,0,0x08,0x39,
    ]);

    // Ensure exact length 524.
    packet.resize(524, 0);
    packet
}

/// Build the 64-byte output report that latches/applies the colors.
pub fn make_refresh_packet() -> Vec<u8> {
    let mut p = vec![0x09];
    p.resize(64, 0);
    p
}