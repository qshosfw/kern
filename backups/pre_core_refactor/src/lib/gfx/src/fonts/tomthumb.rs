use crate::{GfxFont, GfxGlyph};

const BITMAP: &[u8] = &[
    0x00, 0xE8, 0xB4, 0xBE, 0xFA, 0x79, 0xE4, 0x85, 0x42, 0xDB, 0xD6, 0xC0, 0x6A, 0x40, 0x95, 0x80,
    0xAA, 0x80, 0x5D, 0x00, 0x60, 0xE0, 0x80, 0x25, 0x48, 0x76, 0xDC, 0x59, 0x24, 0x00, 0xC5, 0x4E,
    0xC5, 0x1C, 0xB7, 0x92, 0xF3, 0x1C, 0x73, 0xDE, 0xE5, 0x48, 0xF7, 0xDE, 0xF7, 0x9C, 0xA0, 0x46,
    0x2A, 0x22, 0xE3, 0x80, 0x88, 0xA8, 0xE5, 0x04, 0x57, 0xC6, 0x57, 0xDA, 0xD7, 0x5C, 0x72, 0x46,
    0xD6, 0xDC, 0xF3, 0xCE, 0xF3, 0xC8, 0x73, 0xD6, 0xB7, 0xDA, 0xE9, 0x2E, 0x24, 0xD4, 0xB7, 0x5A,
    0x92, 0x4E, 0x8E, 0xEB, 0x18, 0x80, 0x9D, 0xB9, 0x90, 0x69, 0x99, 0x60, 0xD7, 0x48, 0x00, 0x69,
    0x9A, 0x50, 0xD7, 0xEA, 0x71, 0x1C, 0xE9, 0x24, 0xB6, 0xDE, 0x00, 0xB6, 0xD4, 0x00, 0x8C, 0x6B,
    0xB8, 0x80, 0xB5, 0x5A, 0xB5, 0x24, 0xE5, 0x4E, 0xF2, 0x4E, 0x88, 0x80, 0xE4, 0x9E, 0x54, 0xE0,
    0x90, 0xCE, 0xF0, 0x9A, 0xDC, 0x72, 0x30, 0x2E, 0xD6, 0x77, 0x30, 0x2B, 0xA4, 0x77, 0x94, 0x9A,
    0xDA, 0xB8, 0x20, 0x9A, 0x80, 0x97, 0x6A, 0xC9, 0x2E, 0xF5, 0x6B, 0x50, 0xD6, 0xD0, 0x56, 0xA0,
    0xD6, 0xE8, 0x76, 0xB2, 0x72, 0x40, 0x79, 0xE0, 0x5D, 0x26, 0xB6, 0xF0, 0xB6, 0xA0, 0x8D, 0x6A,
    0xF0, 0xA9, 0x50, 0xB5, 0x94, 0xEF, 0x70, 0x6A, 0x26, 0xD8, 0xC8, 0xAC, 0x78,
];

const GLYPHS: &[GfxGlyph] = &[
    GfxGlyph::new(0, 1, 1, 4, 0, -4),   // 0x20 ' '
    GfxGlyph::new(1, 1, 5, 2, 0, -4),   // 0x21 '!'
    GfxGlyph::new(2, 3, 2, 4, 0, -4),   // 0x22 '"'
    GfxGlyph::new(3, 3, 5, 4, 0, -4),   // 0x23 '#'
    GfxGlyph::new(5, 3, 5, 4, 0, -4),   // 0x24 '$'
    GfxGlyph::new(7, 3, 5, 4, 0, -4),   // 0x25 '%'
    GfxGlyph::new(9, 3, 5, 4, 0, -4),   // 0x26 '&'
    GfxGlyph::new(11, 1, 2, 2, 0, -4),  // 0x27 '''
    GfxGlyph::new(12, 2, 5, 3, 0, -4),  // 0x28 '('
    GfxGlyph::new(14, 2, 5, 3, 0, -4),  // 0x29 ')'
    GfxGlyph::new(16, 3, 3, 4, 0, -4),  // 0x2A '*'
    GfxGlyph::new(18, 3, 3, 4, 0, -3),  // 0x2B '+'
    GfxGlyph::new(20, 2, 2, 3, 0, -1),  // 0x2C ','
    GfxGlyph::new(21, 3, 1, 4, 0, -2),  // 0x2D '-'
    GfxGlyph::new(22, 1, 1, 2, 0, 0),   // 0x2E '.'
    GfxGlyph::new(23, 3, 5, 4, 0, -4),  // 0x2F '/'
    GfxGlyph::new(25, 3, 5, 4, 0, -4),  // 0x30 '0'
    GfxGlyph::new(27, 3, 5, 4, 0, -4),  // 0x31 '1'
    GfxGlyph::new(30, 3, 5, 4, 0, -4),  // 0x32 '2'
    GfxGlyph::new(32, 3, 5, 4, 0, -4),  // 0x33 '3'
    GfxGlyph::new(34, 3, 5, 4, 0, -4),  // 0x34 '4'
    GfxGlyph::new(36, 3, 5, 4, 0, -4),  // 0x35 '5'
    GfxGlyph::new(38, 3, 5, 4, 0, -4),  // 0x36 '6'
    GfxGlyph::new(40, 3, 5, 4, 0, -4),  // 0x37 '7'
    GfxGlyph::new(42, 3, 5, 4, 0, -4),  // 0x38 '8'
    GfxGlyph::new(44, 3, 5, 4, 0, -4),  // 0x39 '9'
    GfxGlyph::new(46, 1, 3, 2, 0, -3),  // 0x3A ':'
    GfxGlyph::new(47, 2, 4, 3, 0, -3),  // 0x3B ';'
    GfxGlyph::new(48, 3, 5, 4, 0, -4),  // 0x3C '<'
    GfxGlyph::new(50, 3, 3, 4, 0, -3),  // 0x3D '='
    GfxGlyph::new(52, 3, 5, 4, 0, -4),  // 0x3E '>'
    GfxGlyph::new(54, 3, 5, 4, 0, -4),  // 0x3F '?'
    GfxGlyph::new(56, 3, 5, 4, 0, -4),  // 0x40 '@'
    GfxGlyph::new(58, 3, 5, 4, 0, -4),  // 0x41 'A'
    GfxGlyph::new(60, 3, 5, 4, 0, -4),  // 0x42 'B'
    GfxGlyph::new(62, 3, 5, 4, 0, -4),  // 0x43 'C'
    GfxGlyph::new(64, 3, 5, 4, 0, -4),  // 0x44 'D'
    GfxGlyph::new(66, 3, 5, 4, 0, -4),  // 0x45 'E'
    GfxGlyph::new(68, 3, 5, 4, 0, -4),  // 0x46 'F'
    GfxGlyph::new(70, 3, 5, 4, 0, -4),  // 0x47 'G'
    GfxGlyph::new(72, 3, 5, 4, 0, -4),  // 0x48 'H'
    GfxGlyph::new(74, 3, 5, 4, 0, -4),  // 0x49 'I'
    GfxGlyph::new(76, 3, 5, 4, 0, -4),  // 0x4A 'J'
    GfxGlyph::new(78, 3, 5, 4, 0, -4),  // 0x4B 'K'
    GfxGlyph::new(80, 3, 5, 4, 0, -4),  // 0x4C 'L'
    GfxGlyph::new(82, 5, 5, 6, 0, -4),  // 0x4D 'M'
    GfxGlyph::new(86, 4, 5, 5, 0, -4),  // 0x4E 'N'
    GfxGlyph::new(89, 4, 5, 5, 0, -4),  // 0x4F 'O'
    GfxGlyph::new(92, 3, 5, 4, 0, -4),  // 0x50 'P'
    GfxGlyph::new(95, 4, 5, 5, 0, -4),  // 0x51 'Q'
    GfxGlyph::new(98, 3, 5, 4, 0, -4),  // 0x52 'R'
    GfxGlyph::new(100, 3, 5, 4, 0, -4), // 0x53 'S'
    GfxGlyph::new(102, 3, 5, 4, 0, -4), // 0x54 'T'
    GfxGlyph::new(104, 3, 5, 4, 0, -4), // 0x55 'U'
    GfxGlyph::new(107, 3, 5, 4, 0, -4), // 0x56 'V'
    GfxGlyph::new(110, 5, 5, 6, 0, -4), // 0x57 'W'
    GfxGlyph::new(114, 3, 5, 4, 0, -4), // 0x58 'X'
    GfxGlyph::new(116, 3, 5, 4, 0, -4), // 0x59 'Y'
    GfxGlyph::new(118, 3, 5, 4, 0, -4), // 0x5A 'Z'
    GfxGlyph::new(120, 3, 5, 4, 0, -4), // 0x5B '['
    GfxGlyph::new(122, 3, 3, 4, 0, -3), // 0x5C '\'
    GfxGlyph::new(124, 3, 5, 4, 0, -4), // 0x5D ']'
    GfxGlyph::new(126, 3, 2, 4, 0, -4), // 0x5E '^'
    GfxGlyph::new(127, 3, 1, 4, 0, 0),  // 0x5F '_'
    GfxGlyph::new(128, 2, 2, 3, 0, -4), // 0x60 '`'
    GfxGlyph::new(129, 3, 4, 4, 0, -3), // 0x61 'a'
    GfxGlyph::new(131, 3, 5, 4, 0, -4), // 0x62 'b'
    GfxGlyph::new(133, 3, 4, 4, 0, -3), // 0x63 'c'
    GfxGlyph::new(135, 3, 5, 4, 0, -4), // 0x64 'd'
    GfxGlyph::new(137, 3, 4, 4, 0, -3), // 0x65 'e'
    GfxGlyph::new(139, 3, 5, 4, 0, -4), // 0x66 'f'
    GfxGlyph::new(141, 3, 5, 4, 0, -3), // 0x67 'g'
    GfxGlyph::new(143, 3, 5, 4, 0, -4), // 0x68 'h'
    GfxGlyph::new(145, 1, 5, 2, 0, -4), // 0x69 'i'
    GfxGlyph::new(146, 3, 6, 4, 0, -4), // 0x6A 'j'
    GfxGlyph::new(149, 3, 5, 4, 0, -4), // 0x6B 'k'
    GfxGlyph::new(151, 3, 5, 4, 0, -4), // 0x6C 'l'
    GfxGlyph::new(153, 5, 4, 6, 0, -3), // 0x6D 'm'
    GfxGlyph::new(156, 3, 4, 4, 0, -3), // 0x6E 'n'
    GfxGlyph::new(158, 3, 4, 4, 0, -3), // 0x6F 'o'
    GfxGlyph::new(160, 3, 5, 4, 0, -3), // 0x70 'p'
    GfxGlyph::new(162, 3, 5, 4, 0, -3), // 0x71 'q'
    GfxGlyph::new(164, 3, 4, 4, 0, -3), // 0x72 'r'
    GfxGlyph::new(166, 3, 4, 4, 0, -3), // 0x73 's'
    GfxGlyph::new(168, 3, 5, 4, 0, -4), // 0x74 't'
    GfxGlyph::new(170, 3, 4, 4, 0, -3), // 0x75 'u'
    GfxGlyph::new(172, 3, 4, 4, 0, -3), // 0x76 'v'
    GfxGlyph::new(174, 5, 4, 6, 0, -3), // 0x77 'w'
    GfxGlyph::new(177, 3, 4, 4, 0, -3), // 0x78 'x'
    GfxGlyph::new(179, 3, 5, 4, 0, -3), // 0x79 'y'
    GfxGlyph::new(181, 3, 4, 4, 0, -3), // 0x7A 'z'
    GfxGlyph::new(183, 3, 5, 4, 0, -4), // 0x7B '{'
    GfxGlyph::new(185, 1, 5, 2, 0, -4), // 0x7C '|'
    GfxGlyph::new(186, 3, 5, 4, 0, -4), // 0x7D '}'
    GfxGlyph::new(188, 3, 2, 4, 0, -4), // 0x7E '~'
];

pub const FONT: GfxFont = GfxFont {
    bitmap: BITMAP,
    glyphs: GLYPHS,
    first: 0x20,
    last: 0x7E,
    y_advance: 6,
};
