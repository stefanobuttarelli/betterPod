//! Deterministic actual Go-game positions. Included by the opt-in QA harness.
use super::*;
pub(super) fn seed(
    app: &mut App,
    screen: &str,
    fixture_root: &std::path::Path,
) -> Result<bool, String> {
    let kind = match screen {
        "game2048" => GameKind::TwentyFortyEight,
        "gamebrick" => GameKind::Brick,
        "gamesolitaire" => GameKind::Solitaire,
        "gamewords" => GameKind::FiveLetters,
        _ => return Ok(false),
    };
    let path = fixture_root.join("games").join(format!("{screen}.json"));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut game = Games::new(kind);
    game.restore_reference_fixture(&bytes)?;
    game.set_assets(&app.visual_assets);
    game.set_clock_shown(false);
    game.set_time(0.0);
    game.snap_visual_state();
    app.game = Some(game);
    app.pages.push(app.page(Screen::Game));
    Ok(true)
}
