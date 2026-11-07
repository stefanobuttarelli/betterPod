//! Local integration fixture, not a console benchmark.
use betterpod_rust::render::{Renderer, WHITE};
use std::path::Path;
fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let attr = video.gl_attr();
    attr.set_context_profile(sdl2::video::GLProfile::GLES);
    attr.set_context_version(2, 0);
    let window = video
        .window("Rust renderer check", 640, 480)
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let _context = window.gl_create_context()?;
    let assets = std::env::var("BETTERPOD_RUST_ASSETS_DIR")
        .unwrap_or_else(|_| "port/betterpod/assets".into());
    let mut r = Renderer::new(&video, &Path::new(&assets).join("ui_font.ttf"), 640, 480)?;
    eprintln!("{}", r.backend);
    let img = image::open(Path::new(&assets).join("ipod/default_album_artwork.png"))
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let texture = r.upload(img.width(), img.height(), img.as_raw())?;
    r.begin(WHITE);
    r.gradient(0.0, 0.0, 640.0, 40.0, WHITE, [0.7, 0.72, 0.75, 1.0]);
    r.text("betterPod Rust", 12.0, 4.0, 26, [0.0, 0.0, 0.0, 1.0], 600.0);
    r.cover(texture, 200.0, 222.0, 245.0, 0.7, 1.0, true);
    r.cover(texture, 442.0, 222.0, 245.0, -0.7, 1.0, true);
    r.centered(
        "日本語 · Ελληνικά · Кириллица",
        0.0,
        380.0,
        640.0,
        24,
        [0.1, 0.2, 0.3, 1.0],
    );
    r.centered(
        "Native Rust · GPU composition",
        0.0,
        416.0,
        640.0,
        24,
        [0.1, 0.2, 0.3, 1.0],
    );
    r.finish();
    let out = std::env::var("BETTERPOD_RUST_SCREENSHOT").unwrap_or_else(|_| "renderer.png".into());
    r.screenshot(Path::new(&out))?;
    r.delete_texture(texture);
    Ok(())
}
