//! Local visual probe that uses the production Rust renderer/game modules.
//! This is not the player executable and does not make timing/audio claims.
use betterpod_rust::{
    games::{GameKind, Games},
    input::Action,
    render::{Renderer, BLACK},
};
use std::path::Path;

fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let attr = video.gl_attr();
    attr.set_context_profile(sdl2::video::GLProfile::GLES);
    attr.set_context_version(2, 0);
    let window = video
        .window("Rust games visual probe", 640, 480)
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let _context = window.gl_create_context()?;
    let mut renderer = Renderer::new(
        &video,
        Path::new("/repo/port/betterpod/assets/ui_font.ttf"),
        640,
        480,
    )?;
    for (name, kind) in [
        ("2048", GameKind::TwentyFortyEight),
        ("brick", GameKind::Brick),
        ("solitaire", GameKind::Solitaire),
        ("fiveletters", GameKind::FiveLetters),
    ] {
        let mut game = Games::new(kind);
        renderer.begin(BLACK);
        game.draw(&mut renderer, false);
        renderer.rect(0.0, 0.0, 640.0, 40.0, [0.88, 0.9, 0.93, 1.0]);
        renderer.centered(game.title(), 0.0, 7.0, 640.0, 22, [0.12, 0.14, 0.17, 1.0]);
        renderer.finish();
        renderer.screenshot(Path::new(&format!("/out/game-{name}.png")))?;
        game.handle(Action::Select);
        game.tick(0.08);
        if kind == GameKind::Solitaire {
            game.handle(Action::Down);
            game.handle(Action::Options);
        } else {
            game.handle(Action::Right);
        }
        renderer.begin(BLACK);
        game.draw(&mut renderer, true);
        renderer.rect(0.0, 0.0, 640.0, 40.0, [0.12, 0.14, 0.17, 1.0]);
        renderer.centered(game.title(), 0.0, 7.0, 640.0, 22, [0.9, 0.92, 0.95, 1.0]);
        renderer.finish();
        renderer.screenshot(Path::new(&format!("/out/game-{name}-active.png")))?;
    }
    Ok(())
}
