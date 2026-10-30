//! Image sprites for the AI cursor: decode once, composite with real alpha.
//!
//! The vector artwork is drawn with GDI and gets its alpha from supersample
//! coverage, which is fine for shapes but destroys images: a semi-transparent
//! PNG pixel has no "key colour" to detect, so it would be rendered fully opaque
//! or fully transparent. Images therefore take a separate path — decoded to
//! straight RGBA here and composited onto the finished ARGB surface with proper
//! source-over blending.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::AnimationDecoder;

/// A decoded sprite: one frame for stills, N frames for animated GIFs.
pub struct Sprite {
    pub w: u32,
    pub h: u32,
    pub frames: Vec<Vec<u8>>, // straight (non-premultiplied) RGBA8
    pub delays_ms: Vec<u32>,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub mtime_ms: u128,
}

impl Sprite {
    pub fn total_ms(&self) -> u64 {
        self.delays_ms.iter().map(|d| *d as u64).sum::<u64>().max(1)
    }

    /// Which frame to show `elapsed_ms` into the animation.
    pub fn frame_at(&self, elapsed_ms: u64) -> usize {
        if self.frames.len() <= 1 {
            return 0;
        }
        let mut t = elapsed_ms % self.total_ms();
        for (i, d) in self.delays_ms.iter().enumerate() {
            let d = (*d).max(10) as u64;
            if t < d {
                return i;
            }
            t -= d;
        }
        self.frames.len() - 1
    }

    pub fn aspect(&self) -> f64 {
        if self.h == 0 {
            1.0
        } else {
            self.w as f64 / self.h as f64
        }
    }
}

fn mtime_ms(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn is_svg(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("svg")
    )
}

/// Rasterise an SVG at a square box; the sprite is later scaled anyway.
fn load_svg(path: &Path, box_px: u32) -> Result<Sprite, String> {
    let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt)
        .map_err(|e| format!("parse svg {}: {e}", path.display()))?;
    let size = tree.size();
    let (sw, sh) = (size.width().max(1.0), size.height().max(1.0));
    let scale = box_px as f32 / sw.max(sh);
    let w = (sw * scale).round().max(1.0) as u32;
    let h = (sh * scale).round().max(1.0) as u32;

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| "svg pixmap alloc failed".to_string())?;
    let ts = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, ts, &mut pixmap.as_mut());

    // tiny-skia stores premultiplied RGBA; convert to straight alpha.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.data().chunks_exact(4) {
        let a = px[3] as u32;
        if a == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let un = |c: u8| ((c as u32 * 255) / a).min(255) as u8;
            rgba.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), a as u8]);
        }
    }
    Ok(Sprite {
        w,
        h,
        frames: vec![rgba],
        delays_ms: vec![0],
        path: path.to_path_buf(),
        mtime_ms: mtime_ms(path),
    })
}

fn load_raster(path: &Path) -> Result<Sprite, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    // Animated GIF: `image` can hand back every frame with its delay.
    if ext == "gif" {
        let file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
            .map_err(|e| format!("gif {}: {e}", path.display()))?;
        let frames = decoder
            .into_frames()
            .collect_frames()
            .map_err(|e| format!("gif frames {}: {e}", path.display()))?;
        if frames.is_empty() {
            return Err(format!("gif {} has no frames", path.display()));
        }
        let (w, h) = (frames[0].buffer().width(), frames[0].buffer().height());
        let mut rgba_frames = Vec::with_capacity(frames.len());
        let mut delays = Vec::with_capacity(frames.len());
        for f in frames {
            delays.push(f.delay().numer_denom_ms().0.max(20));
            rgba_frames.push(f.into_buffer().into_raw());
        }
        return Ok(Sprite {
            w,
            h,
            frames: rgba_frames,
            delays_ms: delays,
            path: path.to_path_buf(),
            mtime_ms: mtime_ms(path),
        });
    }

    let img = image::ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .with_guessed_format()
        .map_err(|e| format!("guess {}: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("decode {}: {e}", path.display()))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok(Sprite {
        w,
        h,
        frames: vec![rgba.into_raw()],
        delays_ms: vec![0],
        path: path.to_path_buf(),
        mtime_ms: mtime_ms(path),
    })
}

pub fn load(path: &Path) -> Result<Sprite, String> {
    if !path.exists() {
        return Err(format!("not found: {}", path.display()));
    }
    if is_svg(path) {
        load_svg(path, 256)
    } else {
        load_raster(path)
    }
}

/// Decoded sprites, reloaded when the file's mtime changes so iterating on the
/// artwork needs no restart.
#[derive(Default)]
pub struct SpriteCache {
    entries: HashMap<PathBuf, Result<Sprite, String>>,
}

impl SpriteCache {
    pub fn get(&mut self, path: &Path) -> Option<&Result<Sprite, String>> {
        let now = mtime_ms(path);
        let stale = match self.entries.get(path) {
            None => true,
            Some(Ok(s)) => s.mtime_ms != now,
            Some(Err(_)) => true, // retry: the user may have just fixed the file
        };
        if stale {
            self.entries.insert(path.to_path_buf(), load(path));
        }
        self.entries.get(path)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Composite one sprite frame onto a premultiplied-BGRA destination surface.
///
/// `dst` is `dw * dh * 4` bytes of B,G,R,A where the colour channels are already
/// multiplied by alpha — the layout `UpdateLayeredWindow` expects. Source pixels
/// are straight RGBA, so each tap is premultiplied before interpolation (doing it
/// the other way round bleeds the colour of transparent pixels into edges).
#[allow(clippy::too_many_arguments)]
pub fn composite(
    dst: &mut [u8],
    dw: i32,
    dh: i32,
    dst_x: i32,
    dst_y: i32,
    src: &[u8],
    sw: u32,
    sh: u32,
    target_w: u32,
    target_h: u32,
    opacity: f64,
    flip_x: bool,
) {
    if target_w == 0 || target_h == 0 || sw == 0 || sh == 0 {
        return;
    }
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let sx_scale = sw as f64 / target_w as f64;
    let sy_scale = sh as f64 / target_h as f64;

    for ty in 0..target_h as i32 {
        let py = dst_y + ty;
        if py < 0 || py >= dh {
            continue;
        }
        let v = (ty as f64 + 0.5) * sy_scale - 0.5;
        let y0 = v.floor().max(0.0) as i64;
        let y1 = (y0 + 1).min(sh as i64 - 1);
        let fy = (v - y0 as f64).clamp(0.0, 1.0);

        for tx in 0..target_w as i32 {
            let px = dst_x + tx;
            if px < 0 || px >= dw {
                continue;
            }
            let tx_flipped = if flip_x { target_w as i32 - 1 - tx } else { tx };
            let u = (tx_flipped as f64 + 0.5) * sx_scale - 0.5;
            let x0 = u.floor().max(0.0) as i64;
            let x1 = (x0 + 1).min(sw as i64 - 1);
            let fx = (u - x0 as f64).clamp(0.0, 1.0);

            // premultiplied bilinear taps: (r*a, g*a, b*a, a)
            let mut acc = [0.0f64; 4];
            for (xi, yy, wy) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)),
                                 (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] {
                if wy <= 0.0 {
                    continue;
                }
                let i = ((yy as u32 * sw + xi as u32) * 4) as usize;
                let a = src[i + 3] as f64 / 255.0;
                acc[0] += src[i] as f64 * a * wy;
                acc[1] += src[i + 1] as f64 * a * wy;
                acc[2] += src[i + 2] as f64 * a * wy;
                acc[3] += a * wy;
            }
            let sa = (acc[3] * opacity).clamp(0.0, 1.0);
            if sa <= 0.0 {
                continue;
            }
            let inv = 1.0 - sa;
            let o = ((py * dw + px) * 4) as usize;
            for (c, chan) in [(0usize, 2usize), (1, 1), (2, 0)] {
                // acc is premultiplied already; scale by opacity and blend over dst
                let s = acc[c] * opacity;
                let d = dst[o + chan] as f64;
                dst[o + chan] = (s + d * inv).round().clamp(0.0, 255.0) as u8;
            }
            let d = dst[o + 3] as f64;
            dst[o + 3] = ((sa * 255.0) + d * inv).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Rotate an RGBA frame by `deg` degrees about its centre, returning a new
/// buffer sized to the rotated bounding box.
///
/// Deliberately separate from `composite`: rotating the source once and letting the
/// existing scaler paste it keeps the tested path untouched, and these frames are
/// small enough that the extra copy does not matter.
pub fn rotate_rgba(src: &[u8], sw: u32, sh: u32, deg: f64) -> (Vec<u8>, u32, u32) {
    if deg.abs() < 0.01 || sw == 0 || sh == 0 {
        return (src.to_vec(), sw, sh);
    }
    let rad = deg.to_radians();
    let (sin, cos) = (rad.sin(), rad.cos());
    let (fw, fh) = (sw as f64, sh as f64);
    let nw = (fw * cos.abs() + fh * sin.abs()).ceil().max(1.0) as u32;
    let nh = (fw * sin.abs() + fh * cos.abs()).ceil().max(1.0) as u32;
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    let (cx, cy) = (fw / 2.0, fh / 2.0);
    let (ncx, ncy) = (nw as f64 / 2.0, nh as f64 / 2.0);
    for y in 0..nh {
        for x in 0..nw {
            // inverse-rotate the destination pixel back into the source
            let dx = x as f64 + 0.5 - ncx;
            let dy = y as f64 + 0.5 - ncy;
            let sx = dx * cos + dy * sin + cx;
            let sy = -dx * sin + dy * cos + cy;
            if sx < 0.0 || sy < 0.0 || sx >= fw || sy >= fh {
                continue;
            }
            let si = ((sy as u32 * sw + sx as u32) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    (out, nw, nh)
}
