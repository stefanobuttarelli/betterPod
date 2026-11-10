//! GPU composition. Rust prepares small vertex batches; the GPU rasterizes
//! gradients, text and perspective covers. No full-frame CPU pixel buffer.
use glow::HasContext;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
mod fallback;
use fallback::{OutlineFont, RasterFont};
#[cfg(test)]
mod qa;

pub type Color = [f32; 4];
pub const WHITE: Color = [1.0; 4];
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// Physical-pixel placement shared by menu, album-card and saver scrollbars.
/// Keep the twelve-pixel rail centered in its reserved column.
pub fn scrollbar_track_x(column_x: f32, column_width: f32) -> f32 {
    column_x + (column_width - 12.0) * 0.5
}

#[derive(Clone, Copy)]
pub struct Texture {
    pub id: glow::NativeTexture,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    width: f32,
    height: f32,
    left: f32,
    top: f32,
}
struct FontAtlas {
    texture: Texture,
    glyphs: HashMap<(char, u16, u16), Glyph>,
    advances: HashMap<(char, u16, u16), f32>,
    fonts: HashMap<u16, fontdue::Font>,
    metrics: HashMap<u16, crate::font_metrics::MetricsFont>,
    fallback: [Option<OutlineFont>; 2],
    pixels: Vec<u8>,
    x: usize,
    y: usize,
    row_height: usize,
    dirty: Option<[usize; 4]>,
    upload: Vec<u8>,
}
#[derive(Clone, Copy, PartialEq)]
struct Clip {
    rect: [i32; 4],
    edges: [[f32; 3]; 8],
    count: usize,
}
impl Clip {
    fn full() -> Self {
        Self {
            rect: [0, 0, 640, 480],
            edges: [[0.0; 3]; 8],
            count: 0,
        }
    }
}
#[derive(Clone, Copy)]
struct Batch {
    texture: glow::NativeTexture,
    start: i32,
    count: i32,
    clip: Clip,
}

pub struct Renderer {
    gl: glow::Context,
    program: glow::NativeProgram,
    buffer: glow::NativeBuffer,
    vao: Option<glow::NativeVertexArray>,
    white: Texture,
    atlas: FontAtlas,
    vertices: Vec<f32>,
    batches: Vec<Batch>,
    retired: RefCell<Vec<Texture>>,
    clip: Clip,
    position: u32,
    uv: u32,
    color: u32,
    width: i32,
    height: i32,
    transform: [[f32; 3]; 3],
    opacity: f32,
    transforms: Vec<([[f32; 3]; 3], f32, Clip)>,
    clips: Vec<Clip>,
    pub backend: String,
    pub samples: i32,
    pub art_anisotropy: f32,
    art_npot: bool,
}

impl Renderer {
    pub fn new(
        video: &sdl2::VideoSubsystem,
        font_path: &Path,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let gl = unsafe {
            glow::Context::from_loader_function(|name| video.gl_get_proc_address(name).cast())
        };
        let desktop = cfg!(target_os = "macos");
        let version = if desktop {
            "#version 150\n"
        } else {
            "#version 100\nprecision mediump float;\n"
        };
        let vert = if desktop {
            "in vec3 a_position; in vec2 a_uv; in vec4 a_color; out vec2 v_uv; out vec4 v_color; void main(){gl_Position=vec4(a_position.xy*a_position.z,0.0,a_position.z);v_uv=a_uv;v_color=a_color;}"
        } else {
            "attribute vec3 a_position; attribute vec2 a_uv; attribute vec4 a_color; varying highp vec2 v_uv; varying vec4 v_color; void main(){gl_Position=vec4(a_position.xy*a_position.z,0.0,a_position.z);v_uv=a_uv;v_color=a_color;}"
        };
        // The clip planes are tested in 640x480 units. Under GLES's
        // mediump (fp16 on Mali), gl_FragCoord.x*640.0 overflows past
        // x=102 and every fragment of a projective clip was discarded
        // (the NP card's rows vanished while it flipped): the position and
        // the planes are highp, and the scale is taken before the multiply.
        let frag = if desktop {
            "uniform sampler2D u_texture; uniform vec2 u_viewport; uniform int u_clip_count; uniform vec3 u_clip_edges[8]; in vec2 v_uv; in vec4 v_color; out vec4 out_color; void main(){vec3 p=vec3(gl_FragCoord.x*(640.0/u_viewport.x),480.0-gl_FragCoord.y*(480.0/u_viewport.y),1.0);for(int i=0;i<8;i++){if(i<u_clip_count&&dot(p,u_clip_edges[i])<0.0)discard;}out_color=texture(u_texture,v_uv)*v_color;}"
        } else {
            "uniform sampler2D u_texture; uniform highp vec2 u_viewport; uniform int u_clip_count; uniform highp vec3 u_clip_edges[8]; varying highp vec2 v_uv; varying vec4 v_color; void main(){highp vec3 p=vec3(gl_FragCoord.x*(640.0/u_viewport.x),480.0-gl_FragCoord.y*(480.0/u_viewport.y),1.0);for(int i=0;i<8;i++){if(i<u_clip_count&&dot(p,u_clip_edges[i])<0.0)discard;}gl_FragColor=texture2D(u_texture,v_uv)*v_color;}"
        };
        let program = unsafe { gl.create_program()? };
        for (kind, source) in [(glow::VERTEX_SHADER, vert), (glow::FRAGMENT_SHADER, frag)] {
            let shader = unsafe { gl.create_shader(kind)? };
            unsafe {
                gl.shader_source(shader, &(version.to_owned() + source));
                gl.compile_shader(shader);
            }
            if !unsafe { gl.get_shader_compile_status(shader) } {
                return Err(unsafe { gl.get_shader_info_log(shader) });
            }
            unsafe {
                gl.attach_shader(program, shader);
                gl.delete_shader(shader);
            }
        }
        unsafe {
            gl.link_program(program);
        }
        if !unsafe { gl.get_program_link_status(program) } {
            return Err(unsafe { gl.get_program_info_log(program) });
        }
        let attribute = |name| unsafe {
            gl.get_attrib_location(program, name)
                .ok_or_else(|| format!("Missing shader attribute {name}"))
        };
        let position = attribute("a_position")?;
        let uv = attribute("a_uv")?;
        let color = attribute("a_color")?;
        let buffer = unsafe { gl.create_buffer()? };
        let vao = if desktop {
            Some(unsafe { gl.create_vertex_array()? })
        } else {
            None
        };
        let white = upload(&gl, 1, 1, &[255; 4])?;
        let inter = font_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("fonts/Inter-Regular.otf");
        let primary = if inter.is_file() {
            inter.as_path()
        } else {
            font_path
        };
        let load_font = |path: &Path| -> Result<fontdue::Font, String> {
            fontdue::Font::from_bytes(
                std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?,
                fontdue::FontSettings::default(),
            )
            .map_err(|e| e.to_string())
        };
        let directory = inter.parent().unwrap_or_else(|| Path::new("."));
        let mut fonts = HashMap::new();
        let mut metrics = HashMap::new();
        metrics.insert(
            400,
            crate::font_metrics::MetricsFont::new(
                std::fs::read(primary).map_err(|e| e.to_string())?,
            )?,
        );
        fonts.insert(400, load_font(primary)?);
        for (weight, name) in [
            (300, "InterDisplay-Light-Digits.otf"),
            (500, "Inter-Medium.otf"),
            (600, "Inter-SemiBold.otf"),
            (700, "Inter-Bold.otf"),
            (900, "Inter-Black.otf"),
        ] {
            let path = directory.join(name);
            if path.is_file() {
                fonts.insert(weight, load_font(&path)?);
                metrics.insert(
                    weight,
                    crate::font_metrics::MetricsFont::new(
                        std::fs::read(&path).map_err(|e| e.to_string())?,
                    )?,
                );
            }
        }
        let fallback = ["NotoSansJP-Regular.otf", "NotoSansJP-Bold.otf"].map(|name| {
            std::fs::read(directory.join(name))
                .ok()
                .and_then(|data| OutlineFont::new(data).ok())
        });
        let pixels = vec![0; 2048 * 2048 * 4];
        let atlas_texture = upload(&gl, 2048, 2048, &pixels)?;
        let backend = unsafe {
            format!(
                "{} / {}",
                gl.get_parameter_string(glow::RENDERER),
                gl.get_parameter_string(glow::VERSION)
            )
        };
        unsafe {
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "u_texture").as_ref(), 0);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::SCISSOR_TEST);
        }
        let samples = unsafe { gl.get_parameter_i32(glow::SAMPLES) };
        let art_npot = !gl.version().is_embedded
            || gl.version().major >= 3
            || gl.supported_extensions().contains("GL_OES_texture_npot");
        let art_anisotropy = if gl
            .supported_extensions()
            .contains("GL_EXT_texture_filter_anisotropic")
        {
            unsafe {
                gl.get_parameter_f32(glow::MAX_TEXTURE_MAX_ANISOTROPY_EXT)
                    .clamp(1., 4.)
            }
        } else {
            1.
        };
        let mut out = Self {
            gl,
            program,
            buffer,
            vao,
            white,
            atlas: FontAtlas {
                texture: atlas_texture,
                glyphs: HashMap::new(),
                advances: HashMap::new(),
                fonts,
                metrics,
                fallback,
                pixels,
                x: 1,
                y: 1,
                row_height: 0,
                dirty: None,
                upload: Vec::new(),
            },
            vertices: Vec::with_capacity(65536),
            batches: Vec::with_capacity(512),
            retired: RefCell::new(Vec::new()),
            clip: Clip::full(),
            position,
            uv,
            color,
            width,
            height,
            transform: identity(),
            opacity: 1.0,
            transforms: Vec::with_capacity(12),
            clips: Vec::with_capacity(12),
            backend,
            samples,
            art_anisotropy,
            art_npot,
        };
        for size in [16, 18, 20, 22, 24, 26, 28, 32, 36] {
            for c in (32..=255).filter_map(char::from_u32) {
                out.glyph(c, size as f32, 400);
            }
        }
        for c in "0123456789-+.°CF".chars() {
            out.glyph(c, 48.0, 400);
        }
        Ok(out)
    }

    pub fn upload(&self, width: u32, height: u32, pixels: &[u8]) -> Result<Texture, String> {
        upload(&self.gl, width, height, pixels)
    }
    /// Static album art only. Mipmaps prevent shimmer inside a shrinking or
    /// tilted cover; MSAA handles its outer polygon edge separately.
    pub fn upload_artwork(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
        mipmaps: &[image::RgbaImage],
    ) -> Result<Texture, String> {
        let texture = self.upload(width, height, pixels)?;
        // Preserve rectangular artwork too; older GLES2 drivers may require
        // power-of-two dimensions, so retain their ordinary linear fallback.
        if self.art_npot || width.is_power_of_two() && height.is_power_of_two() {
            unsafe {
                self.gl.bind_texture(glow::TEXTURE_2D, Some(texture.id));
                if mipmaps.is_empty() {
                    self.gl.generate_mipmap(glow::TEXTURE_2D);
                } else {
                    for (index, image) in mipmaps.iter().enumerate() {
                        self.gl.tex_image_2d(
                            glow::TEXTURE_2D,
                            index as i32 + 1,
                            glow::RGBA as i32,
                            image.width() as i32,
                            image.height() as i32,
                            0,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelUnpackData::Slice(Some(image.as_raw())),
                        );
                    }
                }
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR_MIPMAP_LINEAR as i32,
                );
                if self.art_anisotropy > 1. {
                    self.gl.tex_parameter_f32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MAX_ANISOTROPY_EXT,
                        self.art_anisotropy,
                    );
                }
            }
        }
        Ok(texture)
    }
    pub fn set_texture_nearest(&self, texture: Texture) {
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture.id));
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            self.gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
        }
    }
    pub fn delete_texture(&self, texture: Texture) {
        // Images are batched on the CPU until finish(). Cache eviction during
        // composition must not delete a texture referenced by an earlier image.
        if self.batches.iter().any(|batch| batch.texture == texture.id) {
            self.retired.borrow_mut().push(texture);
            return;
        }
        unsafe {
            self.gl.delete_texture(texture.id);
        }
    }
    pub fn update_texture(&self, texture: Texture, pixels: &[u8]) -> Result<(), String> {
        if pixels.len() != texture.width as usize * texture.height as usize * 4 {
            return Err("Invalid texture update size".into());
        }
        unsafe {
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture.id));
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                texture.width as i32,
                texture.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(pixels)),
            );
        }
        Ok(())
    }
    pub fn begin(&mut self, bg: Color) {
        self.vertices.clear();
        self.batches.clear();
        for texture in self.retired.get_mut().drain(..) {
            unsafe { self.gl.delete_texture(texture.id) };
        }
        self.transform = identity();
        self.opacity = 1.0;
        self.transforms.clear();
        self.clips.clear();
        self.clip = Clip::full();
        unsafe {
            self.gl.viewport(0, 0, self.width, self.height);
            self.gl.scissor(0, 0, self.width, self.height);
            self.gl.clear_color(bg[0], bg[1], bg[2], bg[3]);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }
    pub fn set_clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let corners = [
            [x, y, 1.0],
            [x + width, y, 1.0],
            [x + width, y + height, 1.0],
            [x, y + height, 1.0],
        ]
        .map(|p| transform_point(self.transform, p));
        let x0 = corners
            .iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min)
            .clamp(0.0, 640.0);
        let y0 = corners
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .clamp(0.0, 480.0);
        let x1 = corners
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .clamp(x0, 640.0);
        let y1 = corners
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .clamp(y0, 480.0);
        self.clip = Clip::full();
        self.clip.rect = [
            x0.floor() as i32,
            y0.floor() as i32,
            (x1.ceil() - x0.floor()) as i32,
            (y1.ceil() - y0.floor()) as i32,
        ];
        let m = self.transform;
        if m[0][1].abs() + m[1][0].abs() + m[2][0].abs() + m[2][1].abs() > 1e-6 {
            let centre = [
                corners.iter().map(|p| p[0]).sum::<f32>() * 0.25,
                corners.iter().map(|p| p[1]).sum::<f32>() * 0.25,
            ];
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                let mut edge = [a[1] - b[1], b[0] - a[0], a[0] * b[1] - b[0] * a[1]];
                let length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt().max(1e-8);
                let sign = if edge[0] * centre[0] + edge[1] * centre[1] + edge[2] < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                for component in &mut edge {
                    *component *= sign / length;
                }
                self.clip.edges[i] = edge;
            }
            self.clip.count = 4;
        }
    }
    pub fn set_clip_screen(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let x0 = x.clamp(0.0, 640.0);
        let y0 = y.clamp(0.0, 480.0);
        let x1 = (x + width).clamp(x0, 640.0);
        let y1 = (y + height).clamp(y0, 480.0);
        self.clip = Clip::full();
        self.clip.rect = [
            x0.floor() as i32,
            y0.floor() as i32,
            (x1.ceil() - x0.floor()) as i32,
            (y1.ceil() - y0.floor()) as i32,
        ];
    }
    pub fn push_clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let parent = self.clip;
        self.clips.push(parent);
        self.set_clip(x, y, width, height);
        let a = parent.rect;
        let b = self.clip.rect;
        let x0 = a[0].max(b[0]);
        let y0 = a[1].max(b[1]);
        let x1 = (a[0] + a[2]).min(b[0] + b[2]);
        let y1 = (a[1] + a[3]).min(b[1] + b[3]);
        self.clip.rect = [x0, y0, (x1 - x0).max(0), (y1 - y0).max(0)];
        for edge in parent.edges.iter().take(parent.count) {
            if self.clip.count < 8 {
                self.clip.edges[self.clip.count] = *edge;
                self.clip.count += 1;
            }
        }
    }
    pub fn pop_clip(&mut self) {
        if let Some(clip) = self.clips.pop() {
            self.clip = clip;
        }
    }
    pub fn reset_clip(&mut self) {
        self.clip = Clip::full();
    }
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.gradient(x, y, w, h, color, color);
    }
    pub fn triangle(&mut self, p: [[f32; 2]; 3], color: Color) {
        self.colored_triangle(p, [color; 3]);
    }
    pub fn colored_triangle(&mut self, p: [[f32; 2]; 3], colors: [Color; 3]) {
        if self.clip.rect[2] <= 0 || self.clip.rect[3] <= 0 {
            return;
        }
        let start = (self.vertices.len() / 9) as i32;
        for (point, color) in p.into_iter().zip(colors) {
            let point = transform_point(self.transform, [point[0], point[1], 1.0]);
            self.vertices.extend_from_slice(&[
                point[0] / 320.0 - 1.0,
                1.0 - point[1] / 240.0,
                point[2],
                0.5,
                0.5,
                color[0],
                color[1],
                color[2],
                color[3] * self.opacity,
            ]);
        }
        if let Some(last) = self.batches.last_mut() {
            if last.texture == self.white.id && last.clip == self.clip {
                last.count += 3;
                return;
            }
        }
        self.batches.push(Batch {
            texture: self.white.id,
            start,
            count: 3,
            clip: self.clip,
        });
    }
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        let length = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        if length < 0.001 {
            return;
        }
        let dx = -(y1 - y0) / length * width * 0.5;
        let dy = (x1 - x0) / length * width * 0.5;
        self.quad(
            self.white,
            [
                [x0 + dx, y0 + dy, 1.0],
                [x1 + dx, y1 + dy, 1.0],
                [x1 - dx, y1 - dy, 1.0],
                [x0 - dx, y0 - dy, 1.0],
            ],
            [0.0, 0.0, 1.0, 1.0],
            [color; 4],
        );
    }
    pub fn circle(&mut self, x: f32, y: f32, radius: f32, color: Color) {
        for i in 0..32 {
            let a = i as f32 * std::f32::consts::TAU / 32.0;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 32.0;
            self.triangle(
                [
                    [x, y],
                    [x + radius * a.cos(), y + radius * a.sin()],
                    [x + radius * b.cos(), y + radius * b.sin()],
                ],
                color,
            );
        }
    }
    pub fn gradient(&mut self, x: f32, y: f32, w: f32, h: f32, top: Color, bottom: Color) {
        self.quad(
            self.white,
            [
                [x, y, 1.0],
                [x + w, y, 1.0],
                [x + w, y + h, 1.0],
                [x, y + h, 1.0],
            ],
            [0.0, 0.0, 1.0, 1.0],
            [top, top, bottom, bottom],
        );
    }
    pub fn image_cover(&mut self, texture: Texture, rect: [f32; 4], opacity: f32) {
        let image_ratio = texture.width as f32 / texture.height.max(1) as f32;
        let target_ratio = rect[2] / rect[3].max(1.);
        let (u, v) = if image_ratio > target_ratio {
            ((1. - target_ratio / image_ratio) * 0.5, 0.)
        } else {
            (0., (1. - image_ratio / target_ratio) * 0.5)
        };
        self.image_uv(texture, rect, [u, v, 1. - u, 1. - v], opacity);
    }
    pub fn image(&mut self, texture: Texture, x: f32, y: f32, w: f32, h: f32, opacity: f32) {
        let c = [1.0, 1.0, 1.0, opacity];
        self.quad(
            texture,
            [
                [x, y, 1.0],
                [x + w, y, 1.0],
                [x + w, y + h, 1.0],
                [x, y + h, 1.0],
            ],
            [0.0, 0.0, 1.0, 1.0],
            [c; 4],
        );
    }
    pub fn cover(
        &mut self,
        texture: Texture,
        cx: f32,
        cy: f32,
        size: f32,
        angle: f32,
        opacity: f32,
        reflection: bool,
    ) {
        let half = size * 0.5;
        let f = 540.0;
        let mut positions = [[0.0; 3]; 4];
        for (i, (x, y)) in [(-half, -half), (half, -half), (half, half), (-half, half)]
            .into_iter()
            .enumerate()
        {
            let z = f + x * angle.sin();
            let perspective = f / z;
            positions[i] = [
                cx + x * angle.cos() * perspective,
                cy + y * perspective,
                z / f,
            ];
        }
        let c = [1.0, 1.0, 1.0, opacity];
        self.quad(texture, positions, [0.0, 0.0, 1.0, 1.0], [c; 4]);
        if reflection {
            let mut reflected = positions;
            for p in &mut reflected {
                p[1] = 2.0 * (cy + half + 3.0) - p[1];
            }
            let a = [1.0, 1.0, 1.0, opacity * 0.0];
            let b = [1.0, 1.0, 1.0, opacity * 0.20];
            self.quad(texture, reflected, [0.0, 0.0, 1.0, 1.0], [a, a, b, b]);
        }
    }
    fn quad(&mut self, texture: Texture, p: [[f32; 3]; 4], uv: [f32; 4], colors: [Color; 4]) {
        if self.clip.rect[2] <= 0 || self.clip.rect[3] <= 0 {
            return;
        }
        let start = (self.vertices.len() / 9) as i32;
        let p = p.map(|point| transform_point(self.transform, point));
        for i in [0, 1, 2, 0, 2, 3] {
            self.vertices.extend_from_slice(&[
                p[i][0] / 320.0 - 1.0,
                1.0 - p[i][1] / 240.0,
                p[i][2],
                if i == 0 || i == 3 { uv[0] } else { uv[2] },
                if i < 2 { uv[1] } else { uv[3] },
                colors[i][0],
                colors[i][1],
                colors[i][2],
                colors[i][3] * self.opacity,
            ]);
        }
        if let Some(last) = self.batches.last_mut() {
            if last.texture == texture.id && last.clip == self.clip {
                last.count += 6;
                return;
            }
        }
        self.batches.push(Batch {
            texture: texture.id,
            start,
            count: 6,
            clip: self.clip,
        });
    }
    fn font(&self, c: char, weight: u16) -> RasterFont<'_> {
        let primary = self
            .atlas
            .fonts
            .get(&weight)
            .unwrap_or(&self.atlas.fonts[&400]);
        if primary.lookup_glyph_index(c) == 0 {
            self.atlas.fallback[usize::from(weight >= 600)]
                .as_ref()
                .filter(|f| f.lookup_glyph_index(c) != 0)
                .map_or(RasterFont::Primary(primary), RasterFont::Fallback)
        } else {
            RasterFont::Primary(primary)
        }
    }
    fn glyph(&mut self, c: char, size: f32, weight: u16) -> Glyph {
        let size_key = (size * 2.0).round().clamp(1.0, 2048.0) as u16;
        let key = (c, size_key, weight);
        if let Some(g) = self.atlas.glyphs.get(&key) {
            return *g;
        }
        let (m, bitmap) = self.font(c, weight).rasterize(c, size_key as f32 * 0.5);
        if self.atlas.x + m.width + 1 >= 2048 {
            self.atlas.x = 1;
            self.atlas.y += self.atlas.row_height + 1;
            self.atlas.row_height = 0;
        }
        if self.atlas.y + m.height + 1 >= 2048 {
            // Submit vertices that still reference this atlas before reusing
            // its bounded storage. Otherwise a large multilingual library
            // eventually renders every newly encountered character blank.
            self.finish();
            self.vertices.clear();
            self.batches.clear();
            self.atlas.glyphs.clear();
            self.atlas.pixels.fill(0);
            self.atlas.x = 1;
            self.atlas.y = 1;
            self.atlas.row_height = 0;
            self.atlas.dirty = Some([0, 0, 2048, 2048]);
        }
        let x = self.atlas.x;
        let y = self.atlas.y;
        for by in 0..m.height {
            for bx in 0..m.width {
                let i = ((y + by) * 2048 + x + bx) * 4;
                self.atlas.pixels[i..i + 4].copy_from_slice(&[
                    255,
                    255,
                    255,
                    bitmap[by * m.width + bx],
                ]);
            }
        }
        let g = Glyph {
            uv: [
                x as f32 / 2048.0,
                y as f32 / 2048.0,
                (x + m.width) as f32 / 2048.0,
                (y + m.height) as f32 / 2048.0,
            ],
            width: m.width as f32,
            height: m.height as f32,
            left: m.xmin as f32,
            top: (m.height as i32 + m.ymin) as f32,
        };
        self.atlas.x += m.width + 1;
        self.atlas.row_height = self.atlas.row_height.max(m.height);
        self.atlas.dirty = Some(match self.atlas.dirty {
            Some([x0, y0, x1, y1]) => [
                x0.min(x),
                y0.min(y),
                x1.max(x + m.width),
                y1.max(y + m.height),
            ],
            None => [x, y, x + m.width, y + m.height],
        });
        self.atlas.glyphs.insert(key, g);
        g
    }
    fn advance(&mut self, c: char, size: f32, weight: u16) -> f32 {
        let size_key = (size * 2.0).round().clamp(1.0, 2048.0) as u16;
        let key = (c, size_key, weight);
        if let Some(advance) = self.atlas.advances.get(&key) {
            return *advance;
        }
        let advance = crate::font_metrics::fixed(
            self.font(c, weight)
                .metrics(c, size_key as f32 * 0.5)
                .advance_width,
        );
        if self.atlas.advances.len() < 65536 {
            self.atlas.advances.insert(key, advance);
        }
        advance
    }
    fn kern(&mut self, left: char, right: char, size: f32, weight: u16) -> f32 {
        let weight = if self.atlas.fonts.contains_key(&weight) {
            weight
        } else {
            400
        };
        let font = &self.atlas.fonts[&weight];
        let a = font.lookup_glyph_index(left);
        let b = font.lookup_glyph_index(right);
        if a != 0 && b != 0 {
            self.atlas
                .metrics
                .get_mut(&weight)
                .map_or(0.0, |m| m.kern(a, b, (size * 2.0).round() * 0.5))
        } else {
            0.0
        }
    }
    pub fn text_bounds_weighted(&self, text: &str, size: f32, weight: u16) -> (f32, f32) {
        let mut top = f32::INFINITY;
        let mut bottom = f32::NEG_INFINITY;
        for c in text.chars().take(4096) {
            let m = self.font(c, weight).metrics(c, (size * 2.0).round() * 0.5);
            if m.width == 0 || m.height == 0 {
                continue;
            }
            top = top.min(-(m.height as f32 + m.ymin as f32));
            bottom = bottom.max(-(m.ymin as f32));
        }
        if top.is_finite() {
            (top, bottom)
        } else {
            (0.0, 0.0)
        }
    }
    pub fn text_width_weighted(&mut self, text: &str, size: f32, weight: u16) -> f32 {
        if crate::inline_keys::has_tokens(text) {
            return crate::inline_keys::width(self, text, size, weight);
        }
        let mut width = 0.0;
        let mut previous = None;
        for c in text.chars().take(4096) {
            if let Some(p) = previous {
                width += self.kern(p, c, size, weight);
            }
            width += self.advance(c, size, weight);
            previous = Some(c);
        }
        width
    }
    pub fn text_width(&mut self, text: &str, size: u16) -> f32 {
        self.text_width_weighted(text, size as f32, 400)
    }
    pub fn text_baseline(
        &mut self,
        text: &str,
        x: f32,
        baseline: f32,
        size: f32,
        weight: u16,
        color: Color,
        max_width: f32,
    ) {
        if crate::inline_keys::has_tokens(text) {
            crate::inline_keys::draw(self, text, x, baseline, size, weight, color, max_width);
            return;
        }
        let mut cursor = x;
        let mut previous = None;
        for c in text.chars().take(4096) {
            if let Some(p) = previous {
                cursor += self.kern(p, c, size, weight);
            }
            let advance = self.advance(c, size, weight);
            if cursor + advance > x + max_width {
                break;
            }
            let g = self.glyph(c, size, weight);
            let xx = cursor + g.left;
            let yy = baseline - g.top;
            self.quad(
                self.atlas.texture,
                [
                    [xx, yy, 1.0],
                    [xx + g.width, yy, 1.0],
                    [xx + g.width, yy + g.height, 1.0],
                    [xx, yy + g.height, 1.0],
                ],
                g.uv,
                [color; 4],
            );
            cursor += advance;
            previous = Some(c);
        }
    }
    pub fn text_weighted(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        size: f32,
        weight: u16,
        color: Color,
        max_width: f32,
    ) {
        let (_, baseline) = line_box(size);
        self.text_baseline(text, x, y + baseline, size, weight, color, max_width);
    }
    pub fn text(&mut self, text: &str, x: f32, y: f32, size: u16, color: Color, max_width: f32) {
        self.text_baseline(text, x, y + size as f32, size as f32, 400, color, max_width);
    }
    pub fn centered(&mut self, text: &str, x: f32, y: f32, w: f32, size: u16, color: Color) {
        let width = self.text_width(text, size).min(w);
        self.text(text, x + (w - width) * 0.5, y, size, color, w);
    }
    pub fn push_transform(&mut self, tx: f32, ty: f32, sx: f32, sy: f32, opacity: f32) {
        self.push_projective([[sx, 0.0, tx], [0.0, sy, ty], [0.0, 0.0, 1.0]], opacity);
    }
    pub fn push_projective(&mut self, matrix: [[f32; 3]; 3], opacity: f32) {
        self.transforms
            .push((self.transform, self.opacity, self.clip));
        self.transform = multiply(self.transform, matrix);
        self.opacity *= opacity;
    }
    pub fn pop_transform(&mut self) {
        if let Some((transform, opacity, clip)) = self.transforms.pop() {
            self.transform = transform;
            self.opacity = opacity;
            self.clip = clip;
        }
    }
    pub fn image_uv(&mut self, texture: Texture, rect: [f32; 4], uv: [f32; 4], opacity: f32) {
        self.image_tinted(texture, rect, uv, [1.0, 1.0, 1.0, opacity]);
    }
    pub fn image_tinted(&mut self, texture: Texture, rect: [f32; 4], uv: [f32; 4], color: Color) {
        let [x, y, w, h] = rect;
        self.quad(
            texture,
            [
                [x, y, 1.0],
                [x + w, y, 1.0],
                [x + w, y + h, 1.0],
                [x, y + h, 1.0],
            ],
            uv,
            [color; 4],
        );
    }
    pub fn projected_image(
        &mut self,
        texture: Texture,
        points: [[f32; 3]; 4],
        uv: [f32; 4],
        colors: [Color; 4],
    ) {
        self.quad(texture, points, uv, colors);
    }
    pub fn gradient_stops(&mut self, x: f32, y: f32, w: f32, h: f32, stops: &[(f32, Color)]) {
        for pair in stops.windows(2) {
            let [(a, top), (b, bottom)] = pair else {
                continue;
            };
            self.gradient(x, y + h * a, w, h * (b - a), *top, *bottom);
        }
    }
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        self.rounded_gradient(x, y, w, h, radius, &[(0.0, color), (1.0, color)]);
    }
    pub fn rounded_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        stops: &[(f32, Color)],
    ) {
        if w <= 0.0 || h <= 0.0 || stops.is_empty() {
            return;
        }
        let radius = radius.clamp(0.0, w.min(h) * 0.5);
        if radius <= 0.5 {
            self.gradient_stops(x, y, w, h, stops);
            return;
        }
        let inner = rounded_outline(x + 0.5, y + 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0));
        for pair in stops.windows(2) {
            let y0 = y + h * pair[0].0;
            let y1 = y + h * pair[1].0;
            let polygon = clip_y(&clip_y(&inner, y0, true), y1, false);
            if polygon.len() < 3 {
                continue;
            }
            for i in 1..polygon.len() - 1 {
                let points = [polygon[0], polygon[i], polygon[i + 1]];
                self.colored_triangle(
                    points,
                    points.map(|p| gradient_color(stops, (p[1] - y) / h)),
                );
            }
        }
        let outer = rounded_outline(x - 0.5, y - 0.5, w + 1.0, h + 1.0, radius + 0.5);
        for i in 0..inner.len() {
            let j = (i + 1) % inner.len();
            let a = gradient_color(stops, (inner[i][1] - y) / h);
            let b = gradient_color(stops, (inner[j][1] - y) / h);
            let mut c = gradient_color(stops, (outer[j][1] - y) / h);
            c[3] = 0.0;
            let mut d = gradient_color(stops, (outer[i][1] - y) / h);
            d[3] = 0.0;
            self.colored_triangle([inner[i], inner[j], outer[j]], [a, b, c]);
            self.colored_triangle([inner[i], outer[j], outer[i]], [a, c, d]);
        }
    }
    /// A centered antialiased outline. Inset x/y by half the line width when
    /// the original drawing contract specifies an entirely inside border.
    pub fn stroke_rounded(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        line: f32,
        color: Color,
    ) {
        let half = line * 0.5;
        let rings = [
            rounded_outline(
                x - half - 0.5,
                y - half - 0.5,
                w + line + 1.0,
                h + line + 1.0,
                radius + half + 0.5,
            ),
            rounded_outline(
                x - half + 0.5,
                y - half + 0.5,
                w + line - 1.0,
                h + line - 1.0,
                (radius + half - 0.5).max(0.0),
            ),
            rounded_outline(
                x + half - 0.5,
                y + half - 0.5,
                w - line + 1.0,
                h - line + 1.0,
                (radius - half + 0.5).max(0.0),
            ),
            rounded_outline(
                x + half + 0.5,
                y + half + 0.5,
                w - line - 1.0,
                h - line - 1.0,
                (radius - half - 0.5).max(0.0),
            ),
        ];
        for band in 0..3 {
            for i in 0..rings[0].len() {
                let j = (i + 1) % rings[0].len();
                let mut a = color;
                let mut b = color;
                if band == 0 {
                    a[3] = 0.0
                }
                if band == 2 {
                    b[3] = 0.0
                }
                self.colored_triangle(
                    [rings[band][i], rings[band][j], rings[band + 1][j]],
                    [a, a, b],
                );
                self.colored_triangle(
                    [rings[band][i], rings[band + 1][j], rings[band + 1][i]],
                    [a, b, b],
                );
            }
        }
    }
    pub fn finish(&mut self) {
        unsafe {
            if let Some([x0, y0, x1, y1]) = self.atlas.dirty.take() {
                self.atlas.upload.clear();
                for row in y0..y1 {
                    let start = (row * 2048 + x0) * 4;
                    self.atlas
                        .upload
                        .extend_from_slice(&self.atlas.pixels[start..start + (x1 - x0) * 4]);
                }
                if x1 > x0 && y1 > y0 {
                    self.gl
                        .bind_texture(glow::TEXTURE_2D, Some(self.atlas.texture.id));
                    self.gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        x0 as i32,
                        y0 as i32,
                        (x1 - x0) as i32,
                        (y1 - y0) as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&self.atlas.upload)),
                    );
                }
            }
            self.gl.use_program(Some(self.program));
            if let Some(vao) = self.vao {
                self.gl.bind_vertex_array(Some(vao));
            }
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
            let bytes = std::slice::from_raw_parts(
                self.vertices.as_ptr().cast::<u8>(),
                self.vertices.len() * 4,
            );
            self.gl
                .buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
            for (a, n, offset) in [(self.position, 3, 0), (self.uv, 2, 12), (self.color, 4, 20)] {
                self.gl.enable_vertex_attrib_array(a);
                self.gl
                    .vertex_attrib_pointer_f32(a, n, glow::FLOAT, false, 36, offset);
            }
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.uniform_2_f32(
                self.gl
                    .get_uniform_location(self.program, "u_viewport")
                    .as_ref(),
                self.width as f32,
                self.height as f32,
            );
            let count_location = self.gl.get_uniform_location(self.program, "u_clip_count");
            let edges_location = self
                .gl
                .get_uniform_location(self.program, "u_clip_edges[0]");
            for batch in &self.batches {
                let [x, y, w, h] = batch.clip.rect;
                let sx = self.width as f32 / 640.0;
                let sy = self.height as f32 / 480.0;
                self.gl.scissor(
                    (x as f32 * sx) as i32,
                    ((480 - y - h) as f32 * sy) as i32,
                    (w as f32 * sx).ceil() as i32,
                    (h as f32 * sy).ceil() as i32,
                );
                self.gl
                    .uniform_1_i32(count_location.as_ref(), batch.clip.count as i32);
                if batch.clip.count > 0 {
                    let edges = std::slice::from_raw_parts(
                        batch.clip.edges.as_ptr().cast::<f32>(),
                        batch.clip.count * 3,
                    );
                    self.gl.uniform_3_f32_slice(edges_location.as_ref(), edges);
                }
                self.gl.bind_texture(glow::TEXTURE_2D, Some(batch.texture));
                self.gl
                    .draw_arrays(glow::TRIANGLES, batch.start, batch.count);
            }
        }
    }
    pub fn screenshot(&self, path: &Path) -> Result<(), String> {
        self.capture_rgba().save(path).map_err(|e| e.to_string())
    }
    /// Only used by explicit screenshots and opt-in remote diagnostics.
    pub fn capture_rgba(&self) -> image::RgbaImage {
        let mut pixels = vec![0u8; self.width as usize * self.height as usize * 4];
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                self.width,
                self.height,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
        }
        let image = image::RgbaImage::from_raw(self.width as u32, self.height as u32, pixels)
            .expect("Framebuffer dimensions");
        image::imageops::flip_vertical(&image)
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        let mut errors = Vec::new();
        unsafe {
            for _ in 0..8 {
                let e = self.gl.get_error();
                if e == glow::NO_ERROR {
                    break;
                }
                errors.push(e);
            }
        }
        serde_json::json!({"vertices":self.vertices.len()/9,"batches":self.batches.len(),"transforms":self.transforms.len(),"clips":self.clips.len(),"gl_errors":errors,
            "fallback_font_bytes":self.atlas.fallback.iter().flatten().map(OutlineFont::source_bytes).sum::<usize>(),
            "fallback_cached_metrics":self.atlas.fallback.iter().flatten().map(OutlineFont::cached_metrics).sum::<usize>()})
    }
}

fn upload(gl: &glow::Context, width: u32, height: u32, pixels: &[u8]) -> Result<Texture, String> {
    if pixels.len() != width as usize * height as usize * 4 {
        return Err("Invalid RGBA texture size".into());
    }
    unsafe {
        let id = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(id));
        for (key, value) in [
            (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, key, value as i32);
        }
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(pixels)),
        );
        Ok(Texture { id, width, height })
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            for texture in self.retired.get_mut().drain(..) {
                self.gl.delete_texture(texture.id);
            }
            self.gl.delete_texture(self.white.id);
            self.gl.delete_texture(self.atlas.texture.id);
            self.gl.delete_buffer(self.buffer);
            self.gl.delete_program(self.program);
            if let Some(vao) = self.vao {
                self.gl.delete_vertex_array(vao);
            }
        }
    }
}

// Go 1.1's CSS line box, expressed in physical pixels at DPR 2.
pub fn line_box(size: f32) -> (f32, f32) {
    let css = size * 0.5;
    let baseline =
        (css * 0.96875 * 2.0).round() - if (css - 14.0).abs() < 0.001 { 1.0 } else { 0.0 };
    (
        2.0 * ((css * 0.96875).round() + (css * 0.2412109375).round()),
        baseline,
    )
}
fn identity() -> [[f32; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}
fn multiply(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            for k in 0..3 {
                out[row][col] += a[row][k] * b[k][col];
            }
        }
    }
    out
}
fn transform_point(m: [[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    let [x, y, w] = p;
    let d = m[2][0] * x + m[2][1] * y + m[2][2];
    [
        (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
        (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
        w * d,
    ]
}

fn rounded_outline(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Vec<[f32; 2]> {
    let radius = radius.max(0.0).min(w.max(0.0).min(h.max(0.0)) * 0.5);
    let mut points = Vec::with_capacity(52);
    for (cx, cy, start) in [
        (x + w - radius, y + radius, -std::f32::consts::FRAC_PI_2),
        (x + w - radius, y + h - radius, 0.0),
        (x + radius, y + h - radius, std::f32::consts::FRAC_PI_2),
        (x + radius, y + radius, std::f32::consts::PI),
    ] {
        for i in 0..=12 {
            let a = start + i as f32 * std::f32::consts::FRAC_PI_2 / 12.0;
            points.push([cx + radius * a.cos(), cy + radius * a.sin()]);
        }
    }
    points
}
fn clip_y(points: &[[f32; 2]], y: f32, above: bool) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(points.len() + 2);
    if points.is_empty() {
        return out;
    }
    let inside = |p: [f32; 2]| if above { p[1] >= y } else { p[1] <= y };
    let mut a = *points.last().unwrap();
    let mut ai = inside(a);
    for &b in points {
        let bi = inside(b);
        if ai != bi {
            let t = (y - a[1]) / (b[1] - a[1]);
            out.push([a[0] + (b[0] - a[0]) * t, y]);
        }
        if bi {
            out.push(b)
        }
        a = b;
        ai = bi;
    }
    out
}
fn gradient_color(stops: &[(f32, Color)], p: f32) -> Color {
    let p = p.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        if p <= pair[1].0 {
            let t = ((p - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0.0, 1.0);
            return std::array::from_fn(|i| pair[0].1[i] + (pair[1].1[i] - pair[0].1[i]) * t);
        }
    }
    stops.last().map(|s| s.1).unwrap_or(WHITE)
}
