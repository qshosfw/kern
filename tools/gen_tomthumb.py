import re

with open('inspo/fagci-k1-fw/src/ui/fonts/TomThumb.h', 'r') as f:
    text = f.read()

# Extract bitmap
bitmap_match = re.search(r'const uint8_t TomThumbBitmaps\[\] PROGMEM = \{(.*?)\};', text, re.DOTALL)
bitmap_str = bitmap_match.group(1).strip()

# Extract glyphs
glyphs_match = re.search(r'const GFXglyph TomThumbGlyphs\[\] PROGMEM = \{(.*?)\};', text, re.DOTALL)
glyphs_str = glyphs_match.group(1).strip()

glyph_lines = []
for line in glyphs_str.split('\n'):
    line = line.strip()
    if not line: continue
    # {0, 1, 1, 4, 0, -4},   // 0x20 ' '
    m = re.match(r'\{(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*(-?\d+),\s*(-?\d+)\},?(.*)', line)
    if m:
        offset, w, h, adv, xo, yo, comment = m.groups()
        glyph_lines.append(f"    GfxGlyph {{ bitmap_offset: {offset}, width: {w}, height: {h}, x_advance: {adv}, x_offset: {xo}, y_offset: {yo} }}, {comment}")

out = f"""use crate::{{GfxGlyph, GfxFont}};

const BITMAP: &[u8] = &[
    {bitmap_str}
];

const GLYPHS: &[GfxGlyph] = &[
{chr(10).join(glyph_lines)}
];

pub const FONT: GfxFont = GfxFont {{
    bitmap: BITMAP,
    glyphs: GLYPHS,
    first: 0x20,
    last: 0x7E,
    y_advance: 6,
}};
"""

with open('crates/gfx/src/fonts/tomthumb.rs', 'w') as f:
    f.write(out)
