//! Go 1.1 Bluetooth row tiles and device classification (ij_bluetooth.go).
//! Generated once before the event loop at twice their native display size.
use super::{alpha, circle, gradient, rgb, rounded, GeneratedIcon, Pen};
use crate::bluetooth::Device;
use tiny_skia as sk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Headphones,
    Earbuds,
    Speaker,
    Gamepad,
    Other,
}
const KINDS: [Kind; 5] = [
    Kind::Headphones,
    Kind::Earbuds,
    Kind::Speaker,
    Kind::Gamepad,
    Kind::Other,
];
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Headphones => "bt_headphones",
            Self::Earbuds => "bt_earbuds",
            Self::Speaker => "bt_speaker",
            Self::Gamepad => "bt_gamepad",
            Self::Other => "bt_other",
        }
    }
    fn colors(self) -> (u32, u32) {
        match self {
            Self::Headphones => (0x5ec2ff, 0x1769d6),
            Self::Earbuds => (0x6fe0d0, 0x14958a),
            Self::Speaker => (0xffb35c, 0xe2620f),
            Self::Gamepad => (0xb58cff, 0x6a3fd1),
            Self::Other => (0x9fb2c8, 0x56697f),
        }
    }
    fn for_device(device: &Device) -> Self {
        let name = device.name.to_lowercase();
        if !name.contains("max")
            && [
                "airpods",
                "buds",
                "pods",
                "earbud",
                "tws",
                "wf-",
                "elite",
                "free",
                "fit pro",
                "studio buds",
                "beats fit",
                "powerbeats",
                "liberty",
                "soundcore life p",
                "ear (",
                "nothing ear",
            ]
            .iter()
            .any(|word| name.contains(word))
        {
            return Self::Earbuds;
        }
        if device.icon.starts_with("audio-head") {
            Self::Headphones
        } else if device.icon == "audio-card"
            || ["speaker", "flip", "boom", "soundlink"]
                .iter()
                .any(|word| name.contains(word))
        {
            Self::Speaker
        } else if device.icon == "input-gaming"
            || ["controller", "gamepad", "xbox", "dualsense"]
                .iter()
                .any(|word| name.contains(word))
        {
            Self::Gamepad
        } else if ["wh-", "headphone", "qc", "max"]
            .iter()
            .any(|word| name.contains(word))
        {
            Self::Headphones
        } else {
            Self::Other
        }
    }
}
pub fn device_icon_name(device: &Device) -> &'static str {
    Kind::for_device(device).name()
}

pub fn generate() -> Result<Vec<GeneratedIcon>, String> {
    let mut icons = Vec::with_capacity(KINDS.len());
    for kind in KINDS {
        let mut pen = Pen::new(96, 100.)?;
        let (top, bottom) = kind.colors();
        pen.round(8., 8., 84., 84., 20., &[(0., rgb(top)), (1., rgb(bottom))]);
        pen.fill(
            &rounded(10., 9.5, 80., 38., 18.),
            gradient(
                9.,
                39.,
                &[(0., alpha(0xffffff, 0.45)), (1., alpha(0xffffff, 0.05))],
            ),
        );
        let transform = pen.transform;
        pen.transform = transform.pre_translate(0., 1.5);
        draw_symbol(&mut pen, kind, alpha(0, 0.18), rgb(bottom));
        pen.transform = transform;
        draw_symbol(&mut pen, kind, rgb(0xffffff), rgb(bottom));
        icons.push(pen.finish(kind.name(), 96));
    }
    Ok(icons)
}
fn fill(pen: &mut Pen, path: &sk::Path, color: sk::Color) {
    pen.fill(path, sk::Shader::SolidColor(color));
}
fn stroke(pen: &mut Pen, path: &sk::Path, color: sk::Color) {
    pen.image.stroke_path(
        path,
        &sk::Paint {
            shader: sk::Shader::SolidColor(color),
            anti_alias: true,
            ..Default::default()
        },
        &sk::Stroke {
            width: 6.,
            line_cap: sk::LineCap::Round,
            line_join: sk::LineJoin::Round,
            ..Default::default()
        },
        pen.transform,
        None,
    );
}
fn rect(x: f32, y: f32, width: f32, height: f32) -> sk::Path {
    let mut path = sk::PathBuilder::new();
    path.push_rect(sk::Rect::from_xywh(x, y, width, height).unwrap());
    path.finish().unwrap()
}
fn draw_symbol(pen: &mut Pen, kind: Kind, white: sk::Color, inset: sk::Color) {
    match kind {
        Kind::Headphones => {
            let k = 0.552_284_8 * 25.;
            let mut arc = sk::PathBuilder::new();
            arc.move_to(25., 54.);
            arc.cubic_to(25., 54. - k, 50. - k, 29., 50., 29.);
            arc.cubic_to(50. + k, 29., 75., 54. - k, 75., 54.);
            stroke(pen, &arc.finish().unwrap(), white);
            for x in [20., 66.] {
                fill(pen, &rounded(x, 50., 14., 24., 6.), white);
            }
        }
        Kind::Earbuds => {
            for x in [36., 64.] {
                fill(pen, &circle(x, 40., 10.), white);
                fill(pen, &rounded(x - 3.5, 42., 7., 28., 3.5), white);
            }
        }
        Kind::Speaker => {
            fill(pen, &rounded(30., 22., 40., 56., 9.), white);
            fill(pen, &circle(50., 58., 11.), inset);
            fill(pen, &circle(50., 35., 5.), inset);
        }
        Kind::Gamepad => {
            fill(pen, &rounded(18., 36., 64., 32., 16.), white);
            fill(pen, &rect(29., 49.5, 16., 5.), inset);
            fill(pen, &rect(34.5, 44., 5., 16.), inset);
            fill(pen, &circle(62., 48., 3.5), inset);
            fill(pen, &circle(70., 56., 3.5), inset);
        }
        Kind::Other => {
            let mut rune = sk::PathBuilder::new();
            rune.move_to(36., 36.);
            for (x, y) in [(62., 60.), (50., 72.), (50., 28.), (62., 40.), (36., 64.)] {
                rune.line_to(x, y);
            }
            stroke(pen, &rune.finish().unwrap(), white);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn device(name: &str, icon: &str) -> Device {
        Device {
            address: "00:11:22:33:44:55".into(),
            name: name.into(),
            icon: icon.into(),
            paired: true,
            connected: false,
            battery: None,
        }
    }
    #[test]
    fn source_device_classification_keeps_headsets_earbuds_speakers_and_controllers_distinct() {
        for (name, icon, expected) in [
            ("WH-1000XM6", "audio-headset", "bt_headphones"),
            ("Sony WH-1000XM6", "", "bt_headphones"),
            ("WF-1000XM5", "audio-headphones", "bt_earbuds"),
            ("AirPods Pro", "audio-headset", "bt_earbuds"),
            ("AirPods Max", "audio-headphones", "bt_headphones"),
            ("Living room", "audio-card", "bt_speaker"),
            ("JBL Flip 6", "", "bt_speaker"),
            ("SOUNDLINK", "", "bt_speaker"),
            ("Wireless controller", "input-gaming", "bt_gamepad"),
            ("Xbox Wireless", "", "bt_gamepad"),
            ("Laptop", "computer", "bt_other"),
        ] {
            assert_eq!(device_icon_name(&device(name, icon)), expected, "{name}");
        }
    }
    #[test]
    fn every_device_kind_has_a_distinct_high_resolution_tile() {
        let icons = generate().unwrap();
        assert_eq!(icons.len(), 5);
        let names = icons
            .iter()
            .map(|i| i.name)
            .collect::<std::collections::HashSet<_>>();
        for kind in KINDS {
            assert!(names.contains(kind.name()));
        }
        let mut rasters = std::collections::HashSet::new();
        for icon in icons {
            assert_eq!((icon.width, icon.height, icon.display_size), (192, 192, 96));
            assert!(icon.pixels.chunks_exact(4).any(|p| p[3] == 0));
            assert!(icon
                .pixels
                .chunks_exact(4)
                .any(|p| p[3] == 255 && p[..3].iter().all(|c| *c > 245)));
            assert!(
                rasters.insert(icon.pixels),
                "device kinds need distinct pictures"
            );
        }
    }
}
