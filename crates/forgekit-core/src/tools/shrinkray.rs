//! Shrinkray: bulk-resize and convert images.

use std::path::{Path, PathBuf};

use anyhow::Context;
use image::{ImageFormat, imageops::FilterType};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Fit inside this box, keeping the aspect ratio. `None` on both = convert only.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Allow making images bigger.
    pub upscale: bool,
    /// Output format by extension (`png`, `jpg`, `webp`, …); default keeps the input's.
    pub format: Option<String>,
    /// Output directory; default is next to the input.
    pub out_dir: Option<PathBuf>,
    /// Replace the input instead of writing `name (WxH).ext`.
    pub in_place: bool,
}

pub fn process(input: &Path, opts: &Options) -> anyhow::Result<PathBuf> {
    let img = image::open(input).with_context(|| format!("opening {}", input.display()))?;
    let (w, h) = (img.width(), img.height());
    let (tw, th) = target_size(w, h, opts);
    let img = if (tw, th) != (w, h) { img.resize(tw, th, FilterType::Lanczos3) } else { img };

    let ext = match &opts.format {
        Some(f) => f.trim_start_matches('.').to_ascii_lowercase(),
        None => input.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase(),
    };
    let format = ImageFormat::from_extension(&ext).with_context(|| format!("unknown image format '{ext}'"))?;
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let name = if opts.in_place {
        format!("{stem}.{ext}")
    } else {
        format!("{stem} ({}x{}).{ext}", img.width(), img.height())
    };
    let dir = opts.out_dir.clone().unwrap_or_else(|| input.parent().unwrap_or(Path::new(".")).to_path_buf());
    std::fs::create_dir_all(&dir)?;
    let out = dir.join(name);

    // JPEG has no alpha channel.
    let img = if format == ImageFormat::Jpeg { image::DynamicImage::ImageRgb8(img.to_rgb8()) } else { img };
    img.save_with_format(&out, format).with_context(|| format!("writing {}", out.display()))?;
    if opts.in_place && out != input {
        std::fs::remove_file(input)?;
    }
    Ok(out)
}

fn target_size(w: u32, h: u32, opts: &Options) -> (u32, u32) {
    let scale = match (opts.width, opts.height) {
        (None, None) => return (w, h),
        (Some(tw), None) => tw as f64 / w as f64,
        (None, Some(th)) => th as f64 / h as f64,
        (Some(tw), Some(th)) => (tw as f64 / w as f64).min(th as f64 / h as f64),
    };
    if scale >= 1.0 && !opts.upscale {
        return (w, h);
    }
    (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing() {
        let o = |w, h, up| Options { width: w, height: h, upscale: up, ..Default::default() };
        assert_eq!(target_size(4000, 3000, &o(Some(800), None, false)), (800, 600));
        assert_eq!(target_size(4000, 3000, &o(Some(800), Some(800), false)), (800, 600));
        assert_eq!(target_size(400, 300, &o(Some(800), None, false)), (400, 300));
        assert_eq!(target_size(400, 300, &o(Some(800), None, true)), (800, 600));
    }

    #[test]
    fn resize_and_convert() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("pic.png");
        image::RgbaImage::new(200, 100).save(&src).unwrap();
        let out =
            process(&src, &Options { width: Some(50), format: Some("jpg".into()), ..Default::default() }).unwrap();
        assert_eq!(out.file_name().unwrap(), "pic (50x25).jpg");
        assert_eq!(image::image_dimensions(&out).unwrap(), (50, 25));
        assert!(src.exists());
    }
}
