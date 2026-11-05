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
