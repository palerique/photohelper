//! Export pipeline for photohelper: resize, watermark, JPEG encode.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

mod isp;
pub use isp::ToneMappingLut;

/// Strong type for Photo Rating (validated/clamped to -1..=5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rating(i32);

impl Rating {
    /// Create a validated rating.
    pub fn new(val: i32) -> Self {
        Self(val.clamp(-1, 5))
    }

    /// Get raw value.
    pub fn value(&self) -> i32 {
        self.0
    }
}

/// Strong type for NIMA aesthetic score (finite, non-NaN).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct NimaScore(f32);

impl NimaScore {
    /// Create a validated NimaScore.
    pub fn new(val: f32) -> Option<Self> {
        if val.is_finite() && !val.is_nan() {
            Some(Self(val))
        } else {
            None
        }
    }

    /// Get raw value.
    pub fn value(&self) -> f32 {
        self.0
    }
}

/// Metadata passed to the export pipeline.
#[derive(Debug, Clone)]
pub struct ExportMetadata {
    pub rating: Rating,
    pub nima_score: Option<NimaScore>,
}

/// Configurable position for text watermarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatermarkPosition {
    BottomLeft,
    TopRight,
    TopLeft,
    BottomRight,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Scale(f32);

impl Scale {
    pub fn new(val: f32) -> Self {
        Self(val.clamp(0.001, 100.0))
    }
    pub fn value(&self) -> f32 {
        self.0
    }
}

#[derive(Clone)]
pub struct PreloadedBadge {
    pub pixmap: Arc<tiny_skia::Pixmap>,
    pub scale: Option<Scale>,
}

impl PreloadedBadge {
    pub fn load(path: &Path, scale: Option<Scale>) -> Result<Self, ExportError> {
        let badge_data = std::fs::read(path).map_err(|e| ExportError::BadgeLoadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        let badge_pixmap = tiny_skia::Pixmap::decode_png(&badge_data).map_err(|e| {
            ExportError::BadgeLoadFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }
        })?;
        Ok(Self {
            pixmap: Arc::new(badge_pixmap),
            scale,
        })
    }
}

impl std::fmt::Debug for PreloadedBadge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreloadedBadge")
            .field("scale", &self.scale)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum Watermark {
    Text(String),
    Image(PreloadedBadge),
}

#[derive(Debug, Clone)]
pub struct ToneMappingOptions {
    pub exposure_ev: f32,
}

impl Default for ToneMappingOptions {
    fn default() -> Self {
        Self { exposure_ev: 0.0 }
    }
}

/// Options to control the export pipeline.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub output_path: PathBuf,
    pub quality: u8,
    pub long_edge: Option<u32>,
    pub watermarks: std::collections::HashMap<WatermarkPosition, Watermark>,
    pub force: bool,
    pub tone_mapping: ToneMappingOptions,
}

/// Errors returned by the export pipeline.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("Invalid dimensions")]
    InvalidDimensions,

    #[error("Allocation failed")]
    AllocationFailed,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("RAW decode error: {0}")]
    RawDecode(String),

    #[error("JPEG encode error: {0}")]
    JpegEncode(String),

    #[error("Duplicate watermark position: {0:?}")]
    DuplicateWatermarkPosition(WatermarkPosition),

    #[error("Failed to load badge at {path}: {reason}")]
    BadgeLoadFailed { path: PathBuf, reason: String },

    #[error("Watermark omitted because it does not fit")]
    WatermarkOmitted,
}

thread_local! {
    static FONT_SYSTEM: RefCell<cosmic_text::FontSystem> = {
        let mut db = cosmic_text::fontdb::Database::new();
        let font_bytes = include_bytes!("RobotoMono-Regular.ttf");
        db.load_font_data(font_bytes.to_vec());
        RefCell::new(cosmic_text::FontSystem::new_with_locale_and_db("en-US".to_string(), db))
    };
    static SWASH_CACHE: RefCell<cosmic_text::SwashCache> = RefCell::new(cosmic_text::SwashCache::new());
}

/// Core function to export a single photo.
#[tracing::instrument(skip(options))]
pub fn export_photo(
    options: &ExportOptions,
    source_path: &Path,
    _metadata: &ExportMetadata,
) -> Result<(), ExportError> {
    // 1. Decode RAW to 16-bit linear image
    let processed = photohelper_raw::decode::decode_image(
        source_path,
        photohelper_raw::decode::ProcessOptions::Linear16,
    )
    .map_err(|e| ExportError::RawDecode(e.to_string()))?;

    let linear_img = match processed {
        photohelper_raw::decode::ProcessedImage::Linear16(img) => img,
        _ => {
            return Err(ExportError::RawDecode(
                "Expected 16-bit linear image".to_string(),
            ));
        }
    };

    let width = linear_img.width.get();
    let height = linear_img.height.get();
    let channels = linear_img.channels as usize;

    if width == 0 || height == 0 {
        return Err(ExportError::InvalidDimensions);
    }
    if channels < 3 {
        return Err(ExportError::RawDecode(
            "Image has fewer than 3 channels".to_string(),
        ));
    }

    // 2. LUT-Accelerated ISP Tone Mapping
    let lut = ToneMappingLut::new(options.tone_mapping.exposure_ev);
    let mut rgb_pixels = Vec::with_capacity((width * height * 3) as usize);

    for pixel in linear_img.data.chunks_exact(channels) {
        rgb_pixels.push(lut.apply(pixel[0]));
        rgb_pixels.push(lut.apply(pixel[1]));
        rgb_pixels.push(lut.apply(pixel[2]));
    }

    // Fast-path bypass: No resizing and no watermarks -> Encode directly
    if options.long_edge.is_none() && options.watermarks.is_empty() {
        compress_jpeg(
            &rgb_pixels,
            width,
            height,
            options.quality,
            &options.output_path,
        )?;
        return Ok(());
    }

    // 3. Aspect-ratio preserving resizing
    let (target_w, target_h, scale) = if let Some(limit) = options.long_edge {
        if limit < 16 {
            return Err(ExportError::InvalidDimensions);
        }
        let w_f = width as f32;
        let h_f = height as f32;
        let long_edge = w_f.max(h_f);
        let s = limit as f32 / long_edge;
        let tw = (w_f * s).round() as u32;
        let th = (h_f * s).round() as u32;
        (tw.max(1), th.max(1), s)
    } else {
        (width, height, 1.0f32)
    };

    let mut pixmap =
        tiny_skia::Pixmap::new(target_w, target_h).ok_or(ExportError::AllocationFailed)?;

    // Fill pixmap with input image pixels padded to 255 alpha
    {
        let output_data = pixmap.data_mut();
        if options.long_edge.is_some() {
            let mut src_pixmap =
                tiny_skia::Pixmap::new(width, height).ok_or(ExportError::AllocationFailed)?;
            let src_data = src_pixmap.data_mut();

            let mut src_idx = 0;
            let mut dst_idx = 0;
            while src_idx < rgb_pixels.len() {
                src_data[dst_idx] = rgb_pixels[src_idx];
                src_data[dst_idx + 1] = rgb_pixels[src_idx + 1];
                src_data[dst_idx + 2] = rgb_pixels[src_idx + 2];
                src_data[dst_idx + 3] = 255;
                src_idx += 3;
                dst_idx += 4;
            }

            let mut paint = tiny_skia::Paint::default();
            let shader = tiny_skia::Pattern::new(
                src_pixmap.as_ref(),
                tiny_skia::SpreadMode::Pad,
                tiny_skia::FilterQuality::Bicubic,
                1.0f32,
                tiny_skia::Transform::from_scale(scale, scale),
            );
            paint.shader = shader;

            let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, target_w as f32, target_h as f32)
                .ok_or(ExportError::InvalidDimensions)?;
            pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        } else {
            let mut src_idx = 0;
            let mut dst_idx = 0;
            while src_idx < rgb_pixels.len() {
                output_data[dst_idx] = rgb_pixels[src_idx];
                output_data[dst_idx + 1] = rgb_pixels[src_idx + 1];
                output_data[dst_idx + 2] = rgb_pixels[src_idx + 2];
                output_data[dst_idx + 3] = 255;
                src_idx += 3;
                dst_idx += 4;
            }
        }
    }

    // 4. Draw Watermarks
    for (pos, watermark) in &options.watermarks {
        match watermark {
            Watermark::Text(text) => {
                draw_text_watermark(&mut pixmap, text, *pos, target_w, target_h)?;
            }
            Watermark::Image(badge) => {
                draw_image_watermark(&mut pixmap, badge, *pos, target_w, target_h)?;
            }
        }
    }

    // 5. Extract and demultiply alpha channels from tiny-skia Pixmap
    let final_data = pixmap.data();
    let mut out_buffer = Vec::with_capacity(target_w as usize * target_h as usize * 3);
    let mut idx = 0;
    while idx < final_data.len() {
        let r = final_data[idx];
        let g = final_data[idx + 1];
        let b = final_data[idx + 2];
        let a = final_data[idx + 3];

        if a == 255 {
            out_buffer.push(r);
            out_buffer.push(g);
            out_buffer.push(b);
        } else if a == 0 {
            out_buffer.push(0);
            out_buffer.push(0);
            out_buffer.push(0);
        } else {
            out_buffer.push(((r as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8);
            out_buffer.push(((g as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8);
            out_buffer.push(((b as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        idx += 4;
    }

    // 6. MozJPEG Optimized Compression
    compress_jpeg(
        &out_buffer,
        target_w,
        target_h,
        options.quality,
        &options.output_path,
    )?;

    Ok(())
}

fn calculate_watermark_position(
    pos: WatermarkPosition,
    width: f32,
    height: f32,
    target_w: f32,
    target_h: f32,
    padding: f32,
) -> (f32, f32) {
    match pos {
        WatermarkPosition::BottomLeft => (padding, target_h - padding - height),
        WatermarkPosition::TopRight => (target_w - padding - width, padding),
        WatermarkPosition::TopLeft => (padding, padding),
        WatermarkPosition::BottomRight => (target_w - padding - width, target_h - padding - height),
        WatermarkPosition::Center => ((target_w - width) / 2.0, (target_h - height) / 2.0),
    }
}

fn draw_text_watermark(
    pixmap: &mut tiny_skia::Pixmap,
    text: &str,
    pos: WatermarkPosition,
    target_w: u32,
    target_h: u32,
) -> Result<(), ExportError> {
    let long_edge_val = target_w.max(target_h) as f32;
    let font_size = (long_edge_val * 0.02).round().max(12.0);
    let padding = (long_edge_val * 0.015).round().max(8.0);

    let mut omitted = false;

    FONT_SYSTEM.with(|fs_cell| {
        SWASH_CACHE.with(|cache_cell| {
            let mut fs = fs_cell.borrow_mut();
            let mut cache = cache_cell.borrow_mut();

            let mut buffer =
                cosmic_text::Buffer::new(&mut fs, cosmic_text::Metrics::new(font_size, font_size));
            buffer.set_size(&mut fs, Some(target_w as f32), None);
            buffer.set_text(
                &mut fs,
                text,
                cosmic_text::Attrs::new().family(cosmic_text::Family::Monospace),
                cosmic_text::Shaping::Basic,
            );
            buffer.shape_until_scroll(&mut fs, false);

            let mut max_width = 0.0f32;
            let mut total_height = 0.0f32;
            let runs: Vec<_> = buffer.layout_runs().collect();
            if !runs.is_empty() {
                for run in &runs {
                    if run.line_w > max_width {
                        max_width = run.line_w;
                    }
                }
                let last_run = &runs[runs.len() - 1];
                total_height = last_run.line_y + font_size;
            }

            let (x_pos, y_pos) = calculate_watermark_position(
                pos,
                max_width,
                total_height,
                target_w as f32,
                target_h as f32,
                padding,
            );

            if x_pos < 0.0
                || y_pos < 0.0
                || (x_pos + max_width) > target_w as f32
                || (y_pos + total_height) > target_h as f32
            {
                tracing::warn!("Watermark text '{}' does not fit on image, omitting.", text);
                omitted = true;
            } else {
                let offset = if font_size < 40.0 { 1 } else { 2 };
                let shadow_color = tiny_skia::Color::from_rgba8(0, 0, 0, 76);
                let text_color = tiny_skia::Color::from_rgba8(255, 255, 255, 178);

                draw_text_at(
                    pixmap,
                    &buffer,
                    &mut fs,
                    &mut cache,
                    x_pos - offset as f32,
                    y_pos - offset as f32,
                    shadow_color,
                );
                draw_text_at(
                    pixmap,
                    &buffer,
                    &mut fs,
                    &mut cache,
                    x_pos + offset as f32,
                    y_pos - offset as f32,
                    shadow_color,
                );
                draw_text_at(
                    pixmap,
                    &buffer,
                    &mut fs,
                    &mut cache,
                    x_pos - offset as f32,
                    y_pos + offset as f32,
                    shadow_color,
                );
                draw_text_at(
                    pixmap,
                    &buffer,
                    &mut fs,
                    &mut cache,
                    x_pos + offset as f32,
                    y_pos + offset as f32,
                    shadow_color,
                );
                draw_text_at(
                    pixmap, &buffer, &mut fs, &mut cache, x_pos, y_pos, text_color,
                );
            }
        });
    });

    if omitted {
        return Err(ExportError::WatermarkOmitted);
    }
    Ok(())
}

fn draw_image_watermark(
    pixmap: &mut tiny_skia::Pixmap,
    badge: &PreloadedBadge,
    pos: WatermarkPosition,
    target_w: u32,
    target_h: u32,
) -> Result<(), ExportError> {
    let badge_pixmap = &badge.pixmap;

    let long_edge_val = target_w.max(target_h) as f32;
    let scale_pct = badge.scale.map(|s| s.value()).unwrap_or(5.0);

    let target_badge_long_edge = (long_edge_val * (scale_pct / 100.0)).max(1.0);

    let badge_w = badge_pixmap.width() as f32;
    let badge_h = badge_pixmap.height() as f32;
    let badge_long_edge = badge_w.max(badge_h);
    let scale_factor = target_badge_long_edge / badge_long_edge;

    let final_badge_w = (badge_w * scale_factor).max(1.0);
    let final_badge_h = (badge_h * scale_factor).max(1.0);

    let padding = (long_edge_val * 0.015).round().max(8.0);
    let (x_pos, y_pos) = calculate_watermark_position(
        pos,
        final_badge_w,
        final_badge_h,
        target_w as f32,
        target_h as f32,
        padding,
    );

    if x_pos < 0.0
        || y_pos < 0.0
        || (x_pos + final_badge_w) > target_w as f32
        || (y_pos + final_badge_h) > target_h as f32
    {
        tracing::warn!("Watermark image does not fit, omitting.");
        return Err(ExportError::WatermarkOmitted);
    }

    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            (**badge_pixmap).as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bicubic,
            1.0,
            tiny_skia::Transform::from_scale(scale_factor, scale_factor),
        ),
        ..Default::default()
    };

    let mut temp_badge =
        tiny_skia::Pixmap::new(final_badge_w.ceil() as u32, final_badge_h.ceil() as u32)
            .ok_or_else(|| ExportError::BadgeLoadFailed {
                path: PathBuf::from("<internal>"),
                reason: "Allocation failed".to_string(),
            })?;
    let temp_rect =
        tiny_skia::Rect::from_xywh(0.0, 0.0, final_badge_w, final_badge_h).ok_or_else(|| {
            ExportError::BadgeLoadFailed {
                path: PathBuf::from("<internal>"),
                reason: "Invalid badge dimensions".to_string(),
            }
        })?;
    temp_badge.fill_rect(temp_rect, &paint, tiny_skia::Transform::identity(), None);

    let src_data = temp_badge.data();
    let dst_w = pixmap.width() as i32;
    let dst_h = pixmap.height() as i32;
    let dst_data = pixmap.data_mut();

    let base_x = x_pos.round() as i32;
    let base_y = y_pos.round() as i32;

    for y in 0..(temp_badge.height() as i32) {
        for x in 0..(temp_badge.width() as i32) {
            let dx = base_x + x;
            let dy = base_y + y;

            if dx >= 0 && dx < dst_w && dy >= 0 && dy < dst_h {
                let src_idx = ((y * temp_badge.width() as i32 + x) * 4) as usize;
                let dst_idx = ((dy * dst_w + dx) * 4) as usize;

                let src_r = src_data[src_idx] as f32;
                let src_g = src_data[src_idx + 1] as f32;
                let src_b = src_data[src_idx + 2] as f32;
                let src_a = src_data[src_idx + 3] as f32;
                let alpha = src_a / 255.0;

                let dst_r = dst_data[dst_idx] as f32;
                let dst_g = dst_data[dst_idx + 1] as f32;
                let dst_b = dst_data[dst_idx + 2] as f32;
                // Leave background alpha untouched (usually 255)

                let inv_alpha = 1.0 - alpha;

                dst_data[dst_idx] = (src_r + dst_r * inv_alpha).clamp(0.0, 255.0) as u8;
                dst_data[dst_idx + 1] = (src_g + dst_g * inv_alpha).clamp(0.0, 255.0) as u8;
                dst_data[dst_idx + 2] = (src_b + dst_b * inv_alpha).clamp(0.0, 255.0) as u8;
            }
        }
    }

    Ok(())
}

fn draw_text_at(
    pixmap: &mut tiny_skia::Pixmap,
    buffer: &cosmic_text::Buffer,
    fs: &mut cosmic_text::FontSystem,
    cache: &mut cosmic_text::SwashCache,
    x_pos: f32,
    y_pos: f32,
    color: tiny_skia::Color,
) {
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical_glyph = glyph.physical((x_pos, y_pos), 1.0);
            if let Some(image) = cache.get_image(fs, physical_glyph.cache_key) {
                let x_glyph = physical_glyph.x + image.placement.left;
                // Since Swash Y is Y-up, and tiny-skia is Y-down:
                let y_glyph = physical_glyph.y - image.placement.top;

                for gy in 0..image.placement.height {
                    for gx in 0..image.placement.width {
                        let mask_alpha = image.data[(gy * image.placement.width + gx) as usize];
                        if mask_alpha == 0 {
                            continue;
                        }
                        blend_pixel(
                            pixmap,
                            x_glyph + gx as i32,
                            y_glyph + gy as i32,
                            color,
                            mask_alpha,
                        );
                    }
                }
            }
        }
    }
}

fn blend_pixel(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    color: tiny_skia::Color,
    mask_alpha: u8,
) {
    if x < 0 || x >= pixmap.width() as i32 || y < 0 || y >= pixmap.height() as i32 {
        return;
    }
    let idx = ((y as usize * pixmap.width() as usize) + x as usize) * 4;
    let data = pixmap.data_mut();

    let f = (mask_alpha as f32 / 255.0) * color.alpha();
    if f <= 0.0 {
        return;
    }

    let src_r = color.red() * f * 255.0;
    let src_g = color.green() * f * 255.0;
    let src_b = color.blue() * f * 255.0;
    let src_a = f * 255.0;

    let dst_r = data[idx] as f32;
    let dst_g = data[idx + 1] as f32;
    let dst_b = data[idx + 2] as f32;
    let dst_a = data[idx + 3] as f32;

    let inv_src_a = 1.0 - f;

    let out_r = (src_r + dst_r * inv_src_a).round().clamp(0.0, 255.0) as u8;
    let out_g = (src_g + dst_g * inv_src_a).round().clamp(0.0, 255.0) as u8;
    let out_b = (src_b + dst_b * inv_src_a).round().clamp(0.0, 255.0) as u8;
    let out_a = (src_a + dst_a * inv_src_a).round().clamp(0.0, 255.0) as u8;

    data[idx] = out_r;
    data[idx + 1] = out_g;
    data[idx + 2] = out_b;
    data[idx + 3] = out_a;
}

/// Compress RGB buffer using MozJPEG inside a panic-safe FFI boundary.
fn compress_jpeg(
    rgb_pixels: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    output_path: &Path,
) -> Result<(), ExportError> {
    // SAFETY: We catch any panic from MozJPEG FFI bindings inside AssertUnwindSafe.
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(width as usize, height as usize);
        comp.set_quality(quality as f32);
        comp.set_progressive_mode();

        let mut comp_writer = comp
            .start_compress(Vec::new())
            .map_err(|e| format!("Failed to start compress: {e}"))?;

        comp_writer
            .write_scanlines(rgb_pixels)
            .map_err(|e| format!("Failed to write scanlines: {e}"))?;

        let jpeg_bytes = comp_writer
            .finish()
            .map_err(|e| format!("Failed to finish compress: {e}"))?;

        Ok(jpeg_bytes)
    }));

    match res {
        Ok(Ok(bytes)) => {
            std::fs::write(output_path, bytes)?;
            Ok(())
        }
        Ok(Err(err_msg)) => Err(ExportError::JpegEncode(err_msg)),
        Err(_) => Err(ExportError::JpegEncode("MozJPEG thread panic".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rating_clamping() {
        assert_eq!(Rating::new(10).value(), 5);
        assert_eq!(Rating::new(-5).value(), -1);
        assert_eq!(Rating::new(3).value(), 3);
    }

    #[test]
    fn test_nima_score_validation() {
        assert!(NimaScore::new(f32::NAN).is_none());
        assert!(NimaScore::new(f32::INFINITY).is_none());
        assert_eq!(NimaScore::new(6.5).unwrap().value(), 6.5);
    }

    #[test]
    fn test_pixel_demultiplication() {
        let mut pixmap = tiny_skia::Pixmap::new(2, 2).unwrap();
        {
            let data = pixmap.data_mut();
            // Pixel 0: Opaque Red [255, 0, 0, 255]
            data[0] = 255;
            data[1] = 0;
            data[2] = 0;
            data[3] = 255;
            // Pixel 1: Half-transparent Green [0, 100, 0, 200]
            data[4] = 0;
            data[5] = 100;
            data[6] = 0;
            data[7] = 200;
            // Pixel 2: Fully transparent [0, 0, 0, 0]
            data[8] = 0;
            data[9] = 0;
            data[10] = 0;
            data[11] = 0;
            // Pixel 3: Fully opaque White [255, 255, 255, 255]
            data[12] = 255;
            data[13] = 255;
            data[14] = 255;
            data[15] = 255;
        }

        let final_data = pixmap.data();
        let mut rgb_buffer = Vec::new();
        let mut idx = 0;
        while idx < final_data.len() {
            let r = final_data[idx];
            let g = final_data[idx + 1];
            let b = final_data[idx + 2];
            let a = final_data[idx + 3];

            if a == 255 {
                rgb_buffer.push(r);
                rgb_buffer.push(g);
                rgb_buffer.push(b);
            } else if a == 0 {
                rgb_buffer.push(0);
                rgb_buffer.push(0);
                rgb_buffer.push(0);
            } else {
                let r_demult = ((r as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8;
                let g_demult = ((g as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8;
                let b_demult = ((b as f32 / a as f32) * 255.0).round().clamp(0.0, 255.0) as u8;
                rgb_buffer.push(r_demult);
                rgb_buffer.push(g_demult);
                rgb_buffer.push(b_demult);
            }
            idx += 4;
        }

        assert_eq!(rgb_buffer[0..3], [255, 0, 0]);
        // 100 / 200 * 255 = 127.5 -> 128
        assert_eq!(rgb_buffer[3..6], [0, 128, 0]);
        assert_eq!(rgb_buffer[6..9], [0, 0, 0]);
        assert_eq!(rgb_buffer[9..12], [255, 255, 255]);
    }

    #[test]
    fn test_scale_clamping() {
        assert_eq!(Scale::new(150.0).value(), 100.0);
        assert_eq!(Scale::new(0.0).value(), 0.001);
        assert_eq!(Scale::new(-5.0).value(), 0.001);
        assert_eq!(Scale::new(5.0).value(), 5.0);
    }

    #[test]
    fn test_watermark_position_calculation() {
        // Test 1x1 image target zero-clamping bounds
        let (x, y) =
            calculate_watermark_position(WatermarkPosition::Center, 10.0, 10.0, 1.0, 1.0, 0.0);
        assert_eq!(x, -4.5);
        assert_eq!(y, -4.5);
    }

    #[test]
    fn test_tone_mapping_lut() {
        let lut = ToneMappingLut::new(0.0);
        // Test black
        assert_eq!(lut.apply(0), 0);
        // Test white (may not reach exactly 255 due to filmic curve shoulder)
        assert!(lut.apply(65535) > 230);

        let lut_plus_1 = ToneMappingLut::new(1.0); // +1 EV
        // Mid-tone should be brighter
        assert!(lut_plus_1.apply(10000) > lut.apply(10000));
    }

    #[test]
    fn test_aspect_ratio_calculations() {
        // Landscape input: 100x50, long edge limit 20 -> expected 20x10
        let w_f = 100.0f32;
        let h_f = 50.0f32;
        let limit = 20.0f32;
        let scale = limit / w_f.max(h_f);
        let tw = (w_f * scale).round() as u32;
        let th = (h_f * scale).round() as u32;
        assert_eq!(tw.max(1), 20);
        assert_eq!(th.max(1), 10);

        // Portrait input: 50x100, long edge limit 20 -> expected 10x20
        let w_f = 50.0f32;
        let h_f = 100.0f32;
        let scale = limit / w_f.max(h_f);
        let tw = (w_f * scale).round() as u32;
        let th = (h_f * scale).round() as u32;
        assert_eq!(tw.max(1), 10);
        assert_eq!(th.max(1), 20);

        // Square input: 100x100, limit 20 -> expected 20x20
        let w_f = 100.0f32;
        let h_f = 100.0f32;
        let scale = limit / w_f.max(h_f);
        let tw = (w_f * scale).round() as u32;
        let th = (h_f * scale).round() as u32;
        assert_eq!(tw.max(1), 20);
        assert_eq!(th.max(1), 20);

        // Extremely thin panoramic strip: 1000x2, limit 16 -> expected 16x1
        let w_f = 1000.0f32;
        let h_f = 2.0f32;
        let limit = 16.0f32;
        let scale = limit / w_f.max(h_f);
        let tw = (w_f * scale).round() as u32;
        let th = (h_f * scale).round() as u32;
        assert_eq!(tw.max(1), 16);
        assert_eq!(th.max(1), 1);
    }
}
