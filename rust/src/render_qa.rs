//! Opt-in real GLES checks. No console, screenshots or synthetic frame-rate claims.
use super::*;
#[test]
#[ignore = "requires SDL/OpenGL and original fonts; run with Xvfb"]
fn atlas_rollover_preserves_earlier_text_in_the_same_frame() {
    let assets = std::path::PathBuf::from(
        std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("original assets"),
    );
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Atlas test", 640, 480)
        .opengl()
        .hidden()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let pixels = |r: &Renderer| {
        let mut out = vec![0; 640 * 480 * 4];
        unsafe {
            r.gl.read_pixels(
                0,
                0,
                640,
                480,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut out)),
            );
        }
        out
    };
    r.begin(WHITE);
    r.text_weighted("AAAA AAAA", 24.0, 32.0, 28.0, 400, BLACK, 600.0);
    r.text_weighted("Z漢字", 24.0, 100.0, 28.0, 400, BLACK, 600.0);
    r.finish();
    let expected = pixels(&r);
    r.begin(WHITE);
    r.text_weighted("AAAA AAAA", 24.0, 32.0, 28.0, 400, BLACK, 600.0);
    r.atlas.glyphs.remove(&('Z', 56, 400));
    r.atlas.x = 2047;
    r.atlas.y = 2047;
    r.text_weighted("Z漢字", 24.0, 100.0, 28.0, 400, BLACK, 600.0);
    r.finish();
    assert!(
        !r.atlas.glyphs.contains_key(&('A', 56, 400)),
        "the full atlas must actually have rolled over"
    );
    assert_eq!(
        pixels(&r),
        expected,
        "rollover must neither erase prior text nor leave new glyphs blank"
    );
}

#[test]
#[ignore = "requires SDL/OpenGL and original fonts; run with Xvfb"]
fn evicting_a_texture_preserves_queued_images_until_frame_end() {
    let assets = std::path::PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").unwrap());
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Texture lifetime", 640, 480)
        .opengl()
        .hidden()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let texture = r.upload(1, 1, &[255, 0, 0, 255]).unwrap();
    r.begin(WHITE);
    r.image_uv(texture, [0., 0., 640., 480.], [0., 0., 1., 1.], 1.);
    // Another weather icon upload can evict an earlier icon in this frame.
    r.delete_texture(texture);
    r.finish();
    let pixels = r.capture_rgba().into_raw();
    assert!(
        pixels
            .chunks_exact(4)
            .all(|p| p[0] > 250 && p[1] < 5 && p[2] < 5),
        "queued image was deleted before the GPU used it"
    );
    r.begin(WHITE);
    r.finish();
}

#[test]
#[ignore = "requires SDL/OpenGL and original fonts; run with Xvfb"]
fn artwork_minification_averages_fine_details_instead_of_aliasing() {
    let assets = std::path::PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").unwrap());
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Artwork filtering", 640, 480)
        .opengl()
        .hidden()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    for (width, height) in [(512, 512), (511, 341)] {
        if !r.art_npot && (width, height) != (512, 512) {
            continue;
        }
        let mut pixels = vec![0; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let shade = if (x + y) % 2 == 0 { 0 } else { 255 };
                pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
                    .copy_from_slice(&[shade, shade, shade, 255]);
            }
        }
        let error = |image: &image::RgbaImage| -> i64 {
            (21..36)
                .flat_map(|y| (21..36).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let d = image.get_pixel(x, y)[0] as i64 - 128;
                    d * d
                })
                .sum()
        };
        let ordinary = r.upload(width as u32, height as u32, &pixels).unwrap();
        r.begin(WHITE);
        r.image_uv(ordinary, [20., 20., 17., 17.], [0., 0., 1., 1.], 1.);
        r.finish();
        let before = error(&r.capture_rgba());
        let image =
            image::RgbaImage::from_raw(width as u32, height as u32, pixels.clone()).unwrap();
        let mipmaps = crate::art::mipmap_levels(&image);
        let texture = r
            .upload_artwork(width as u32, height as u32, &pixels, &mipmaps)
            .unwrap();
        r.begin(WHITE);
        r.image_uv(texture, [20., 20., 17., 17.], [0., 0., 1., 1.], 1.);
        r.finish();
        let after = error(&r.capture_rgba());
        assert!(before > 0 && after * 4 < before, "{width}x{height}: filtering must reduce checker alias energy by at least 75% ({before} -> {after})");
        r.delete_texture(ordinary);
        r.delete_texture(texture);
    }
}
