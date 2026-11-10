//! The original Go player geometry, in its 320×220 content coordinate system.
//! Keep these transforms in source order: changing their order changes the
//! cover's resting position and the path followed by the shared flip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub scale: f64,
    pub z2: f64,
    pub angle: f64,
    pub ty: f64,
    pub scale2: f64,
    pub widen: f64,
}
impl Pose {
    pub const FRONT: Self = Self {
        x: 100.,
        y: 4.,
        z: 20.,
        scale: 1.,
        z2: 0.,
        angle: 0.,
        ty: 0.,
        scale2: 1.,
        widen: 0.,
    };
    pub const BACK: Self = Self {
        angle: -180.,
        ty: 32.,
        scale2: 0.96,
        ..Self::FRONT
    };
    pub fn lerp(self, b: Self, t: f64) -> Self {
        let f = |a: f64, b: f64| a + (b - a) * t;
        Self {
            x: f(self.x, b.x),
            y: f(self.y, b.y),
            z: f(self.z, b.z),
            scale: f(self.scale, b.scale),
            z2: f(self.z2, b.z2),
            angle: f(self.angle, b.angle),
            ty: f(self.ty, b.ty),
            scale2: f(self.scale2, b.scale2),
            widen: f(self.widen, b.widen),
        }
    }
}
pub fn flow_offset(i: isize, width: f64) -> f64 {
    if i == 0 {
        0.
    } else {
        width - 46. + i as f64 * 48. + if i < 0 { -48. } else { 24. }
    }
}
pub fn side_pose(delta: isize) -> Pose {
    Pose {
        x: flow_offset(delta, 160.),
        y: 0.,
        z: 0.,
        scale: 1.1,
        z2: -65.,
        angle: if delta > 0 { -70. } else { 70. },
        ty: 0.,
        scale2: 1.,
        widen: 0.,
    }
}
pub fn now_playing_pose(t: f64) -> Pose {
    let theta0 = 35_f64.to_radians();
    let theta = theta0 * t;
    Pose {
        x: 160. / 9. + 64. * theta0.cos() - 64. * theta.cos(),
        y: 8.,
        z: -64. * theta0.sin() + 64. * theta.sin(),
        scale: 1.,
        z2: 0.,
        angle: 35. * t,
        ty: 0.,
        scale2: 0.85 + 0.15 * t,
        widen: 9. / 128. * t,
    }
}
pub fn card_grow(angle: f64) -> f64 {
    (-angle.to_radians().cos()).max(0.)
}
pub type Matrix = [f64; 16];
fn mul(a: Matrix, b: Matrix) -> Matrix {
    let mut r = [0.; 16];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                r[i * 4 + j] += a[i * 4 + k] * b[k * 4 + j];
            }
        }
    }
    r
}
fn translation(x: f64, y: f64, z: f64) -> Matrix {
    [1., 0., 0., x, 0., 1., 0., y, 0., 0., 1., z, 0., 0., 0., 1.]
}
fn scale(s: f64) -> Matrix {
    [s, 0., 0., 0., 0., s, 0., 0., 0., 0., s, 0., 0., 0., 0., 1.]
}
fn rotation(degrees: f64) -> Matrix {
    let (c, s) = (degrees.to_radians().cos(), degrees.to_radians().sin());
    [c, 0., s, 0., 0., 1., 0., 0., -s, 0., c, 0., 0., 0., 0., 1.]
}
pub fn matrix(p: Pose) -> Matrix {
    let mut m = translation(p.x, p.y, p.z);
    for b in [
        scale(p.scale),
        translation(0., 0., p.z2),
        rotation(p.angle),
        translation(0., p.ty, 0.),
        scale(p.scale2),
    ] {
        m = mul(m, b)
    }
    let mut full = mul(
        translation(64., 84., 0.),
        mul(m, translation(-64., -64., 0.)),
    );
    if p.widen != 0. {
        full = mul(
            full,
            [
                1. + p.widen,
                0.,
                0.,
                0.,
                0.,
                1.,
                0.,
                0.,
                0.,
                0.,
                1.,
                0.,
                0.,
                0.,
                0.,
                1.,
            ],
        );
    }
    let perspective = [
        1.,
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
        0.,
        0.,
        0.,
        1.,
        0.,
        0.,
        0.,
        -1. / 500.,
        1.,
    ];
    mul(
        mul(
            translation(160., 110., 0.),
            mul(perspective, translation(-160., -110., 0.)),
        ),
        full,
    )
}
pub fn plane(m: Matrix) -> [f64; 9] {
    [m[0], m[1], m[3], m[4], m[5], m[7], m[12], m[13], m[15]]
}
pub fn project(h: [f64; 9], u: f64, v: f64) -> [f64; 3] {
    let w = h[6] * u + h[7] * v + h[8];
    [
        (h[0] * u + h[1] * v + h[2]) / w,
        (h[3] * u + h[4] * v + h[5]) / w,
        w,
    ]
}
pub fn front(p: Pose) -> bool {
    let h = plane(matrix(p));
    let a = project(h, 0., 0.);
    let b = project(h, 128., 0.);
    let c = project(h, 0., 128.);
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.
}
/// GPU vertices carry the original projective denominator, preserving the
/// inverse-mapped source texture used by Go's software rasterizer.
pub fn quad(p: Pose, u0: f64, v0: f64, u1: f64, v1: f64) -> [[f32; 3]; 4] {
    let h = plane(matrix(p));
    [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| {
        let q = project(h, u, v);
        [(q[0] * 2.) as f32, (40. + q[1] * 2.) as f32, q[2] as f32]
    })
}
pub fn info_height() -> f64 {
    let a = crate::render::line_box(32.).0 as f64 / 2.;
    let b = crate::render::line_box(28.).0 as f64 / 2.;
    a + 4. + b + 4. + 1.
}
pub fn backside_height() -> f64 {
    let h = plane(matrix(Pose::BACK));
    let bv0 = -35.84;
    let mut lo = bv0 + 60.;
    let mut hi = bv0 + 600.;
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        let bottom = project(h, -64., mid)[1].max(project(h, 192., mid)[1]);
        if bottom > 221. {
            hi = mid
        } else {
            lo = mid
        }
    }
    (lo - bv0).max(info_height() + 1. + 25. + 1.)
}
/// Map the card's full-size physical local canvas onto the growing, mirrored
/// back of the original cover plane. The final card height stays constant.
pub fn card_homography(p: Pose) -> [[f32; 3]; 3] {
    let g = card_grow(p.angle);
    let height = backside_height();
    let u0 = -64. * g;
    let u1 = 128. + 64. * g;
    let v0 = -35.84 * g;
    let v1 = 128. + (-35.84 + height - 128.) * g;
    let a = -(u1 - u0) / 512.;
    let b = (v1 - v0) / (height * 2.);
    let tx = u1;
    let ty = v0;
    let h = plane(matrix(p));
    let x = [
        2. * h[0] * a,
        2. * h[1] * b,
        2. * (h[0] * tx + h[1] * ty + h[2]),
    ];
    let w = [h[6] * a, h[7] * b, h[6] * tx + h[7] * ty + h[8]];
    let y = [
        2. * h[3] * a + 40. * w[0],
        2. * h[4] * b + 40. * w[1],
        2. * (h[3] * tx + h[4] * ty + h[5]) + 40. * w[2],
    ];
    [
        x.map(|v| v as f32),
        y.map(|v| v as f32),
        w.map(|v| v as f32),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cover_targets_match_original_source() {
        assert_eq!(flow_offset(0, 160.), 0.);
        assert_eq!(flow_offset(-1, 160.), 18.);
        assert_eq!(flow_offset(1, 160.), 186.);
        assert_eq!(flow_offset(-2, 160.), -30.);
        assert_eq!(flow_offset(2, 160.), 234.);
        assert!(front(Pose::FRONT));
        assert!(!front(Pose::BACK));
    }
    #[test]
    fn np_resting_pose_keeps_exact_tilt_and_pause_scale() {
        let p = now_playing_pose(1.);
        assert!((p.x - 160. / 9.).abs() < 1e-10);
        assert!(p.z.abs() < 1e-10);
        assert_eq!(p.angle, 35.);
        assert_eq!(p.scale2, 1.);
        let p = now_playing_pose(0.);
        assert_eq!(p.scale2, 0.85);
        assert_eq!(p.widen, 0.);
    }
    #[test]
    fn final_card_lands_at_original_screen_bottom() {
        let height = backside_height();
        let h = plane(matrix(Pose::BACK));
        let bottom = project(h, -64., -35.84 + height)[1].max(project(h, 192., -35.84 + height)[1]);
        assert!((bottom - 221.).abs() < 1e-8);
    }
    #[test]
    fn card_is_mirrored_on_back_and_full_size_only_after_turn() {
        assert_eq!(card_grow(0.), 0.);
        assert!((card_grow(-180.) - 1.).abs() < 1e-12);
        let h = card_homography(Pose::BACK);
        let apply = |x: f32, y: f32| {
            let w = h[2][0] * x + h[2][1] * y + h[2][2];
            [
                (h[0][0] * x + h[0][1] * y + h[0][2]) / w,
                (h[1][0] * x + h[1][1] * y + h[1][2]) / w,
            ]
        };
        assert!(apply(512., 0.)[0] > apply(0., 0.)[0]);
        assert!((apply(0., backside_height() as f32 * 2.)[1] - 482.).abs() < 0.001);
    }
}
