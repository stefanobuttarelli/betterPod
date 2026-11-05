//! GPU composition. Rust prepares small vertex batches; the GPU rasterizes
//! gradients, text and perspective covers. No full-frame CPU pixel buffer.
use glow::HasContext;
use std::collections::HashMap;
use std::path::Path;

pub type Color = [f32; 4];
pub const WHITE: Color = [1.0; 4];
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

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
    glyphs: HashMap<(char, u16), Glyph>,
    advances: HashMap<(char, u16), f32>,
    font: fontdue::Font,
    fallback: Option<fontdue::Font>,
    pixels: Vec<u8>,
    x: usize,
    y: usize,
    row_height: usize,
    dirty: Option<[usize; 4]>,
    upload: Vec<u8>,
}
#[derive(Clone, Copy)]
struct Batch {
    texture: glow::NativeTexture,
    start: i32,
    count: i32,
    clip: [i32; 4],
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
    clip: [i32; 4],
    position: u32,
    uv: u32,
    color: u32,
    width: i32,
    height: i32,
    pub backend: String,
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
            "attribute vec3 a_position; attribute vec2 a_uv; attribute vec4 a_color; varying vec2 v_uv; varying vec4 v_color; void main(){gl_Position=vec4(a_position.xy*a_position.z,0.0,a_position.z);v_uv=a_uv;v_color=a_color;}"
        };
        let frag = if desktop {
            "uniform sampler2D u_texture; in vec2 v_uv; in vec4 v_color; out vec4 out_color; void main(){out_color=texture(u_texture,v_uv)*v_color;}"
        } else {
            "uniform sampler2D u_texture; varying vec2 v_uv; varying vec4 v_color; void main(){gl_FragColor=texture2D(u_texture,v_uv)*v_color;}"
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
        let font = fontdue::Font::from_bytes(
            std::fs::read(primary).map_err(|e| format!("{}: {e}", primary.display()))?,
            fontdue::FontSettings::default(),
        )
        .map_err(|e| e.to_string())?;
        let jp = font_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("fonts/NotoSansJP-Regular.otf");
        let fallback = std::fs::read(&jp).ok().and_then(|data| {
            fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).ok()
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
                font,
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
            clip: [0, 0, 640, 480],
            position,
            uv,
            color,
            width,
            height,
            backend,
        };
        for size in [16, 18, 20, 22, 24, 26, 28, 32, 36] {
            for c in (32..=255).filter_map(char::from_u32) {
                out.glyph(c, size);
            }
        }
        for c in "0123456789-+.°CF".chars() {
            out.glyph(c, 48);
        }
        Ok(out)
    }

    pub fn upload(&self, width: u32, height: u32, pixels: &[u8]) -> Result<Texture, String> {
        upload(&self.gl, width, height, pixels)
    }
    pub fn delete_texture(&self, texture: Texture) {
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
        self.clip = [0, 0, 640, 480];
        unsafe {
            self.gl.viewport(0, 0, self.width, self.height);
            self.gl.scissor(0, 0, self.width, self.height);
            self.gl.clear_color(bg[0], bg[1], bg[2], bg[3]);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }
    pub fn set_clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let x0 = x.max(0.0).min(640.0);
        let y0 = y.max(0.0).min(480.0);
        let x1 = (x + width).max(x0).min(640.0);
        let y1 = (y + height).max(y0).min(480.0);
        self.clip = [x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32];
    }
    pub fn reset_clip(&mut self) {
        self.clip = [0, 0, 640, 480];
    }
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.gradient(x, y, w, h, color, color);
    }
    pub fn triangle(&mut self, p: [[f32; 2]; 3], color: Color) {
        let start = (self.vertices.len() / 9) as i32;
        for point in p {
            self.vertices.extend_from_slice(&[
                point[0] / 320.0 - 1.0,
                1.0 - point[1] / 240.0,
                1.0,
                0.5,
                0.5,
                color[0],
                color[1],
                color[2],
                color[3],
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
        if self.clip[2] <= 0 || self.clip[3] <= 0 {
            return;
        }
        let start = (self.vertices.len() / 9) as i32;
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
                colors[i][3],
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
    fn glyph(&mut self, c: char, size: u16) -> Glyph {
        if let Some(g) = self.atlas.glyphs.get(&(c, size)) {
            return *g;
        }
        let font = if self.atlas.font.lookup_glyph_index(c) == 0 {
            self.atlas
                .fallback
                .as_ref()
                .filter(|font| font.lookup_glyph_index(c) != 0)
                .unwrap_or(&self.atlas.font)
        } else {
            &self.atlas.font
        };
        let (m, bitmap) = font.rasterize(c, size as f32);
        if self.atlas.x + m.width + 1 >= 2048 {
            self.atlas.x = 1;
            self.atlas.y += self.atlas.row_height + 1;
            self.atlas.row_height = 0;
        }
        if self.atlas.y + m.height + 1 >= 2048 {
            let fallback = *self.atlas.glyphs.get(&('?', size)).unwrap_or(&Glyph {
                uv: [0.0; 4],
                width: 0.0,
                height: 0.0,
                left: 0.0,
                top: 0.0,
            });
            self.atlas.glyphs.insert((c, size), fallback);
            return fallback;
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
        self.atlas.glyphs.insert((c, size), g);
        g
    }
    fn advance(&mut self, c: char, size: u16) -> f32 {
        if let Some(advance) = self.atlas.advances.get(&(c, size)) {
            return *advance;
        }
        let font = if self.atlas.font.lookup_glyph_index(c) == 0 {
            self.atlas
                .fallback
                .as_ref()
                .filter(|f| f.lookup_glyph_index(c) != 0)
                .unwrap_or(&self.atlas.font)
        } else {
            &self.atlas.font
        };
        let advance = font.metrics(c, size as f32).advance_width;
        if self.atlas.advances.len() < 65536 {
            self.atlas.advances.insert((c, size), advance);
        }
        advance
    }
    pub fn text_width(&mut self, text: &str, size: u16) -> f32 {
        // Measuring a long marquee must not rasterize every offscreen glyph.
        text.chars().take(4096).map(|c| self.advance(c, size)).sum()
    }
    pub fn text(&mut self, text: &str, x: f32, y: f32, size: u16, color: Color, max_width: f32) {
        let mut cursor = x;
        let right = (x + max_width).min((self.clip[0] + self.clip[2]) as f32);
        let left = self.clip[0] as f32;
        if y >= (self.clip[1] + self.clip[3]) as f32 || y + size as f32 + 8. < self.clip[1] as f32 {
            return;
        }
        for c in text.chars().take(4096) {
            let advance = self.advance(c, size);
            if cursor + advance > x + max_width || cursor >= right {
                break;
            }
            if cursor + advance < left {
                cursor += advance;
                continue;
            }
            let g = self.glyph(c, size);
            let xx = cursor + g.left;
            let yy = y + size as f32 - g.top;
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
        }
    }
    pub fn centered(&mut self, text: &str, x: f32, y: f32, w: f32, size: u16, color: Color) {
        let width = self.text_width(text, size).min(w);
        self.text(text, x + (w - width) * 0.5, y, size, color, w);
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
            for batch in &self.batches {
                let [x, y, w, h] = batch.clip;
                let sx = self.width as f32 / 640.0;
                let sy = self.height as f32 / 480.0;
                self.gl.scissor(
                    (x as f32 * sx) as i32,
                    ((480 - y - h) as f32 * sy) as i32,
                    (w as f32 * sx).ceil() as i32,
                    (h as f32 * sy).ceil() as i32,
                );
                self.gl.bind_texture(glow::TEXTURE_2D, Some(batch.texture));
                self.gl
                    .draw_arrays(glow::TRIANGLES, batch.start, batch.count);
            }
        }
    }
    pub fn screenshot(&self, path: &Path) -> Result<(), String> {
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
            .ok_or("Invalid screenshot size")?;
        image::imageops::flip_vertical(&image)
            .save(path)
            .map_err(|e| e.to_string())
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
