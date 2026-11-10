//! Rust frontend for Simon Tatham's unchanged MIT-licensed puzzle engines.
//! No Go code and no custom C frontend are linked. Drawing, input, loading,
//! save callbacks and geometry replay are implemented here in Rust.
use crate::input::Action;
use crate::render::{Color, Renderer};
use libc::{c_char, c_float, c_int, c_void};
use std::ffi::CStr;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct PuzzleInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub numeric: bool,
}
pub const PUZZLES: [PuzzleInfo; 12] = [
    PuzzleInfo {
        id: "mines",
        name: "Mines",
        description: "Find the mines: numbers count those around",
        numeric: false,
    },
    PuzzleInfo {
        id: "solo",
        name: "Sudoku",
        description: "Every row, column and box holds each digit once",
        numeric: true,
    },
    PuzzleInfo {
        id: "net",
        name: "Net",
        description: "Turn the tiles to connect the whole network",
        numeric: false,
    },
    PuzzleInfo {
        id: "flood",
        name: "Flood",
        description: "Fill the board with one colour in few moves",
        numeric: false,
    },
    PuzzleInfo {
        id: "samegame",
        name: "Same Game",
        description: "Clear groups of the same colour",
        numeric: false,
    },
    PuzzleInfo {
        id: "fifteen",
        name: "Fifteen",
        description: "Slide the tiles into order",
        numeric: false,
    },
    PuzzleInfo {
        id: "lightup",
        name: "Light Up",
        description: "Light every square with bulbs that don't see each other",
        numeric: false,
    },
    PuzzleInfo {
        id: "unruly",
        name: "Unruly",
        description: "Black and white: no three in a row, as many of each",
        numeric: false,
    },
    PuzzleInfo {
        id: "towers",
        name: "Towers",
        description: "Place the towers: the clues count those you can see",
        numeric: true,
    },
    PuzzleInfo {
        id: "keen",
        name: "Keen",
        description: "A Latin square whose cages add, multiply…",
        numeric: true,
    },
    PuzzleInfo {
        id: "pattern",
        name: "Pattern",
        description: "Fill the squares the clues describe (nonogram)",
        numeric: false,
    },
    PuzzleInfo {
        id: "tents",
        name: "Tents",
        description: "A tent next to each tree, never touching",
        numeric: false,
    },
];

#[repr(C)]
struct Midend {
    _private: [u8; 0],
}
#[repr(C)]
struct Game {
    _private: [u8; 0],
}
#[repr(C)]
struct Params {
    _private: [u8; 0],
}
#[repr(C)]
struct DrawingApi {
    text: Option<
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int, c_int, *mut c_char),
    >,
    rect: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int)>,
    line: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int)>,
    polygon: Option<unsafe extern "C" fn(*mut c_void, *mut c_int, c_int, c_int, c_int)>,
    circle: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int, c_int)>,
    update: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int)>,
    clip: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int)>,
    unclip: Option<unsafe extern "C" fn(*mut c_void)>,
    start: Option<unsafe extern "C" fn(*mut c_void)>,
    end: Option<unsafe extern "C" fn(*mut c_void)>,
    status: Option<unsafe extern "C" fn(*mut c_void, *mut c_char)>,
    blitter_new: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int) -> *mut Blitter>,
    blitter_free: Option<unsafe extern "C" fn(*mut c_void, *mut Blitter)>,
    blitter_save: Option<unsafe extern "C" fn(*mut c_void, *mut Blitter, c_int, c_int)>,
    blitter_load: Option<unsafe extern "C" fn(*mut c_void, *mut Blitter, c_int, c_int)>,
    begin_doc: Option<unsafe extern "C" fn()>,
    begin_page: Option<unsafe extern "C" fn()>,
    begin_puzzle: Option<unsafe extern "C" fn()>,
    end_puzzle: Option<unsafe extern "C" fn()>,
    end_page: Option<unsafe extern "C" fn()>,
    end_doc: Option<unsafe extern "C" fn()>,
    line_width: Option<unsafe extern "C" fn()>,
    line_dotted: Option<unsafe extern "C" fn()>,
    text_fallback:
        Option<unsafe extern "C" fn(*mut c_void, *const *const c_char, c_int) -> *mut c_char>,
    thick_line: Option<
        unsafe extern "C" fn(*mut c_void, c_float, c_float, c_float, c_float, c_float, c_int),
    >,
}
extern "C" {
    static mines: Game;
    static solo: Game;
    static net: Game;
    static flood: Game;
    static samegame: Game;
    static fifteen: Game;
    static lightup: Game;
    static unruly: Game;
    static towers: Game;
    static keen: Game;
    static pattern: Game;
    static tents: Game;
    fn midend_new(
        frontend: *mut c_void,
        game: *const Game,
        api: *const DrawingApi,
        handle: *mut c_void,
    ) -> *mut Midend;
    fn midend_free(midend: *mut Midend);
    fn midend_new_game(midend: *mut Midend);
    fn midend_restart_game(midend: *mut Midend);
    fn midend_size(midend: *mut Midend, w: *mut c_int, h: *mut c_int, user_size: c_int);
    fn midend_process_key(midend: *mut Midend, x: c_int, y: c_int, button: c_int) -> c_int;
    #[cfg(test)]
    fn midend_game_id(midend: *mut Midend, id: *mut c_char) -> *mut c_char;
    #[cfg(test)]
    fn midend_get_random_seed(midend: *mut Midend) -> *mut c_char;
    fn midend_force_redraw(midend: *mut Midend);
    fn midend_redraw(midend: *mut Midend);
    fn midend_colours(midend: *mut Midend, count: *mut c_int) -> *mut c_float;
    fn midend_timer(midend: *mut Midend, dt: c_float);
    fn midend_status(midend: *mut Midend) -> c_int;
    fn midend_solve(midend: *mut Midend) -> *mut c_char;
    fn midend_num_presets(midend: *mut Midend) -> c_int;
    fn midend_which_preset(midend: *mut Midend) -> c_int;
    fn midend_fetch_preset(
        midend: *mut Midend,
        index: c_int,
        name: *mut *mut c_char,
        params: *mut *mut Params,
    );
    fn midend_set_params(midend: *mut Midend, params: *mut Params);
    fn midend_serialise(
        midend: *mut Midend,
        write: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int),
        ctx: *mut c_void,
    );
    fn midend_deserialise(
        midend: *mut Midend,
        read: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> c_int,
        ctx: *mut c_void,
    ) -> *mut c_char;
}
const UP: c_int = 0x209;
const DOWN: c_int = 0x20a;
const LEFT: c_int = 0x20b;
const RIGHT: c_int = 0x20c;
const SELECT: c_int = 0x20d;
const SELECT2: c_int = 0x20e;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}
impl Rect {
    fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            w: (self.x + self.w).min(other.x + other.w).max(x) - x,
            h: (self.y + self.h).min(other.y + other.h).max(y) - y,
        }
    }
    fn shifted(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }
}
#[derive(Clone)]
enum Shape {
    Text {
        x: f32,
        y: f32,
        size: u16,
        align: i32,
        colour: i32,
        text: Arc<str>,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        colour: i32,
    },
    Line {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        colour: i32,
    },
    Polygon {
        points: Arc<[[f32; 2]]>,
        triangles: Arc<[[[f32; 2]; 3]]>,
        fill: i32,
        outline: i32,
    },
    Circle {
        x: f32,
        y: f32,
        r: f32,
        fill: i32,
        outline: i32,
        points: Arc<[[f32; 2]]>,
    },
}
#[derive(Clone)]
struct Primitive {
    shape: Shape,
    clip: Rect,
    dx: f32,
    dy: f32,
}
struct Frontend {
    commands: Vec<Primitive>,
    clip: Rect,
    bounds: Rect,
    bg: [f32; 3],
    status: String,
    timer: bool,
}
impl Frontend {
    fn push(&mut self, shape: Shape) {
        if self.clip.w > 0.0 && self.clip.h > 0.0 {
            self.commands.push(Primitive {
                shape,
                clip: self.clip,
                dx: 0.0,
                dy: 0.0,
            });
        }
    }
}
struct Blitter {
    width: f32,
    height: f32,
    x: f32,
    y: f32,
    commands: Vec<Primitive>,
}
unsafe fn frontend<'a>(handle: *mut c_void) -> &'a mut Frontend {
    &mut *(handle as *mut Frontend)
}
unsafe extern "C" fn d_text(
    h: *mut c_void,
    x: c_int,
    y: c_int,
    _font: c_int,
    size: c_int,
    align: c_int,
    colour: c_int,
    text: *mut c_char,
) {
    if text.is_null() {
        return;
    }
    frontend(h).push(Shape::Text {
        x: x as f32,
        y: y as f32,
        size: size.clamp(6, 96) as u16,
        align,
        colour,
        text: Arc::from(CStr::from_ptr(text).to_string_lossy().as_ref()),
    });
}
unsafe extern "C" fn d_rect(
    h: *mut c_void,
    x: c_int,
    y: c_int,
    w: c_int,
    hh: c_int,
    colour: c_int,
) {
    frontend(h).push(Shape::Rect {
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: hh as f32,
        colour,
    });
}
unsafe extern "C" fn d_line(
    h: *mut c_void,
    x0: c_int,
    y0: c_int,
    x1: c_int,
    y1: c_int,
    colour: c_int,
) {
    frontend(h).push(Shape::Line {
        x0: x0 as f32 + 0.5,
        y0: y0 as f32 + 0.5,
        x1: x1 as f32 + 0.5,
        y1: y1 as f32 + 0.5,
        width: 1.2,
        colour,
    });
}
unsafe extern "C" fn d_polygon(
    h: *mut c_void,
    xy: *mut c_int,
    n: c_int,
    fill: c_int,
    outline: c_int,
) {
    if xy.is_null() || !(3..=10000).contains(&n) {
        return;
    }
    let coords = std::slice::from_raw_parts(xy, n as usize * 2);
    let points: Vec<[f32; 2]> = coords
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| [p[0] as f32 + 0.5, p[1] as f32 + 0.5])
        .collect();
    frontend(h).push(Shape::Polygon {
        triangles: triangulate(&points).into(),
        points: points.into(),
        fill,
        outline,
    });
}
unsafe extern "C" fn d_circle(
    h: *mut c_void,
    x: c_int,
    y: c_int,
    r: c_int,
    fill: c_int,
    outline: c_int,
) {
    let points = circle_points(x as f32 + 0.5, y as f32 + 0.5, r as f32);
    frontend(h).push(Shape::Circle {
        x: x as f32 + 0.5,
        y: y as f32 + 0.5,
        r: r as f32,
        fill,
        outline,
        points: points.into(),
    });
}
unsafe extern "C" fn d_update(_h: *mut c_void, _x: c_int, _y: c_int, _w: c_int, _hh: c_int) {}
unsafe extern "C" fn d_clip(h: *mut c_void, x: c_int, y: c_int, w: c_int, hh: c_int) {
    let f = frontend(h);
    f.clip = Rect {
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: hh as f32,
    }
    .intersect(f.bounds);
}
unsafe extern "C" fn d_unclip(h: *mut c_void) {
    let f = frontend(h);
    f.clip = f.bounds;
}
unsafe extern "C" fn d_noop(_h: *mut c_void) {}
unsafe extern "C" fn d_status(h: *mut c_void, text: *mut c_char) {
    if !text.is_null() {
        frontend(h).status = CStr::from_ptr(text).to_string_lossy().into_owned();
    }
}
unsafe extern "C" fn d_blitter_new(_h: *mut c_void, w: c_int, h: c_int) -> *mut Blitter {
    Box::into_raw(Box::new(Blitter {
        width: w as f32,
        height: h as f32,
        x: 0.0,
        y: 0.0,
        commands: Vec::new(),
    }))
}
unsafe extern "C" fn d_blitter_free(_h: *mut c_void, blitter: *mut Blitter) {
    if !blitter.is_null() {
        drop(Box::from_raw(blitter));
    }
}
unsafe extern "C" fn d_blitter_save(h: *mut c_void, b: *mut Blitter, x: c_int, y: c_int) {
    if b.is_null() {
        return;
    }
    let b = &mut *b;
    b.x = x as f32;
    b.y = y as f32;
    b.commands.clear();
    let bounds = Rect {
        x: b.x,
        y: b.y,
        w: b.width,
        h: b.height,
    };
    for p in &frontend(h).commands {
        let mut p = p.clone();
        p.clip = p.clip.intersect(bounds);
        if p.clip.w > 0.0 && p.clip.h > 0.0 {
            b.commands.push(p);
        }
    }
}
unsafe extern "C" fn d_blitter_load(h: *mut c_void, b: *mut Blitter, x: c_int, y: c_int) {
    if b.is_null() {
        return;
    }
    let b = &*b;
    let f = frontend(h);
    let (x, y) = if x == -1 && y == -1 {
        (b.x, b.y)
    } else {
        (x as f32, y as f32)
    };
    let dx = x - b.x;
    let dy = y - b.y;
    for p in &b.commands {
        let mut p = p.clone();
        p.dx += dx;
        p.dy += dy;
        p.clip = p.clip.shifted(dx, dy).intersect(f.clip);
        if p.clip.w > 0.0 && p.clip.h > 0.0 {
            f.commands.push(p);
        }
    }
}
unsafe extern "C" fn d_fallback(
    _h: *mut c_void,
    strings: *const *const c_char,
    n: c_int,
) -> *mut c_char {
    if strings.is_null() || n < 1 {
        return ptr::null_mut();
    }
    libc::strdup(*strings)
}
unsafe extern "C" fn d_thick(
    h: *mut c_void,
    width: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    colour: c_int,
) {
    frontend(h).push(Shape::Line {
        // pzfe.c stores sixteenth-pixel integers; the Go replayer then
        // restores them and moves every line to pixel centres.
        x0: (x0 * 16.0).trunc() / 16.0 + 0.5,
        y0: (y0 * 16.0).trunc() / 16.0 + 0.5,
        x1: (x1 * 16.0).trunc() / 16.0 + 0.5,
        y1: (y1 * 16.0).trunc() / 16.0 + 0.5,
        width: width.max(1.2),
        colour,
    });
}
static DRAWING: DrawingApi = DrawingApi {
    text: Some(d_text),
    rect: Some(d_rect),
    line: Some(d_line),
    polygon: Some(d_polygon),
    circle: Some(d_circle),
    update: Some(d_update),
    clip: Some(d_clip),
    unclip: Some(d_unclip),
    start: Some(d_noop),
    end: Some(d_noop),
    status: Some(d_status),
    blitter_new: Some(d_blitter_new),
    blitter_free: Some(d_blitter_free),
    blitter_save: Some(d_blitter_save),
    blitter_load: Some(d_blitter_load),
    begin_doc: None,
    begin_page: None,
    begin_puzzle: None,
    end_puzzle: None,
    end_page: None,
    end_doc: None,
    line_width: None,
    line_dotted: None,
    text_fallback: Some(d_fallback),
    thick_line: Some(d_thick),
};

// Symbols required by the unchanged engine. The fatal path deliberately
// aborts without trying to interpret a foreign variadic format string.
#[no_mangle]
unsafe extern "C" fn fatal(_format: *const c_char) {
    libc::abort();
}
#[no_mangle]
unsafe extern "C" fn frontend_default_colour(fe: *mut Frontend, out: *mut c_float) {
    ptr::copy_nonoverlapping((*fe).bg.as_ptr(), out, 3);
}
#[no_mangle]
unsafe extern "C" fn activate_timer(fe: *mut Frontend) {
    (*fe).timer = true;
}
#[no_mangle]
unsafe extern "C" fn deactivate_timer(fe: *mut Frontend) {
    (*fe).timer = false;
}
#[no_mangle]
unsafe extern "C" fn get_random_seed(out: *mut *mut c_void, size: *mut c_int) {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |v| v.as_nanos());
    let data = libc::malloc(std::mem::size_of::<u128>());
    if data.is_null() {
        libc::abort();
    }
    ptr::copy_nonoverlapping((&seed as *const u128).cast::<u8>(), data.cast(), 16);
    *out = data;
    *size = 16;
}
#[no_mangle]
unsafe extern "C" fn document_add_puzzle(
    _doc: *mut c_void,
    _game: *const Game,
    _params: *mut Params,
    _state: *mut c_void,
    _solution: *mut c_void,
) {
}

struct Core {
    midend: *mut Midend,
    frontend: Box<Frontend>,
    palette: Vec<Color>,
    width: i32,
    height: i32,
    presets: Vec<String>,
}
// A Core is created entirely on a worker and transferred once to the UI.
// Its C engine and callback frontend have one owner and are never concurrent.
unsafe impl Send for Core {}
impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            midend_free(self.midend);
        }
    }
}
impl Core {
    #[cfg(test)]
    fn new(index: usize, save: Option<Vec<u8>>) -> Result<Self, String> {
        Self::configured(index, save, None)
    }
    fn configured(
        index: usize,
        save: Option<Vec<u8>>,
        preset: Option<usize>,
    ) -> Result<Self, String> {
        let engine = unsafe {
            match index {
                0 => &mines,
                1 => &solo,
                2 => &net,
                3 => &flood,
                4 => &samegame,
                5 => &fifteen,
                6 => &lightup,
                7 => &unruly,
                8 => &towers,
                9 => &keen,
                10 => &pattern,
                11 => &tents,
                _ => return Err("Unknown puzzle".into()),
            }
        };
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: 624.0,
            h: 384.0,
        };
        let mut frontend = Box::new(Frontend {
            commands: Vec::with_capacity(2048),
            clip: bounds,
            bounds,
            bg: [0.88, 0.89, 0.91],
            status: String::new(),
            timer: false,
        });
        let fe = &mut *frontend as *mut Frontend;
        let midend = unsafe { midend_new(fe.cast(), engine, &DRAWING, fe.cast()) };
        if midend.is_null() {
            return Err("Puzzle engine allocation failed".into());
        }
        let mut out = Self {
            midend,
            frontend,
            palette: Vec::new(),
            width: 624,
            height: 384,
            presets: Vec::new(),
        };
        let loaded = save.as_deref().is_some_and(|s| out.load(s).is_ok());
        if !loaded {
            unsafe {
                if let Some(index) = preset {
                    if index < midend_num_presets(midend).max(0) as usize {
                        let mut name = ptr::null_mut();
                        let mut params = ptr::null_mut();
                        midend_fetch_preset(midend, index as i32, &mut name, &mut params);
                        midend_set_params(midend, params);
                    }
                }
                midend_new_game(midend);
            }
        }
        out.layout();
        if PUZZLES[index].numeric {
            unsafe {
                midend_process_key(midend, 0, 0, DOWN);
                midend_process_key(midend, 0, 0, UP);
            }
        }
        out.refresh();
        let n = unsafe { midend_num_presets(midend) }.clamp(0, 200);
        for i in 0..n {
            let mut name = ptr::null_mut();
            let mut params = ptr::null_mut();
            unsafe {
                midend_fetch_preset(midend, i, &mut name, &mut params);
            }
            out.presets.push(if name.is_null() {
                format!("Preset {}", i + 1)
            } else {
                unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned()
            });
        }
        Ok(out)
    }
    fn layout(&mut self) {
        self.width = 624;
        self.height = 384;
        unsafe {
            midend_size(self.midend, &mut self.width, &mut self.height, 1);
        }
        self.frontend.bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: self.width as f32,
            h: self.height as f32,
        };
        self.frontend.clip = self.frontend.bounds;
        let mut n = 0;
        let colours = unsafe { midend_colours(self.midend, &mut n) };
        if !colours.is_null() && (1..1000).contains(&n) {
            let floats = unsafe { std::slice::from_raw_parts(colours, n as usize * 3) };
            self.palette = floats
                .as_chunks::<3>()
                .0
                .iter()
                .map(|v| [v[0], v[1], v[2], 1.0])
                .collect();
        }
        unsafe {
            libc::free(colours.cast());
        }
    }
    fn refresh(&mut self) {
        self.frontend.commands.clear();
        self.frontend.clip = self.frontend.bounds;
        unsafe {
            midend_force_redraw(self.midend);
            midend_redraw(self.midend);
        }
    }
    fn key(&mut self, key: i32) {
        unsafe {
            midend_process_key(self.midend, 0, 0, key);
        }
        self.refresh();
    }
    fn tick(&mut self, dt: f32) {
        if self.frontend.timer {
            let n = self.frontend.commands.len();
            unsafe {
                midend_timer(self.midend, dt);
            }
            if self.frontend.commands.len() != n {
                self.refresh();
            }
        }
    }
    fn save(&self) -> Vec<u8> {
        let mut output = Vec::<u8>::new();
        unsafe {
            midend_serialise(
                self.midend,
                write_save,
                (&mut output as *mut Vec<u8>).cast(),
            );
        }
        output
    }
    fn load(&mut self, save: &[u8]) -> Result<(), String> {
        if save.len() > 8 * 1024 * 1024 {
            return Err("Puzzle save too large".into());
        }
        let mut reader = SaveReader { data: save, at: 0 };
        let error = unsafe {
            midend_deserialise(
                self.midend,
                read_save,
                (&mut reader as *mut SaveReader<'_>).cast(),
            )
        };
        if error.is_null() {
            Ok(())
        } else {
            Err(unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned())
        }
    }
    fn colour(&self, index: i32) -> Color {
        self.palette
            .get(index as usize)
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, 1.0])
    }
}
unsafe extern "C" fn write_save(ctx: *mut c_void, buf: *mut c_void, len: c_int) {
    if len > 0 {
        (&mut *(ctx as *mut Vec<u8>))
            .extend_from_slice(std::slice::from_raw_parts(buf.cast::<u8>(), len as usize));
    }
}
struct SaveReader<'a> {
    data: &'a [u8],
    at: usize,
}
unsafe extern "C" fn read_save(ctx: *mut c_void, buf: *mut c_void, len: c_int) -> c_int {
    let reader = &mut *(ctx as *mut SaveReader<'_>);
    if len < 0 || len as usize > reader.data.len().saturating_sub(reader.at) {
        return 0;
    }
    ptr::copy_nonoverlapping(
        reader.data.as_ptr().add(reader.at),
        buf.cast(),
        len as usize,
    );
    reader.at += len as usize;
    1
}

struct GenerationJob {
    index: usize,
    preset: Option<usize>,
    save: Option<Vec<u8>>,
    new_game: bool,
    cancelled: Arc<AtomicBool>,
    result: mpsc::SyncSender<Result<Core, String>>,
}
static GENERATOR: OnceLock<Result<mpsc::SyncSender<GenerationJob>, String>> = OnceLock::new();
fn generator() -> Result<mpsc::SyncSender<GenerationJob>, String> {
    GENERATOR
        .get_or_init(|| {
            let (tx, rx) = mpsc::sync_channel::<GenerationJob>(1);
            std::thread::Builder::new()
                .name("puzzle-generator".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        if job.cancelled.load(Ordering::Relaxed) {
                            continue;
                        }
                        let result =
                            Core::configured(job.index, job.save, job.preset).map(|mut core| {
                                if job.new_game {
                                    // Keep the current parameters (including a loaded custom
                                    // game) while generating its replacement off the UI.
                                    unsafe {
                                        midend_process_key(core.midend, 0, 0, 'n' as i32);
                                    }
                                    core.layout();
                                    core.refresh();
                                }
                                core
                            });
                        if !job.cancelled.load(Ordering::Relaxed) {
                            let _ = job.result.send(result);
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(tx)
        })
        .clone()
}

pub struct PuzzleGame {
    index: usize,
    cancelled: Arc<AtomicBool>,
    core: Option<Core>,
    loading: Option<mpsc::Receiver<Result<Core, String>>>,
    error: Option<String>,
    digit: i32,
    menu: bool,
    menu_index: usize,
    preset: usize,
}
impl Drop for PuzzleGame {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl PuzzleGame {
    pub fn new(index: usize) -> Result<Self, String> {
        Self::with_save(index, None)
    }
    pub fn with_save(index: usize, save: Option<Vec<u8>>) -> Result<Self, String> {
        if index >= PUZZLES.len() {
            return Err("Unknown puzzle".into());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        generator()?
            .try_send(GenerationJob {
                index,
                preset: None,
                save,
                new_game: false,
                cancelled: cancelled.clone(),
                result: tx,
            })
            .map_err(|_| "Puzzle generator is busy; try again shortly".to_string())?;
        Ok(Self {
            index,
            cancelled,
            core: None,
            loading: Some(rx),
            error: None,
            digit: 1,
            menu: false,
            menu_index: 0,
            preset: 0,
        })
    }
    pub fn title(&self) -> &str {
        PUZZLES[self.index].name
    }
    pub fn id(&self) -> &str {
        PUZZLES[self.index].id
    }
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn serialize_snapshot(&self) -> Option<Vec<u8>> {
        self.core.as_ref().map(Core::save)
    }
    pub fn tick(&mut self, dt: f32) {
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(Ok(core)) => {
                    self.preset = unsafe { midend_which_preset(core.midend) }.max(0) as usize;
                    self.core = Some(core);
                    self.loading = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    self.loading = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Puzzle worker stopped".into());
                    self.loading = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.menu {
            if let Some(core) = &mut self.core {
                core.tick(dt.clamp(0.0, 0.1));
            }
        }
    }
    fn new_puzzle(&mut self, preset: Option<usize>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let save = self.core.as_ref().map(Core::save);
        let new_game = save.is_some();
        let result = generator().and_then(|g| {
            g.try_send(GenerationJob {
                index: self.index,
                preset,
                save,
                new_game,
                cancelled: cancelled.clone(),
                result: tx,
            })
            .map_err(|_| "Puzzle generator is busy; try again shortly".to_string())
        });
        match result {
            Ok(()) => {
                self.cancelled.store(true, Ordering::Relaxed);
                self.cancelled = cancelled;
                self.loading = Some(rx);
                self.core = None;
                self.error = None;
                self.menu = false;
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn new_game(&mut self) {
        let preset = self.core.as_ref().and_then(|core| {
            let index = unsafe { midend_which_preset(core.midend) };
            if index >= 0 {
                Some(index as usize)
            } else {
                None
            }
        });
        self.new_puzzle(preset);
    }
    pub fn undo(&mut self) {
        if let Some(core) = self.core.as_mut() {
            core.key('u' as i32);
        }
    }
    pub fn back_consumed(&mut self) -> bool {
        false
    }
    pub fn handle(&mut self, action: Action) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        if let Some(key) = input_key(PUZZLES[self.index].numeric, &mut self.digit, action) {
            core.key(key);
        }
    }
    pub fn draw(&self, r: &mut Renderer, _dark: bool) {
        let background = self
            .core
            .as_ref()
            .and_then(|core| core.palette.first())
            .copied()
            .unwrap_or([224.0 / 255.0, 227.0 / 255.0, 232.0 / 255.0, 1.0]);
        r.rect(0.0, 40.0, 640.0, 440.0, background);
        let Some(core) = &self.core else {
            r.centered(
                self.error.as_deref().unwrap_or("Preparing puzzle..."),
                16.0,
                220.0,
                608.0,
                24,
                [0.1, 0.12, 0.16, 1.0],
            );
            return;
        };
        let ox = (640 - core.width) as f32 * 0.5;
        let oy = 40.0 + ((396 - core.height) / 2) as f32;
        for primitive in &core.frontend.commands {
            let clip = primitive.clip;
            r.push_clip(ox + clip.x, oy + clip.y, clip.w, clip.h);
            let dx = ox + primitive.dx;
            let dy = oy + primitive.dy;
            match &primitive.shape {
                Shape::Text {
                    x,
                    y,
                    size,
                    align,
                    colour,
                    text,
                } => {
                    let width = r.text_width_weighted(text, *size as f32, 700);
                    let x = x + dx
                        - match align & 3 {
                            1 => width * 0.5,
                            2 => width,
                            _ => 0.0,
                        };
                    let y = y
                        + dy
                        + if align & 0x100 != 0 {
                            *size as f32 * 0.36
                        } else {
                            0.0
                        };
                    r.text_baseline(
                        text,
                        x,
                        y,
                        *size as f32,
                        700,
                        core.colour(*colour),
                        width + 2.0,
                    );
                }
                Shape::Rect { x, y, w, h, colour } => {
                    r.rect(x + dx, y + dy, *w, *h, core.colour(*colour))
                }
                Shape::Line {
                    x0,
                    y0,
                    x1,
                    y1,
                    width,
                    colour,
                } => puzzle_line(
                    r,
                    x0 + dx,
                    y0 + dy,
                    x1 + dx,
                    y1 + dy,
                    *width,
                    core.colour(*colour),
                ),
                Shape::Polygon {
                    points,
                    triangles,
                    fill,
                    outline,
                } => {
                    if *fill >= 0 {
                        for triangle in triangles.iter().copied() {
                            r.triangle(
                                triangle.map(|p| [p[0] + dx, p[1] + dy]),
                                core.colour(*fill),
                            );
                        }
                    }
                    if *outline >= 0 {
                        for i in 0..points.len() {
                            let a = points[i];
                            let b = points[(i + 1) % points.len()];
                            puzzle_line(
                                r,
                                a[0] + dx,
                                a[1] + dy,
                                b[0] + dx,
                                b[1] + dy,
                                1.2,
                                core.colour(*outline),
                            );
                        }
                    }
                }
                Shape::Circle {
                    x,
                    y,
                    r: radius,
                    fill,
                    outline,
                    points,
                    ..
                } => {
                    if *fill >= 0 {
                        puzzle_circle(
                            r,
                            *x + dx,
                            *y + dy,
                            *radius,
                            points,
                            dx,
                            dy,
                            core.colour(*fill),
                        );
                    }
                    if *outline >= 0 {
                        for i in 0..points.len() {
                            let a = points[i];
                            let b = points[(i + 1) % points.len()];
                            puzzle_line(
                                r,
                                a[0] + dx,
                                a[1] + dy,
                                b[0] + dx,
                                b[1] + dy,
                                1.2,
                                core.colour(*outline),
                            );
                        }
                    }
                }
            }
            r.pop_clip();
        }
        let mut status = if unsafe { midend_status(core.midend) } > 0 {
            "Solved!".to_owned()
        } else {
            core.frontend.status.clone()
        };
        if PUZZLES[self.index].numeric {
            status = format!(
                "Digit: {}   {}",
                if self.digit == 0 {
                    "clear".into()
                } else {
                    self.digit.to_string()
                },
                status
            );
        }
        let hint = if PUZZLES[self.index].numeric {
            "[R] digit · [A] place · [Y] pencil · [L] undo · [START] menu"
        } else {
            match self.index {
                0 => "[A] open · [Y] flag · [L] undo · [START] menu",
                2 => "[A] turn · [Y] turn back · [L] undo · [START] menu",
                3 => "[A] flood with this colour · [L] undo · [START] menu",
                4 => "[A] select, [A] again remove · [L] undo · [START] menu",
                5 => "[DPAD] slide · [L] undo · [START] menu",
                6 => "[A] bulb · [Y] mark · [L] undo · [START] menu",
                7 => "[A] cycle · [Y] cycle back · [L] undo · [START] menu",
                10 => "[A] fill · [Y] empty · [L] undo · [START] menu",
                _ => "[A] tent · [Y] grass · [L] undo · [START] menu",
            }
        };
        r.gradient(
            0.0,
            436.0,
            640.0,
            44.0,
            [0.0, 0.0, 0.0, 0.06],
            [0.0, 0.0, 0.0, 0.12],
        );
        let baseline = 458.0 + crate::render::line_box(20.0).1 * 0.5;
        let ink = [70.0 / 255.0, 76.0 / 255.0, 88.0 / 255.0, 1.0];
        if status.is_empty() {
            let w = r.text_width_weighted(hint, 20.0, 600);
            r.text_baseline(hint, 320.0 - w * 0.5, baseline, 20.0, 600, ink, w + 1.0);
        } else {
            let status = short_text(r, &status, 200.0);
            let width = r.text_width_weighted(&status, 20.0, 600).min(200.0);
            r.text_baseline(
                &status,
                14.0,
                baseline,
                20.0,
                600,
                [30.0 / 255.0, 34.0 / 255.0, 42.0 / 255.0, 1.0],
                200.0,
            );
            let available = 640.0 - 14.0 - (14.0 + width + 20.0);
            let hint = short_text(r, hint, available);
            let w = r.text_width_weighted(&hint, 20.0, 600).min(available);
            r.text_baseline(&hint, 626.0 - w, baseline, 20.0, 600, ink, available);
        }
    }
}

fn short_text(r: &mut Renderer, text: &str, width: f32) -> String {
    if r.text_width_weighted(text, 20.0, 600) <= width {
        return text.into();
    }
    let mut result = String::new();
    for c in text.chars() {
        let next = format!("{result}{c}…");
        if r.text_width_weighted(&next, 20.0, 600) > width {
            break;
        }
        result.push(c);
    }
    result.push('…');
    result
}

fn input_key(numeric: bool, digit: &mut i32, action: Action) -> Option<i32> {
    Some(match action {
        Action::Up => UP,
        Action::Down => DOWN,
        Action::Left => LEFT,
        Action::Right => RIGHT,
        Action::Select => {
            if numeric {
                if *digit == 0 {
                    SELECT2
                } else {
                    '0' as i32 + *digit
                }
            } else {
                SELECT
            }
        }
        Action::Options => {
            if numeric {
                SELECT
            } else {
                SELECT2
            }
        }
        Action::Previous => 'u' as i32,
        Action::Next if numeric => {
            *digit = (*digit + 1) % 10;
            return None;
        }
        Action::Next => 'r' as i32,
        _ => return None,
    })
}

fn circle_points(cx: f32, cy: f32, r: f32) -> Vec<[f32; 2]> {
    let count = ((r * 1.5) as usize).clamp(16, 64);
    (0..count)
        .map(|i| {
            let a = i as f32 / count as f32 * std::f32::consts::TAU;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}
fn line_rings(x0: f32, y0: f32, x1: f32, y1: f32, width: f32) -> ([[f32; 2]; 4], [[f32; 2]; 4]) {
    let length = (x1 - x0).hypot(y1 - y0);
    let (ux, uy) = if length < 0.01 {
        (1.0, 0.0)
    } else {
        ((x1 - x0) / length, (y1 - y0) / length)
    };
    let nx = -uy;
    let ny = ux;
    let half = width * 0.5;
    let ring = |inset: f32| {
        let side = (half - inset).max(0.0);
        let cap = half - inset;
        [
            [x0 - ux * cap + nx * side, y0 - uy * cap + ny * side],
            [x1 + ux * cap + nx * side, y1 + uy * cap + ny * side],
            [x1 + ux * cap - nx * side, y1 + uy * cap - ny * side],
            [x0 - ux * cap - nx * side, y0 - uy * cap - ny * side],
        ]
    };
    (ring(0.5), ring(-0.5))
}
#[allow(clippy::too_many_arguments)]
fn puzzle_circle(
    r: &mut Renderer,
    cx: f32,
    cy: f32,
    radius: f32,
    points: &[[f32; 2]],
    dx: f32,
    dy: f32,
    color: Color,
) {
    if radius <= 0.0 {
        return;
    }
    let inset = (radius - 0.5).max(0.0) / radius;
    let outset = (radius + 0.5) / radius;
    let mut clear = color;
    clear[3] = 0.0;
    let ring =
        |p: [f32; 2], scale: f32| [cx + (p[0] + dx - cx) * scale, cy + (p[1] + dy - cy) * scale];
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let ia = ring(a, inset);
        let ib = ring(b, inset);
        let oa = ring(a, outset);
        let ob = ring(b, outset);
        r.triangle([[cx, cy], ia, ib], color);
        r.colored_triangle([ia, ib, ob], [color, color, clear]);
        r.colored_triangle([ia, ob, oa], [color, clear, clear]);
    }
}

fn puzzle_line(r: &mut Renderer, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
    let (inner, outer) = line_rings(x0, y0, x1, y1, width);
    r.triangle([inner[0], inner[1], inner[2]], color);
    r.triangle([inner[0], inner[2], inner[3]], color);
    let mut clear = color;
    clear[3] = 0.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        r.colored_triangle([inner[i], inner[j], outer[j]], [color, color, clear]);
        r.colored_triangle([inner[i], outer[j], outer[i]], [color, clear, clear]);
    }
}

fn triangulate(points: &[[f32; 2]]) -> Vec<[[f32; 2]; 3]> {
    if points.len() < 3 {
        return Vec::new();
    }
    let area: f32 = (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut indices: Vec<usize> = (0..points.len()).collect();
    let mut out = Vec::with_capacity(points.len() - 2);
    let mut attempts = 0;
    while indices.len() > 3 && attempts < points.len() * points.len() {
        let mut found = false;
        for i in 0..indices.len() {
            let a = points[indices[(i + indices.len() - 1) % indices.len()]];
            let b = points[indices[i]];
            let c = points[indices[(i + 1) % indices.len()]];
            if cross(a, b, c) * sign <= 0.0001 {
                continue;
            }
            if indices.iter().any(|j| {
                let p = points[*j];
                p != a
                    && p != b
                    && p != c
                    && cross(a, b, p) * sign >= 0.0
                    && cross(b, c, p) * sign >= 0.0
                    && cross(c, a, p) * sign >= 0.0
            }) {
                continue;
            }
            out.push([a, b, c]);
            indices.remove(i);
            found = true;
            break;
        }
        if !found {
            break;
        }
        attempts += 1;
    }
    if indices.len() == 3 {
        out.push([points[indices[0]], points[indices[1]], points[indices[2]]]);
    }
    out
}
fn cross(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference_fixture(index: usize) -> Core {
        let mut core = Core::new(index, None).unwrap();
        let parameters = unsafe {
            let raw = midend_get_random_seed(core.midend);
            assert!(!raw.is_null());
            let seed = CStr::from_ptr(raw).to_string_lossy().into_owned();
            libc::free(raw.cast());
            seed.split('#').next().unwrap().to_owned()
        };
        let mut seed = format!("{parameters}#betterPod-1.1-{}", PUZZLES[index].id).into_bytes();
        seed.push(0);
        let error = unsafe { midend_game_id(core.midend, seed.as_mut_ptr().cast()) };
        assert!(
            error.is_null(),
            "{}: {}",
            PUZZLES[index].name,
            if error.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            }
        );
        unsafe {
            midend_new_game(core.midend);
        }
        core.layout();
        if PUZZLES[index].numeric {
            core.key(DOWN);
            core.key(UP);
        }
        core.refresh();
        core
    }
    #[test]
    fn all_twelve_seeded_engines_have_repeatable_positions_and_source_controls() {
        for index in 0..PUZZLES.len() {
            let mut a = reference_fixture(index);
            let b = reference_fixture(index);
            let initial = a.save();
            assert_eq!(initial, b.save(), "{} seed", PUZZLES[index].name);
            assert!((1..=624).contains(&a.width));
            assert!((1..=384).contains(&a.height));
            let mut digit = 1;
            for action in [
                Action::Right,
                Action::Down,
                Action::Select,
                Action::Options,
                Action::Previous,
                Action::Next,
                Action::Left,
                Action::Up,
            ] {
                if let Some(key) = input_key(PUZZLES[index].numeric, &mut digit, action) {
                    a.key(key);
                }
            }
            assert!(!a.frontend.commands.is_empty());
            assert!(a.load(&initial).is_ok());
            a.layout();
            a.refresh();
            assert_eq!(a.save(), initial, "{} save restore", PUZZLES[index].name);
        }
    }
    #[test]
    fn numeric_digit_clear_pencil_and_global_keys_follow_go_frontend() {
        let mut digit = 9;
        assert_eq!(input_key(true, &mut digit, Action::Next), None);
        assert_eq!(digit, 0);
        assert_eq!(input_key(true, &mut digit, Action::Select), Some(SELECT2));
        assert_eq!(input_key(true, &mut digit, Action::Options), Some(SELECT));
        assert_eq!(input_key(true, &mut digit, Action::Next), None);
        assert_eq!(digit, 1);
        assert_eq!(
            input_key(true, &mut digit, Action::Select),
            Some('1' as i32)
        );
        assert_eq!(input_key(false, &mut digit, Action::Next), Some('r' as i32));
        assert_eq!(
            input_key(false, &mut digit, Action::Previous),
            Some('u' as i32)
        );
        assert_eq!(input_key(false, &mut digit, Action::Options), Some(SELECT2));
        assert_eq!(input_key(false, &mut digit, Action::PlayPause), None);
        assert_eq!(input_key(false, &mut digit, Action::Back), None);
        assert_eq!(input_key(false, &mut digit, Action::Start), None);
    }
    #[test]
    fn drawing_callbacks_keep_source_quantization_circle_limit_and_square_caps() {
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: 624.0,
            h: 384.0,
        };
        let mut frontend = Frontend {
            commands: Vec::new(),
            clip: bounds,
            bounds,
            bg: [0.88, 0.89, 0.91],
            status: String::new(),
            timer: false,
        };
        unsafe {
            d_thick(
                (&mut frontend as *mut Frontend).cast(),
                0.5,
                3.19,
                -2.19,
                10.99,
                14.02,
                0,
            );
        }
        let Shape::Line {
            x0,
            y0,
            x1,
            y1,
            width,
            ..
        } = frontend.commands[0].shape
        else {
            panic!("line")
        };
        assert_eq!(
            (x0, y0, x1, y1, width),
            (3.6875, -1.6875, 11.4375, 14.5, 1.2)
        );
        assert_eq!(circle_points(0.0, 0.0, 2.0).len(), 16);
        assert_eq!(circle_points(0.0, 0.0, 100.0).len(), 64);
        let (inner, outer) = line_rings(10.0, 20.0, 30.0, 20.0, 2.0);
        assert_eq!(inner[0], [9.5, 20.5]);
        assert_eq!(outer[0], [8.5, 21.5]);
        let (inner, _) = line_rings(4.0, 6.0, 4.0, 6.0, 2.0);
        assert_eq!(inner[0], [3.5, 6.5]);
    }
    #[test]
    #[ignore = "writes local deterministic twelve-engine source-control fixtures"]
    fn export_twelve_engine_fixtures() -> Result<(), String> {
        use sha2::Digest;
        let out = std::path::PathBuf::from(
            std::env::var("BETTERPOD_PUZZLE_FIXTURES_OUT").map_err(|e| e.to_string())?,
        );
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let mut manifest = Vec::new();
        for index in 0..PUZZLES.len() {
            let mut core = reference_fixture(index);
            let initial = core.save();
            std::fs::write(out.join(format!("{}.sav", PUZZLES[index].id)), &initial)
                .map_err(|e| e.to_string())?;
            let mut digit = 1;
            for action in [
                Action::Right,
                Action::Down,
                Action::Select,
                Action::Options,
                Action::Previous,
                Action::Next,
            ] {
                if let Some(key) = input_key(PUZZLES[index].numeric, &mut digit, action) {
                    core.key(key);
                }
            }
            let after = core.save();
            std::fs::write(out.join(format!("{}-after.sav", PUZZLES[index].id)), &after)
                .map_err(|e| e.to_string())?;
            manifest.push(serde_json::json!({"id":PUZZLES[index].id,"width":core.width,"height":core.height,"numeric":PUZZLES[index].numeric,"initial_sha256":format!("{:x}",sha2::Sha256::digest(initial)),"after_sha256":format!("{:x}",sha2::Sha256::digest(after)),"draw_commands":core.frontend.commands.len(),"digit":digit,"actions":["Right","Down","A","Y","L","R"]}));
        }
        std::fs::write(
            out.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    #[test]
    fn every_upstream_puzzle_generates_draws_and_round_trips() {
        for index in 0..PUZZLES.len() {
            let mut core = Core::new(index, None).unwrap();
            assert!(core.width > 0 && core.width <= 624);
            // Go pzAreaH = 440 - 44; midend gets pzAreaH - 12 = 384.
            assert!(core.height > 0 && core.height <= 384);
            assert!(
                !core.frontend.commands.is_empty(),
                "{}",
                PUZZLES[index].name
            );
            let bytes = core.save();
            assert!(!bytes.is_empty());
            core.key(DOWN);
            core.key(UP);
            core.key(SELECT);
            core.key('u' as i32);
            assert!(core.load(&bytes).is_ok());
            core.layout();
            core.refresh();
            assert!(!core.frontend.commands.is_empty());
            assert!(unsafe { midend_status(core.midend) } >= -1);
        }
    }
    #[test]
    fn malformed_save_is_rejected_without_out_of_bounds_reads() {
        let mut core = Core::new(5, None).unwrap();
        assert!(core.load(b"not a saved puzzle").is_err());
        core.refresh();
        assert!(!core.frontend.commands.is_empty());
    }
    #[test]
    fn concave_polygon_is_triangulated_without_a_fan_crossing() {
        let p = [[0., 0.], [4., 0.], [4., 4.], [2., 2.], [0., 4.]];
        let triangles = triangulate(&p);
        assert_eq!(triangles.len(), 3);
        let area: f32 = triangles
            .iter()
            .map(|t| cross(t[0], t[1], t[2]).abs() * 0.5)
            .sum();
        assert!((area - 12.).abs() < 0.001);
    }
    #[test]
    fn blitter_regions_preserve_clip_and_translation() {
        let bounds = Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        };
        let mut fe = Frontend {
            commands: Vec::new(),
            clip: bounds,
            bounds,
            bg: [1.; 3],
            status: String::new(),
            timer: false,
        };
        fe.push(Shape::Rect {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
            colour: 0,
        });
        unsafe {
            let h = (&mut fe as *mut Frontend).cast();
            let b = d_blitter_new(h, 10, 10);
            d_blitter_save(h, b, 20, 30);
            d_blitter_load(h, b, 40, 50);
            let p = fe.commands.last().unwrap();
            assert_eq!((p.dx, p.dy), (20., 20.));
            assert_eq!(
                (p.clip.x, p.clip.y, p.clip.w, p.clip.h),
                (40., 50., 10., 10.)
            );
            d_blitter_free(h, b);
        }
    }
}
