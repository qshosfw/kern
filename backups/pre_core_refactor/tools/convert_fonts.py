import os
import re

FONTS_DIR = 'inspo/deltafw/src/ui/fonts'
OUT_FILE = 'src/gfx/fonts.rs'

def parse_font_file(filepath, filename):
    with open(filepath, 'r') as f:
        content = f.read()
    
    font_name = filename.split('.')[0]
    
    # Extract bitmap array
    bitmap_match = re.search(r'const\s+uint8_t\s+(\w+)Bitmaps\[\]\s*(?:PROGMEM\s*)?=\s*\{([^}]*)\}', content, re.MULTILINE | re.DOTALL)
    if not bitmap_match: return ""
    bitmap_name = bitmap_match.group(1)
    bitmap_data = bitmap_match.group(2).replace('\n', ' ')
    
    # Extract glyph array
    glyph_match = re.search(r'const\s+GFXglyph\s+(\w+)Glyphs\[\]\s*(?:PROGMEM\s*)?=\s*\{([^}]*)\}', content, re.MULTILINE | re.DOTALL)
    if not glyph_match: return ""
    glyph_data = glyph_match.group(2)
    
    # Extract font metadata
    font_match = re.search(r'const\s+GFXfont\s+(\w+)\s*(?:PROGMEM\s*)?=\s*\{[^,]+,\s*[^,]+,\s*([^,]+),\s*([^,]+),\s*([^}]+)\}', content, re.MULTILINE | re.DOTALL)
    if not font_match: return ""
    first, last, y_adv = font_match.group(2).strip(), font_match.group(3).strip(), font_match.group(4).strip()
    
    out = []
    out.append(f"const {font_name.upper()}_BITMAP: &[u8] = &[{bitmap_data}];")
    out.append(f"const {font_name.upper()}_GLYPHS: &[crate::gfx::GfxGlyph] = &[")
    
    for line in glyph_data.split('\n'):
        line = line.strip()
        if not line: continue
        if line.startswith('//'): continue
        
        # match { offset, width, height, advance, x_off, y_off }
        m = re.match(r'\{\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*\}', line)
        if m:
            out.append(f"    crate::gfx::GfxGlyph {{ bitmap_offset: {m.group(1)}, width: {m.group(2)}, height: {m.group(3)}, x_advance: {m.group(4)}, x_offset: {m.group(5)}, y_offset: {m.group(6)} }},")
    
    out.append("];")
    out.append(f"pub const {font_name.upper()}: crate::gfx::GfxFont = crate::gfx::GfxFont {{")
    out.append(f"    bitmap: {font_name.upper()}_BITMAP,")
    out.append(f"    glyphs: {font_name.upper()}_GLYPHS,")
    out.append(f"    first: {first},")
    out.append(f"    last: {last},")
    out.append(f"    y_advance: {y_adv},")
    out.append("};")
    
    return "\n".join(out) + "\n\n"

with open(OUT_FILE, 'w') as out:
    for filename in os.listdir(FONTS_DIR):
        if filename.endswith('.h'):
            out.write(parse_font_file(os.path.join(FONTS_DIR, filename), filename))
