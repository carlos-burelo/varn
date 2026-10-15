use super::font;

pub struct ShapedGlyph {
    pub id: u16,
    pub advance: f32,
    pub offset_x: f32,
    pub cluster: u32,
}

pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
}

pub fn fonts() -> Vec<String> {
    font::fonts()
}

pub fn shape_run(text: &str, size: f32) -> Result<ShapedRun, String> {
    if size <= 0.0 || !size.is_finite() {
        return Err("text.shape: size must be positive".to_string());
    }
    let (data, index) = font::sans_bytes()?;
    let font = swash::FontRef::from_index(&data, index as usize)
        .ok_or_else(|| "text.shape: bad font".to_string())?;
    let mut ctx = swash::shape::ShapeContext::new();
    let mut shaper = ctx.builder(font).size(size).build();
    shaper.add_str(text);
    let mut glyphs = Vec::new();
    let mut width = 0.0f32;
    shaper.shape_with(|cluster| {
        for g in cluster.glyphs {
            glyphs.push(ShapedGlyph {
                id: g.id,
                advance: g.advance,
                offset_x: g.x,
                cluster: cluster.source.start,
            });
            width += g.advance;
        }
    });
    Ok(ShapedRun { glyphs, width })
}

pub fn font_ascent(size: f32) -> Result<f32, String> {
    if size <= 0.0 || !size.is_finite() {
        return Err("text.ascent: size must be positive".to_string());
    }
    let (data, index) = font::sans_bytes()?;
    let font = swash::FontRef::from_index(&data, index as usize)
        .ok_or_else(|| "text.ascent: bad font".to_string())?;
    let mut ctx = swash::shape::ShapeContext::new();
    let shaper = ctx.builder(font).size(size).build();
    Ok(shaper.metrics().ascent)
}

pub fn font_descent(size: f32) -> Result<f32, String> {
    if size <= 0.0 || !size.is_finite() {
        return Err("text.descent: size must be positive".to_string());
    }
    let (data, index) = font::sans_bytes()?;
    let font = swash::FontRef::from_index(&data, index as usize)
        .ok_or_else(|| "text.descent: bad font".to_string())?;
    let mut ctx = swash::shape::ShapeContext::new();
    let shaper = ctx.builder(font).size(size).build();
    Ok(shaper.metrics().descent)
}

pub fn wrap_lines(text: &str, size: f32, max_width: f32) -> Result<Vec<String>, String> {
    if size <= 0.0 || !size.is_finite() {
        return Err("text.wrap: size must be positive".to_string());
    }
    if max_width <= 0.0 || !max_width.is_finite() {
        return Err("text.wrap: max width must be positive".to_string());
    }
    let space = shape_run(" ", size)?.width;
    let space = if space > 0.0 { space } else { size * 0.25 };
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f32;
    let push_word =
        |word: &str, w: f32, lines: &mut Vec<String>, cur: &mut String, cur_w: &mut f32| {
            if cur.is_empty() {
                cur.push_str(word);
                *cur_w = w;
            } else if *cur_w + space + w <= max_width {
                cur.push(' ');
                cur.push_str(word);
                *cur_w += space + w;
            } else {
                lines.push(std::mem::take(cur));
                cur.push_str(word);
                *cur_w = w;
            }
        };
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        let w = shape_run(word, size)?.width;
        if w <= max_width {
            push_word(word, w, &mut lines, &mut cur, &mut cur_w);
            continue;
        }
        for ch in word.chars() {
            let mut buf = [0u8; 4];
            let cw = shape_run(ch.encode_utf8(&mut buf), size)?.width;
            let cw = if cw > 0.0 { cw } else { size * 0.5 };
            push_word(&ch.to_string(), cw, &mut lines, &mut cur, &mut cur_w);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    Ok(lines)
}
