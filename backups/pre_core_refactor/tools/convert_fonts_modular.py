import os
import re

FONTS_DIR = 'inspo/deltafw/src/ui/fonts'
OUT_DIR = 'crates/ui/src/fonts'

def generate_mod_rs(font_names):
    with open(os.path.join(OUT_DIR, 'mod.rs'), 'w') as f:
        for name in font_names:
            f.write(f"pub mod {name};\n")

def parse_font_file(filepath, filename):
    with open(filepath, 'r') as f:
        content = f.read()
    
    font_name = filename.split('.')[0]
    out_filename = font_name.lower()
    
    # Extract bitmap array
    bitmap_match = re.search(r'const\s+uint8_t\s+(\w+)Bitmaps\[\]\s*(?:PROGMEM\s*)?=\s*\{([^}]*)\}', content, re.MULTILINE | re.DOTALL)
    if not bitmap_match: return None
    bitmap_data_raw = bitmap_match.group(2)
    
    # Extract all hex bytes
    hex_bytes = re.findall(r'0x[0-9a-fA-F]{2}', bitmap_data_raw)
    
    # Format beautifully into 12 columns
    formatted_bitmap = "const BITMAP: &[u8] = &[\n"
    for i in range(0, len(hex_bytes), 12):
        chunk = hex_bytes[i:i+12]
        formatted_bitmap += "    " + ", ".join(chunk) + ",\n"
    formatted_bitmap += "];"
    
    # Extract glyph array
    glyph_match = re.search(r'const\s+GFXglyph\s+(\w+)Glyphs\[\]\s*(?:PROGMEM\s*)?=\s*\{([^}]*)\}', content, re.MULTILINE | re.DOTALL)
    if not glyph_match: return None
    glyph_data = glyph_match.group(2)
    
    # Extract font metadata
    font_match = re.search(r'const\s+GFXfont\s+(\w+)\s*(?:PROGMEM\s*)?=\s*\{[^,]+,\s*[^,]+,\s*([^,]+),\s*([^,]+),\s*([^}]+)\}', content, re.MULTILINE | re.DOTALL)
    if not font_match: return None
    first, last, y_adv = font_match.group(2).strip(), font_match.group(3).strip(), font_match.group(4).strip()
    
    out = []
    out.append("use crate::{GfxGlyph, GfxFont};\n")
    out.append(formatted_bitmap)
    out.append(f"const GLYPHS: &[GfxGlyph] = &[")
    
    for line in glyph_data.split('\n'):
        line = line.strip()
        if not line: continue
        if line.startswith('//'): continue
        
        m = re.match(r'\{\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*,\s*(-?\d+)\s*\}', line)
        if m:
            out.append(f"    GfxGlyph {{ bitmap_offset: {m.group(1)}, width: {m.group(2)}, height: {m.group(3)}, x_advance: {m.group(4)}, x_offset: {m.group(5)}, y_offset: {m.group(6)} }},")
    
    out.append("];")
    out.append(f"pub const FONT: GfxFont = GfxFont {{")
    out.append(f"    bitmap: BITMAP,")
    out.append(f"    glyphs: GLYPHS,")
    out.append(f"    first: {first},")
    out.append(f"    last: {last},")
    out.append(f"    y_advance: {y_adv},")
    out.append("};")
    
    with open(os.path.join(OUT_DIR, f"{out_filename}.rs"), 'w') as f:
        f.write("\n".join(out) + "\n")
        
    return out_filename

font_names = []
for filename in os.listdir(FONTS_DIR):
    if filename.endswith('.h'):
        name = parse_font_file(os.path.join(FONTS_DIR, filename), filename)
        if name:
            font_names.append(name)

generate_mod_rs(font_names)
