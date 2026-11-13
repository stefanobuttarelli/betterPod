//! Artwork keeps its 512px display resolution without full-size JPEG pixels
//! or a source-width by 512 floating-point resize intermediate.
use image::{DynamicImage, ImageBuffer, Pixel, Primitive, RgbaImage};
use jpeg_decoder::{CodingProcess, Decoder, PixelFormat};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Mutex;

const EDGE: u32 = 512;
const MAX_EDGE: u32 = 8192;
const MAX_ENCODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
const LARGE_PIXELS: u64 = 2 * 1024 * 1024;
// Small covers retain two-worker concurrency. Large originals never overlap
// their pixel/coefficient buffers, including while they are being resized.
static LARGE_DECODE: Mutex<()> = Mutex::new(());

fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_EDGE || height > MAX_EDGE {
        return Err("Artwork dimensions exceed 8192px".into());
    }
    Ok(())
}

pub(super) fn decode(path: &Path) -> Result<RgbaImage, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_ENCODED_BYTES {
        return Err("Artwork file exceeds 64 MiB".into());
    }
    let reader = image::ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if reader.format() == Some(image::ImageFormat::Jpeg) {
        let mut jpeg = Decoder::new(reader.into_inner());
        jpeg.read_info().map_err(|e| e.to_string())?;
        let info = jpeg.info().ok_or("Missing JPEG dimensions")?;
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        check_dimensions(width, height)?;
        // Lossless/high-bit-depth JPEGs retain the existing decoder path.
        if info.coding_process != CodingProcess::Lossless && info.pixel_format != PixelFormat::L16 {
            let _large = (u64::from(width) * u64::from(height) > LARGE_PIXELS)
                .then(|| LARGE_DECODE.lock().unwrap_or_else(|e| e.into_inner()));
            // Progressive scans retain full DCT coefficients. Sequential JPEG
            // helpers can also queue rows faster than IDCT consumes them. Budget
            // all original coefficients in either case, with worst-case MCU
            // padding and all planes. A normal 20MP RGB image still fits even
            // without chroma subsampling. Helper threads inherit the artwork
            // thread's lower Linux priority (three RGB planes, four for CMYK).
            let padded_w = u64::from(width).div_ceil(32) * 32;
            let padded_h = u64::from(height).div_ceil(32) * 32;
            let coefficients = padded_w * padded_h * info.pixel_format.pixel_bytes() as u64 * 2;
            if coefficients > MAX_DECODED_BYTES {
                return Err("Artwork JPEG exceeds the decode memory budget".into());
            }
            jpeg.scale(EDGE as u16, EDGE as u16)
                .map_err(|e| e.to_string())?;
            jpeg.set_max_decoding_buffer_size(MAX_DECODED_BYTES as usize);
            let pixels = jpeg.decode().map_err(|e| e.to_string())?;
            let info = jpeg.info().ok_or("Missing JPEG dimensions")?;
            let (w, h) = (u32::from(info.width), u32::from(info.height));
            let image = match info.pixel_format {
                PixelFormat::L8 => DynamicImage::ImageLuma8(
                    ImageBuffer::from_raw(w, h, pixels).ok_or("Invalid grayscale JPEG")?,
                ),
                PixelFormat::RGB24 => DynamicImage::ImageRgb8(
                    ImageBuffer::from_raw(w, h, pixels).ok_or("Invalid RGB JPEG")?,
                ),
                PixelFormat::CMYK32 => {
                    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
                    for p in pixels.as_chunks::<4>().0 {
                        for c in &p[..3] {
                            rgb.push(((255 - u16::from(*c)) * (255 - u16::from(p[3])) / 255) as u8);
                        }
                    }
                    DynamicImage::ImageRgb8(
                        ImageBuffer::from_raw(w, h, rgb).ok_or("Invalid CMYK JPEG")?,
                    )
                }
                PixelFormat::L16 => unreachable!(),
            };
            // Release JPEG coefficient planes before the final resample.
            drop(jpeg);
            return Ok(resize_for_dimensions(&image, width, height));
        }
    }
    decode_other(path)
}

fn decode_other(path: &Path) -> Result<RgbaImage, String> {
    let reader = other_reader(path)?;
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    check_dimensions(width, height)?;
    let _large = (u64::from(width) * u64::from(height) > LARGE_PIXELS)
        .then(|| LARGE_DECODE.lock().unwrap_or_else(|e| e.into_inner()));
    let reader = other_reader(path)?;
    let image = reader.decode().map_err(|e| e.to_string())?;
    Ok(resize(&image))
}

fn other_reader(path: &Path) -> Result<image::ImageReader<BufReader<File>>, String> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    reader.limits(limits);
    Ok(reader)
}

trait Channel: Primitive {
    fn float(self) -> f32;
    fn sampled(value: f32) -> Self;
}
impl Channel for u8 {
    fn float(self) -> f32 {
        self as f32
    }
    fn sampled(value: f32) -> Self {
        value.clamp(0., 255.).round() as u8
    }
}
impl Channel for u16 {
    fn float(self) -> f32 {
        self as f32
    }
    fn sampled(value: f32) -> Self {
        value.clamp(0., 65535.).round() as u16
    }
}
impl Channel for f32 {
    fn float(self) -> f32 {
        self
    }
    fn sampled(value: f32) -> Self {
        value.clamp(0., 1.)
    }
}

struct Weights {
    start: u32,
    values: Vec<f32>,
}
fn weights(source: u32, destination: u32, index: u32) -> Weights {
    let ratio = source as f32 / destination as f32;
    let support = ratio.max(1.);
    let center = (index as f32 + 0.5) * ratio;
    let start = ((center - support).floor() as i64).clamp(0, i64::from(source) - 1) as u32;
    let end =
        ((center + support).ceil() as i64).clamp(i64::from(start) + 1, i64::from(source)) as u32;
    let center = center - 0.5;
    let mut values = Vec::with_capacity((end - start) as usize);
    let mut sum = 0.;
    for i in start..end {
        let distance = ((i as f32 - center) / support).abs();
        let value = if distance < 1. { 1. - distance } else { 0. };
        values.push(value);
        sum += value;
    }
    for value in &mut values {
        *value /= sum;
    }
    Weights { start, values }
}

// Same separable Triangle weights, vertical-then-horizontal accumulation,
// native channel rounding and pixel order as image::imageops::resize. Retain
// one float row rather than the entire width × destination-height image.
fn resize_typed<P, S>(
    image: &ImageBuffer<P, Vec<S>>,
    width: u32,
    height: u32,
) -> ImageBuffer<P, Vec<S>>
where
    P: Pixel<Subpixel = S> + 'static,
    S: Channel + 'static,
{
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    let mut output = ImageBuffer::new(width, height);
    let horizontal: Vec<_> = (0..width)
        .map(|x| weights(image.width(), width, x))
        .collect();
    let mut row = vec![[0f32; 4]; image.width() as usize];
    for y in 0..height {
        let vertical = weights(image.height(), height, y);
        for (x, result) in row.iter_mut().enumerate() {
            *result = [0.; 4];
            for (i, weight) in vertical.values.iter().enumerate() {
                #[allow(deprecated)]
                let channels = image
                    .get_pixel(x as u32, vertical.start + i as u32)
                    .channels4();
                let channels = [channels.0, channels.1, channels.2, channels.3];
                for c in 0..4 {
                    result[c] += channels[c].float() * weight;
                }
            }
        }
        for (x, horizontal) in horizontal.iter().enumerate() {
            let mut pixel = [0f32; 4];
            for (i, weight) in horizontal.values.iter().enumerate() {
                let sample = row[horizontal.start as usize + i];
                for c in 0..4 {
                    pixel[c] += sample[c] * weight;
                }
            }
            #[allow(deprecated)]
            output.put_pixel(
                x as u32,
                y,
                P::from_channels(
                    S::sampled(pixel[0]),
                    S::sampled(pixel[1]),
                    S::sampled(pixel[2]),
                    S::sampled(pixel[3]),
                ),
            );
        }
    }
    output
}

fn resize(image: &DynamicImage) -> RgbaImage {
    resize_for_dimensions(image, image.width(), image.height())
}

fn resize_for_dimensions(image: &DynamicImage, width: u32, height: u32) -> RgbaImage {
    // Scaled JPEG IDCT rounds each axis up. Retain the original aspect ratio
    // when choosing final bounds so odd source dimensions cannot shift a row.
    let ratio = (f64::from(EDGE) / f64::from(width)).min(f64::from(EDGE) / f64::from(height));
    let (w, h) = (
        (f64::from(width) * ratio).round().max(1.) as u32,
        (f64::from(height) * ratio).round().max(1.) as u32,
    );
    macro_rules! resize {
        ($p:expr, $variant:ident) => {
            DynamicImage::$variant(resize_typed($p, w, h)).into_rgba8()
        };
    }
    match image {
        DynamicImage::ImageLuma8(p) => resize!(p, ImageLuma8),
        DynamicImage::ImageLumaA8(p) => resize!(p, ImageLumaA8),
        DynamicImage::ImageRgb8(p) => resize!(p, ImageRgb8),
        DynamicImage::ImageRgba8(p) => resize!(p, ImageRgba8),
        DynamicImage::ImageLuma16(p) => resize!(p, ImageLuma16),
        DynamicImage::ImageLumaA16(p) => resize!(p, ImageLumaA16),
        DynamicImage::ImageRgb16(p) => resize!(p, ImageRgb16),
        DynamicImage::ImageRgba16(p) => resize!(p, ImageRgba16),
        DynamicImage::ImageRgb32F(p) => resize!(p, ImageRgb32F),
        DynamicImage::ImageRgba32F(p) => resize!(p, ImageRgba32F),
        _ => unreachable!("Unsupported decoded artwork pixel type"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "betterpod-art-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn check_jpeg_quality(path: &Path) {
        let expected = image::open(path)
            .unwrap()
            .resize(EDGE, EDGE, image::imageops::FilterType::Triangle)
            .into_rgba8();
        let actual = decode(path).unwrap();
        assert_eq!(actual.dimensions(), expected.dimensions());
        let total: u64 = actual
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(
            (total as f64 / actual.as_raw().len() as f64) < 2.5,
            "Scaled JPEG changed average pixel values excessively"
        );
        assert!(actual.pixels().all(|p| p[3] == 255));
    }

    #[test]
    fn scaled_baseline_and_progressive_jpeg_keep_bounds_and_cover_quality() {
        let temp = Scratch::new();
        // This small generated fixture contains real progressive scans.
        let progressive = temp.path("progressive.jpg");
        std::fs::write(
            &progressive,
            include_bytes!("../../tests/fixtures/art-progressive.jpg"),
        )
        .unwrap();
        check_jpeg_quality(&progressive);
        let rgb = image::RgbImage::from_fn(1280, 961, |x, y| {
            image::Rgb([
                ((x / 8 + y / 8) % 256) as u8,
                (x / 7 % 256) as u8,
                (y / 6 % 256) as u8,
            ])
        });
        let baseline = temp.path("baseline.jpg");
        image::codecs::jpeg::JpegEncoder::new_with_quality(File::create(&baseline).unwrap(), 93)
            .encode_image(&rgb)
            .unwrap();
        check_jpeg_quality(&baseline);
        // Native IDCT ceil-rounding must not alter the source aspect ratio.
        let native = DynamicImage::new_rgb8(625, 417);
        assert_eq!(
            resize_for_dimensions(&native, 5000, 3333).dimensions(),
            (512, 341)
        );
    }

    #[test]
    fn png_and_webp_keep_alpha_and_match_existing_pixels() {
        let temp = Scratch::new();
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(1280, 961, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 11) as u8, (x ^ y) as u8, (x + y) as u8])
        }));
        let expected = resize(&image);
        for name in ["cover.png", "cover.webp"] {
            let path = temp.path(name);
            image.save(&path).unwrap();
            assert_eq!(decode(&path).unwrap(), expected, "{name}");
        }
    }

    #[test]
    fn excessive_progressive_coefficients_and_compressed_files_fail_before_decode() {
        let temp = Scratch::new();
        let mut jpeg = include_bytes!("../../tests/fixtures/art-progressive.jpg").to_vec();
        let sof = jpeg.windows(2).position(|p| p == [0xff, 0xc2]).unwrap();
        jpeg[sof + 5..sof + 7].copy_from_slice(&8192u16.to_be_bytes());
        jpeg[sof + 7..sof + 9].copy_from_slice(&8192u16.to_be_bytes());
        let path = temp.path("excessive.jpg");
        std::fs::write(&path, jpeg).unwrap();
        assert!(decode(&path).unwrap_err().contains("decode memory budget"));
        let oversized = temp.path("oversized.jpg");
        File::create(&oversized)
            .unwrap()
            .set_len(MAX_ENCODED_BYTES + 1)
            .unwrap();
        assert!(decode(&oversized).unwrap_err().contains("64 MiB"));
    }
    #[test]
    fn bounded_triangle_matches_existing_resize_for_all_decoded_pixel_types() {
        for (w, h) in [
            (1, 1),
            (1, 41),
            (43, 1),
            (19, 27),
            (511, 513),
            (1280, 720),
            (1301, 973),
        ] {
            let rgb = image::RgbImage::from_fn(w, h, |x, y| {
                image::Rgb([
                    (x * 37 + y * 59) as u8,
                    (x * 9 + y * 11) as u8,
                    (x ^ y) as u8,
                ])
            });
            let rgba = image::RgbaImage::from_fn(w, h, |x, y| {
                image::Rgba([
                    (x * 37 + y * 59) as u8,
                    (x * 9 + y * 11) as u8,
                    (x ^ y) as u8,
                    (x * 7 + y * 3) as u8,
                ])
            });
            let base = DynamicImage::ImageRgba8(rgba);
            let cases = [
                DynamicImage::ImageRgb8(rgb),
                base.clone(),
                DynamicImage::ImageLuma8(base.to_luma8()),
                DynamicImage::ImageLumaA8(base.to_luma_alpha8()),
                DynamicImage::ImageLuma16(base.to_luma16()),
                DynamicImage::ImageLumaA16(base.to_luma_alpha16()),
                DynamicImage::ImageRgb16(base.to_rgb16()),
                DynamicImage::ImageRgba16(base.to_rgba16()),
                DynamicImage::ImageRgb32F(base.to_rgb32f()),
                DynamicImage::ImageRgba32F(base.to_rgba32f()),
            ];
            // Pixel conversion does not depend on size, nor sampling on the
            // pixel type: the two large frames check one type per sample
            // family (8-bit RGB and RGBA, 16-bit, float) instead of all ten.
            let large = w * h > 512 * 513;
            for image in cases.into_iter().filter(|image| {
                !large
                    || matches!(
                        image,
                        DynamicImage::ImageRgb8(_)
                            | DynamicImage::ImageRgba8(_)
                            | DynamicImage::ImageRgba16(_)
                            | DynamicImage::ImageRgba32F(_)
                    )
            }) {
                let expected = image
                    .resize(EDGE, EDGE, image::imageops::FilterType::Triangle)
                    .to_rgba8();
                let actual = resize(&image);
                assert_eq!(actual, expected, "{}×{} {:?}", w, h, image.color());
            }
        }
    }
}
