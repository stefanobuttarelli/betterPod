//! Controller square-cover framing. Pixel cropping and metadata writes are worker jobs.
use super::{Action, DownloadRequest, Renderer, FG, MUTED, WHITE};
use betterpod_rust::{art::Artwork, downloads::SquareCrop};
use std::path::PathBuf;
pub(super) struct Editor {
    pub url: String,
    pub path: Option<PathBuf>,
    request: DownloadRequest,
    anchor: usize,
}
pub(super) enum Effect {
    Cancel,
    Default(DownloadRequest),
    Crop {
        request: DownloadRequest,
        path: PathBuf,
        crop: SquareCrop,
    },
}
impl Editor {
    pub fn new(request: DownloadRequest, url: String, path: Option<PathBuf>) -> Self {
        Self {
            url,
            path,
            request,
            anchor: 1,
        }
    }
    pub fn handle(&mut self, action: Action) -> Option<Effect> {
        match action {
            Action::Left => self.anchor = self.anchor.saturating_sub(1),
            Action::Right => self.anchor = (self.anchor + 1).min(2),
            Action::Back | Action::BackHeld => return Some(Effect::Cancel),
            Action::Options => return Some(Effect::Default(self.request.clone())),
            Action::Select => {
                if let Some(path) = self.path.clone() {
                    return Some(Effect::Crop {
                        request: self.request.clone(),
                        path,
                        crop: match self.anchor {
                            0 => SquareCrop::Start,
                            2 => SquareCrop::End,
                            _ => SquareCrop::Center,
                        },
                    });
                }
            }
            _ => {}
        }
        None
    }
    pub fn draw(&self, r: &mut Renderer, art: &mut Artwork, dark: bool) {
        let fg = if dark { WHITE } else { FG };
        let bg = if dark {
            [0.07, 0.08, 0.1, 1.0]
        } else {
            [0.96, 0.96, 0.97, 1.0]
        };
        r.rect(0.0, 40.0, 640.0, 440.0, bg);
        r.centered("Choose the Square Cover", 20.0, 56.0, 600.0, 25, fg);
        if let Some(path) = self.path.as_deref() {
            let image = art.get(Some(path));
            let scale = (256.0 / image.width as f32).max(256.0 / image.height as f32);
            let w = image.width as f32 * scale;
            let h = image.height as f32 * scale;
            let anchor = self.anchor as f32 / 2.0;
            r.set_clip(192.0, 105.0, 256.0, 256.0);
            r.image(
                image,
                192.0 - (w - 256.0) * anchor,
                105.0 - (h - 256.0) * anchor,
                w,
                h,
                1.0,
            );
            r.reset_clip();
        } else {
            r.centered("Loading source artwork...", 20.0, 220.0, 600.0, 20, fg);
        }
        for (i, label) in ["Start", "Center", "End"].iter().enumerate() {
            let x = 123.0 + i as f32 * 136.0;
            let selected = i == self.anchor;
            if selected {
                r.rect(x, 383.0, 123.0, 34.0, [0.12, 0.48, 0.88, 1.0]);
            }
            r.centered(
                label,
                x,
                389.0,
                123.0,
                20,
                if selected { WHITE } else { fg },
            );
        }
        r.centered(
            "LEFT / RIGHT: Frame   A: Use   Y: Default   B: Cancel",
            10.0,
            451.0,
            620.0,
            16,
            if dark { [0.65, 0.68, 0.73, 1.0] } else { MUTED },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> DownloadRequest {
        DownloadRequest::YouTube {
            url: "https://example.invalid/video".into(),
            video: false,
            split_chapters: true,
        }
    }
    #[test]
    fn framing_requires_ready_artwork_and_preserves_download_options() {
        let mut editor = Editor::new(request(), "art".into(), None);
        assert!(editor.handle(Action::Select).is_none());
        editor.path = Some("/private/cover.jpg".into());
        editor.handle(Action::Right);
        match editor.handle(Action::Select).unwrap() {
            Effect::Crop {
                request:
                    DownloadRequest::YouTube {
                        split_chapters: true,
                        ..
                    },
                crop: SquareCrop::End,
                ..
            } => {}
            _ => panic!("crop or download options changed"),
        }
    }
    #[test]
    fn cancel_and_default_are_explicit_separate_actions() {
        let mut editor = Editor::new(request(), "art".into(), None);
        assert!(matches!(editor.handle(Action::Back), Some(Effect::Cancel)));
        assert!(matches!(
            editor.handle(Action::Options),
            Some(Effect::Default(_))
        ));
    }
}
