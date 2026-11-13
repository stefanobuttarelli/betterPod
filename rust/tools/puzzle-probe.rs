//! Draw every actual upstream engine through the new Rust frontend.
use betterpod_rust::{
    input::Action,
    puzzles::{PuzzleGame, PUZZLES},
    render::{Renderer, BLACK},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let attr = video.gl_attr();
    attr.set_context_profile(sdl2::video::GLProfile::GLES);
    attr.set_context_version(2, 0);
    let window = video
        .window("Rust puzzle visual probe", 640, 480)
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let _context = window.gl_create_context()?;
    let mut r = Renderer::new(
        &video,
        Path::new("/repo/port/betterpod/assets/ui_font.ttf"),
        640,
        480,
    )?;
    for (index, info) in PUZZLES.iter().enumerate() {
        let mut game = PuzzleGame::new(index)?;
        let began = Instant::now();
        while game.is_loading() {
            game.tick(1.0 / 60.0);
            if began.elapsed() > Duration::from_secs(15) {
                return Err(format!("{} load timed out", info.name));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(error) = game.error() {
            return Err(format!("{}: {error}", info.name));
        }
        for phase in ["initial", "action"] {
            if phase == "action" {
                game.handle(Action::Down);
                game.handle(Action::Right);
                game.handle(Action::Select);
                for _ in 0..10 {
                    game.tick(1.0 / 60.0);
                }
            }
            r.begin(BLACK);
            game.draw(&mut r, false);
            r.rect(0.0, 0.0, 640.0, 40.0, [0.88, 0.9, 0.93, 1.0]);
            r.centered(info.name, 0.0, 7.0, 640.0, 22, [0.12, 0.14, 0.17, 1.0]);
            r.finish();
            r.screenshot(Path::new(&format!("/out/puzzle-{}-{phase}.png", info.id)))?;
        }
        eprintln!("PASS {}", info.name);
    }
    Ok(())
}
