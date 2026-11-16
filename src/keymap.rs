//! Simplified ANSI layout for the alphanum region (42 keys, 3 rows of 14).
//! Keycodes come from the reverse-engineered SteelSeries/MSI encoding.

pub const KEY_ROWS: &[&[u8]] = &[
    &[
        4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
    ],
    &[
        18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    ],
    &[
        32, 33, 34, 35, 36, 37, 38, 39, 58, 59, 60, 61, 62, 63,
    ],
];

pub const COLS: u32 = 14;
pub const ROWS: u32 = 3;
