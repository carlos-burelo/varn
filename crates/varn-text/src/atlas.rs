use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

pub struct RasterGlyph {
    pub w: u32,
    pub h: u32,
    pub bx: i32,
    pub by: i32,
    pub advance: f32,
    pub bitmap: Vec<u8>,
}

struct Cache {
    entries: FxHashMap<(u16, u32), RasterGlyph>,
    scaler: swash::scale::ScaleContext,
}

static ATLAS: OnceLock<Mutex<Cache>> = OnceLock::new();

fn cache() -> &'static Mutex<Cache> {
    ATLAS.get_or_init(|| {
        Mutex::new(Cache {
            entries: FxHashMap::default(),
            scaler: swash::scale::ScaleContext::new(),
        })
    })
}

pub fn cache_size() -> usize {
    cache().lock().entries.len()
}

pub struct PackedAtlas {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
    pub placements: Vec<(u32, u32)>,
}

pub fn pack_bitmaps(items: &[(u32, u32, Vec<u8>)], max_row_w: u32) -> PackedAtlas {
    let mut x = 0u32;
    let mut y = 0u32;
    let mut row_h = 0u32;
    let mut placements = Vec::with_capacity(items.len());
    for (w, h, _) in items {
        if *w == 0 || *h == 0 {
            placements.push((0, 0));
            continue;
        }
        if x + w > max_row_w {
            x = 0;
            y += row_h;
            row_h = 0;
        }
        placements.push((x, y));
        x += w;
        row_h = row_h.max(*h);
    }
    let h = y + row_h;
    let mut data = vec![0u8; (max_row_w * h.max(1)) as usize];
    for ((w, h, bmp), (px, py)) in items.iter().zip(&placements) {
        if *w == 0 || *h == 0 {
            continue;
        }
        for row in 0..*h {
            let dst = ((py + row) * max_row_w + px) as usize;
            let src = (row * w) as usize;
            data[dst..dst + *w as usize].copy_from_slice(&bmp[src..src + *w as usize]);
        }
    }
    PackedAtlas {
        w: max_row_w,
        h: h.max(1),
        data,
        placements,
    }
}

pub fn raster_glyph(id: u16, advance: f32, size: f32) -> Result<RasterGlyph, String> {
    if size <= 0.0 || !size.is_finite() {
        return Err("text.raster: size must be positive".to_string());
    }
    let key = (id, size.to_bits());
    {
        let c = cache().lock();
        if let Some(g) = c.entries.get(&key) {
            return Ok(RasterGlyph {
                w: g.w,
                h: g.h,
                bx: g.bx,
                by: g.by,
                advance: g.advance,
                bitmap: g.bitmap.clone(),
            });
        }
    }
    let (data, index) = super::font::sans_bytes()?;
    let font = swash::FontRef::from_index(&data, index as usize)
        .ok_or_else(|| "text.raster: bad font".to_string())?;
    let mut c = cache().lock();
    let mut scaler = c.scaler.builder(font).size(size).hint(true).build();
    let mut image = swash::scale::image::Image::new();
    let ok = swash::scale::Render::new(&[swash::scale::Source::Outline]).render_into(
        &mut scaler,
        id,
        &mut image,
    );
    let image: &swash::scale::image::Image = &image;
    let (w, h, bx, by, bitmap) = if ok {
        let p = image.placement;
        (p.width, p.height, p.left, p.top, image.data.clone())
    } else {
        (0, 0, 0, 0, Vec::new())
    };
    let g = RasterGlyph {
        w,
        h,
        bx,
        by,
        advance,
        bitmap,
    };
    c.entries.insert(
        key,
        RasterGlyph {
            w: g.w,
            h: g.h,
            bx: g.bx,
            by: g.by,
            advance: g.advance,
            bitmap: g.bitmap.clone(),
        },
    );
    Ok(g)
}
