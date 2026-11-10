#![windows_subsystem = "windows"]

mod engine;
mod sys;

use eframe::egui;
use egui::{pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, RichText, Stroke};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(0x14, 0x10, 0x2b);
const PANEL: Color32 = Color32::from_rgb(0x22, 0x1a, 0x47);
const FG: Color32 = Color32::from_rgb(0xf0, 0xee, 0xff);
const MUTED: Color32 = Color32::from_rgb(0xa7, 0x9f, 0xd6);
const ACCENT: Color32 = Color32::from_rgb(0x7b, 0x5c, 0xff);
const ACCENT_H: Color32 = Color32::from_rgb(0x95, 0x7c, 0xff);
const BG_COL: Color32 = Color32::from_rgb(0xa5, 0x8e, 0xbc);
const AMBER: Color32 = Color32::from_rgb(0xe0, 0xa0, 0x30);

const WIN_W: u32 = 983;
const WIN_H: u32 = 560;
const COL_W: f32 = 330.0;
const SIZES: [&str; 6] = ["983x560", "1024x640", "1100x650", "1280x720", "1366x768", "1600x900"];

fn muted(s: impl Into<String>) -> RichText {
    RichText::new(s).color(MUTED)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn parse_size(t: &str) -> Option<(u32, u32)> {
    let t = t.trim();
    let idx = t.find(|c: char| matches!(c, 'x' | 'X' | 'х' | 'Х' | '×' | '*'))?;
    let sep_len = t[idx..].chars().next()?.len_utf8();
    let a = t[..idx].trim();
    let b = t[idx + sep_len..].trim();
    if a.len() < 3 || a.len() > 4 || b.len() < 3 || b.len() > 4 {
        return None;
    }
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(COL_W - 24.0);
            ui.spacing_mut().item_spacing = vec2(6.0, 5.0);
            add(ui);
        });
}

fn bar(ui: &mut egui::Ui, w: f32, frac: f32, col: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, BG);
    let fill = Rect::from_min_size(rect.min, vec2(w * frac.clamp(0.0, 1.0), 14.0));
    ui.painter().rect_filled(fill, 0.0, col);
}

/// Полоска «таблетки» с горизонтальным градиентом и закруглёнными краями.
fn pill(p: &egui::Painter, x1: f32, y1: f32, x2: f32, y2: f32, c0: Color32, c1: Color32) {
    let r = 14.0_f32;
    let span = (x2 - x1).max(1.0);
    let mut x = x1 + 2.0;
    while x < x2 - 1.0 {
        let dx = (x - x1).min(x2 - x);
        let inset = if dx >= r { 0.0 } else { r - (r * r - (r - dx) * (r - dx)).max(0.0).sqrt() };
        p.line_segment(
            [pos2(x, y1 + inset), pos2(x, y2 - inset)],
            Stroke::new(4.0, mix(c0, c1, (x - x1) / span)),
        );
        x += 3.0;
    }
}

fn play_button(ui: &mut egui::Ui, w: f32, label: &str, sub: &str, enabled: bool, bold: &egui::FontFamily) -> bool {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 66.0), sense);
    let hover = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    if hover {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let off = if pressed { 2.0 } else { 0.0 };
    let (c0, c1, sh) = if !enabled {
        (
            Color32::from_rgb(0x3a, 0x34, 0x66),
            Color32::from_rgb(0x46, 0x3d, 0x7a),
            Color32::from_rgb(0x26, 0x20, 0x48),
        )
    } else if hover {
        (
            Color32::from_rgb(0x8a, 0x6d, 0xff),
            Color32::from_rgb(0xc6, 0x73, 0xff),
            Color32::from_rgb(0x3a, 0x2a, 0x8a),
        )
    } else {
        (
            Color32::from_rgb(0x6a, 0x4c, 0xff),
            Color32::from_rgb(0xa8, 0x5c, 0xf5),
            Color32::from_rgb(0x2e, 0x22, 0x70),
        )
    };
    let p = ui.painter();
    let h = 66.0 - 8.0;
    pill(p, rect.min.x + 4.0, rect.min.y + 8.0, rect.max.x - 4.0, rect.max.y, sh, sh);
    pill(p, rect.min.x + 4.0, rect.min.y + 3.0 + off, rect.max.x - 4.0, rect.min.y + h + 3.0 + off, c0, c1);
    let cy = rect.min.y + (h + 6.0) / 2.0 + off;
    let cx = rect.center().x;
    let txt_col = if enabled { Color32::WHITE } else { MUTED };
    let dy = if sub.is_empty() { 0.0 } else { 8.0 };
    p.text(
        pos2(cx, cy - dy),
        Align2::CENTER_CENTER,
        label,
        FontId::new(19.0, bold.clone()),
        txt_col,
    );
    if !sub.is_empty() {
        let sc = if enabled { Color32::from_rgb(0xe6, 0xde, 0xfc) } else { MUTED };
        p.text(pos2(cx, cy + 15.0), Align2::CENTER_CENTER, sub, FontId::proportional(11.0), sc);
    }
    enabled && resp.clicked()
}

fn load_tex(ctx: &egui::Context, name: &str, bytes: &[u8], opts: egui::TextureOptions) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(name, ci, opts))
}

fn setup_fonts(ctx: &egui::Context) -> bool {
    let mut fonts = egui::FontDefinitions::default();
    let base: Vec<String> = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut has_bold = false;
    if let Ok(b) = std::fs::read("C:\\Windows\\Fonts\\segoeuib.ttf") {
        fonts.font_data.insert("segoeb".to_string(), egui::FontData::from_owned(b));
        let mut fam = vec!["segoeb".to_string()];
        fam.extend(base.clone());
        fonts.families.insert(egui::FontFamily::Name("bold".into()), fam);
        has_bold = true;
    }
    if let Ok(b) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts.font_data.insert("segoe".to_string(), egui::FontData::from_owned(b));
        if let Some(f) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            f.insert(0, "segoe".to_string());
        }
    }
    ctx.set_fonts(fonts);
    has_bold
}

fn setup_style(ctx: &egui::Context) {
    ctx.style_mut(|s| {
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.window_fill = PANEL;
        v.panel_fill = BG_COL;
        v.extreme_bg_color = BG;
        v.faint_bg_color = BG;
        v.selection.bg_fill = ACCENT;
        v.widgets.noninteractive.fg_stroke.color = FG;
        v.widgets.inactive.fg_stroke.color = FG;
        v.widgets.hovered.fg_stroke.color = Color32::WHITE;
        v.widgets.active.fg_stroke.color = Color32::WHITE;
        v.widgets.inactive.weak_bg_fill = BG;
        v.widgets.inactive.bg_fill = BG;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2a, 0x22, 0x58);
        v.widgets.hovered.bg_fill = Color32::from_rgb(0x2a, 0x22, 0x58);
        v.widgets.active.weak_bg_fill = ACCENT;
        v.widgets.active.bg_fill = ACCENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.window_stroke = Stroke::NONE;
        s.spacing.button_padding = vec2(8.0, 3.0);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.0));
    });
}

// ---------------------------------------------------------------- состояние

#[derive(Clone)]
enum Confirm {
    LowRam,
    ClearLogs,
}

#[derive(Clone)]
struct Dialog {
    title: String,
    text: String,
    confirm: Option<Confirm>,
}

/// Декодирует свою картинку фона в отдельном потоке (большие картинки уменьшаются).
fn spawn_bg_decode(shared: Arc<Mutex<Shared>>, ctx: egui::Context, path: String) {
    std::thread::spawn(move || {
        let res = image::open(&path).ok().map(|mut img| {
            if img.width() > 1600 || img.height() > 1000 {
                img = img.resize(1600, 1000, image::imageops::FilterType::Triangle);
            }
            let rgba = img.to_rgba8();
            (rgba.width() as usize, rgba.height() as usize, rgba.into_raw())
        });
        let mut sh = shared.lock().unwrap();
        match res {
            Some(v) => sh.bg_img = Some(v),
            None => sh.bg_fail = true,
        }
        drop(sh);
        ctx.request_repaint();
    });
}

/// Открывает окно выбора картинки в отдельном потоке.
fn spawn_bg_pick(shared: Arc<Mutex<Shared>>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let r = sys::pick_image();
        shared.lock().unwrap().bg_pick = Some(r);
        ctx.request_repaint();
    });
}

#[derive(Clone)]
struct SettingsState {
    ram: u32,
    ram_text: String,
    close_after: bool,
    size: String,
    jvm: String,
    total: u32,
    has_ram: bool,
    lo: u32,
    top: u32,
}

#[derive(Default)]
struct Shared {
    dialogs: Vec<(String, String)>,
    versions: Option<Vec<String>>,
    avatar: Option<(String, Vec<u8>)>,
    cfg_patch: Option<engine::Config>,
    clear_pass: bool,
    quit_at: Option<Instant>,
    shortcut_done: bool,
    bg_pick: Option<Option<String>>,
    bg_img: Option<(usize, usize, Vec<u8>)>,
    bg_fail: bool,
}

enum Act {
    Main,
    Ely,
    Logout,
    Settings,
    OpenGame,
    OpenMods,
    OpenRes,
}

struct App {
    ctx: egui::Context,
    cfg: engine::Config,
    shared: Arc<Mutex<Shared>>,
    prog: Arc<engine::Progress>,
    installing: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    cancelling: bool,
    was_busy: bool,
    mode: String,
    nick: String,
    ver: String,
    loader: String,
    ely_login: String,
    ely_pass: String,
    versions: Vec<String>,
    installed: bool,
    checked_key: (String, String),
    recheck: bool,
    ram: Option<(u64, u64)>,
    ram_at: Instant,
    girl: Option<egui::TextureHandle>,
    logo: Option<egui::TextureHandle>,
    skin: Option<egui::TextureHandle>,
    avatar_for: String,
    avatar_req: String,
    ripple: Option<(egui::Pos2, Instant)>,
    fs: bool,
    dialog: Option<Dialog>,
    settings: Option<SettingsState>,
    custom_bg: Option<egui::TextureHandle>,
    bg_picking: bool,
    bold: egui::FontFamily,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, cfg: engine::Config) -> Self {
        let ctx = cc.egui_ctx.clone();
        let has_bold = setup_fonts(&ctx);
        setup_style(&ctx);
        let girl = load_tex(&ctx, "girl", include_bytes!("girl_00.png"), egui::TextureOptions::LINEAR);
        let logo = load_tex(&ctx, "logo", include_bytes!("icon64.png"), egui::TextureOptions::LINEAR);
        let repaint_ctx = ctx.clone();
        let prog = Arc::new(engine::Progress::new(Box::new(move || repaint_ctx.request_repaint())));
        let shared = Arc::new(Mutex::new(Shared::default()));

        // список версий
        {
            let shared = shared.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                if let Ok(v) = engine::release_versions() {
                    shared.lock().unwrap().versions = Some(v);
                    ctx.request_repaint();
                }
            });
        }
        // самоустановка и ярлык
        {
            let shared = shared.clone();
            let ctx = ctx.clone();
            let sv = cfg.shortcut_ver;
            std::thread::spawn(move || {
                let copied = sys::install_self();
                if cfg!(windows) && (copied || sv != 4) && sys::create_shortcut() {
                    shared.lock().unwrap().shortcut_done = true;
                    ctx.request_repaint();
                }
            });
        }

        // своя картинка фона декодируется в отдельном потоке, чтобы запуск лаунчера не замедлялся
        if cfg.bg_mode == "custom" && !cfg.bg_path.is_empty() {
            spawn_bg_decode(shared.clone(), ctx.clone(), cfg.bg_path.clone());
        }

        let mut versions = vec!["1.16.5".to_string()];
        if !versions.contains(&cfg.ver) {
            versions.push(cfg.ver.clone());
        }
        App {
            ctx,
            mode: cfg.mode.clone(),
            nick: cfg.nick.clone(),
            ver: cfg.ver.clone(),
            loader: cfg.loader.clone(),
            cfg,
            shared,
            prog,
            installing: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            cancelling: false,
            was_busy: false,
            ely_login: String::new(),
            ely_pass: String::new(),
            versions,
            installed: false,
            checked_key: (String::new(), String::new()),
            recheck: true,
            ram: sys::ram_info(),
            ram_at: Instant::now(),
            girl,
            logo,
            skin: None,
            avatar_for: String::new(),
            avatar_req: String::new(),
            ripple: None,
            fs: false,
            dialog: None,
            settings: None,
            custom_bg: None,
            bg_picking: false,
            bold: if has_bold { egui::FontFamily::Name("bold".into()) } else { egui::FontFamily::Proportional },
        }
    }

    fn bold_font(&self, size: f32) -> FontId {
        FontId::new(size, self.bold.clone())
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Dialog { title: title.to_string(), text: text.to_string(), confirm: None });
    }

    fn toggle_fs(&mut self, ctx: &egui::Context) {
        self.fs = !self.fs;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fs));
    }

    /// Забираем результаты фоновых потоков.
    fn pump(&mut self, ctx: &egui::Context) {
        let mut sh = self.shared.lock().unwrap();
        if let Some(v) = sh.versions.take() {
            if !v.is_empty() {
                self.versions = v;
            }
        }
        if let Some((name, rgba)) = sh.avatar.take() {
            let img = egui::ColorImage::from_rgba_unmultiplied([32, 32], &rgba);
            self.skin = Some(ctx.load_texture("skin", img, egui::TextureOptions::NEAREST));
            self.avatar_for = name;
        }
        if let Some(res) = sh.bg_pick.take() {
            self.bg_picking = false;
            if let Some(path) = res {
                self.cfg.bg_mode = "custom".to_string();
                self.cfg.bg_path = path.clone();
                engine::save_cfg(&self.cfg);
                spawn_bg_decode(self.shared.clone(), ctx.clone(), path);
            }
        }
        if let Some((w, h, rgba)) = sh.bg_img.take() {
            let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            self.custom_bg = Some(ctx.load_texture("custom_bg", img, egui::TextureOptions::LINEAR));
        }
        if sh.bg_fail {
            sh.bg_fail = false;
            if self.cfg.bg_mode == "custom" {
                self.cfg.bg_mode = "image".to_string();
                self.cfg.bg_path.clear();
                engine::save_cfg(&self.cfg);
            }
            self.custom_bg = None;
            sh.dialogs.push(("Фон".to_string(), "Не удалось открыть картинку. Подойдут файлы PNG и JPG.".to_string()));
        }
        if let Some(c) = sh.cfg_patch.take() {
            self.cfg.ely_token = c.ely_token;
            self.cfg.ely_name = c.ely_name;
            self.cfg.ely_uuid = c.ely_uuid;
            self.cfg.ely_client = c.ely_client;
        }
        if sh.clear_pass {
            self.ely_pass.clear();
            sh.clear_pass = false;
        }
        if sh.shortcut_done {
            self.cfg.shortcut_ver = 4;
            engine::save_cfg(&self.cfg);
            sh.shortcut_done = false;
        }
        if self.dialog.is_none() && !sh.dialogs.is_empty() {
            let (t, x) = sh.dialogs.remove(0);
            self.dialog = Some(Dialog { title: t, text: x, confirm: None });
        }
        if let Some(t) = sh.quit_at {
            let now = Instant::now();
            if now >= t {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(t - now);
            }
        }
    }

    fn ely_name(&self) -> Option<String> {
        if self.mode == "ely" && !self.cfg.ely_token.is_empty() && !self.cfg.ely_name.is_empty() {
            Some(self.cfg.ely_name.clone())
        } else {
            None
        }
    }

    fn sync_state(&mut self) {
        let busy = self.installing.load(Ordering::SeqCst) || self.running.load(Ordering::SeqCst);
        if self.was_busy && !busy {
            self.recheck = true;
            self.cancelling = false;
        }
        self.was_busy = busy;
        let key = (self.ver.clone(), self.loader.clone());
        if !busy && (self.recheck || key != self.checked_key) {
            self.installed = engine::is_installed(&key.0, &key.1);
            self.checked_key = key;
            self.recheck = false;
        }
        // аватар Ely.by
        match self.ely_name() {
            Some(n) => {
                if self.avatar_req != n {
                    self.avatar_req = n.clone();
                    let shared = self.shared.clone();
                    let ctx = self.ctx.clone();
                    std::thread::spawn(move || {
                        if let Some(rgba) = engine::fetch_avatar(&n) {
                            shared.lock().unwrap().avatar = Some((n, rgba));
                            ctx.request_repaint();
                        }
                    });
                }
            }
            None => {
                if self.cfg.ely_token.is_empty() {
                    self.avatar_req.clear();
                    self.skin = None;
                    self.avatar_for.clear();
                }
            }
        }
    }

    // ---------------------------------------------------------------- действия

    fn ely_logout(&mut self) {
        self.cfg.ely_token.clear();
        self.cfg.ely_name.clear();
        self.cfg.ely_uuid.clear();
        engine::save_cfg(&self.cfg);
        self.ely_pass.clear();
    }

    fn do_ely(&mut self) {
        let login = self.ely_login.trim().to_string();
        let pass = self.ely_pass.clone();
        let mut cfg = self.cfg.clone();
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            match engine::ely_login(&login, &pass, &mut cfg) {
                Ok(()) => {
                    let mut s = shared.lock().unwrap();
                    s.cfg_patch = Some(cfg);
                    s.clear_pass = true;
                }
                Err(e) => {
                    shared.lock().unwrap().dialogs.push(("Ely.by".to_string(), e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    fn on_main(&mut self) {
        if self.installing.load(Ordering::SeqCst) {
            self.prog.cancel.store(true, Ordering::SeqCst);
            self.cancelling = true;
            self.prog.say("Отмена установки...");
            return;
        }
        if self.installed {
            self.play();
        } else {
            self.install_only();
        }
    }

    fn install_only(&mut self) {
        let (mc, loader) = (self.ver.clone(), self.loader.clone());
        self.prog.cancel.store(false, Ordering::SeqCst);
        self.cancelling = false;
        self.installing.store(true, Ordering::SeqCst);
        let prog = self.prog.clone();
        let shared = self.shared.clone();
        let installing = self.installing.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            match engine::prepare(&mc, &loader, &prog) {
                Ok(_) => {
                    prog.say(format!("Установлено: {mc} {loader}"));
                    prog.set(0, 1);
                }
                Err(e) => {
                    if engine::is_cancel(&e) {
                        prog.say("Установка отменена");
                        prog.set(0, 1);
                    } else {
                        shared.lock().unwrap().dialogs.push(("Ошибка установки".to_string(), e.to_string()));
                        prog.say("Ошибка");
                    }
                }
            }
            installing.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn play(&mut self) {
        let ram = self.cfg.ram.unwrap_or_else(engine::default_ram);
        if self.mode == "offline" && self.nick.trim().is_empty() {
            self.info("Ошибка", "Введите ник");
            return;
        }
        if self.mode == "ely" && self.cfg.ely_token.is_empty() {
            self.info("Ошибка", "Сначала войдите в Ely.by");
            return;
        }
        if let Some((_, avail)) = sys::ram_info() {
            if (avail as f64) < ram as f64 * 0.85 {
                self.dialog = Some(Dialog {
                    title: "Мало свободной памяти".to_string(),
                    text: format!(
                        "Свободно {} МБ, а игре отдано {} МБ.\nЗакройте браузер и лишние программы, иначе будут тормоза.\n\nВсё равно запустить?",
                        avail, ram
                    ),
                    confirm: Some(Confirm::LowRam),
                });
                return;
            }
        }
        self.start_play();
    }

    fn start_play(&mut self) {
        let ram = self.cfg.ram.unwrap_or_else(engine::default_ram);
        self.cfg.mode = self.mode.clone();
        self.cfg.nick = self.nick.trim().to_string();
        self.cfg.ver = self.ver.clone();
        self.cfg.loader = self.loader.clone();
        engine::save_cfg(&self.cfg);
        self.prog.cancel.store(false, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);

        let mut cfg = self.cfg.clone();
        let (mode, nick, mc, loader) =
            (self.mode.clone(), self.nick.clone(), self.ver.clone(), self.loader.clone());
        let prog = self.prog.clone();
        let shared = self.shared.clone();
        let running = self.running.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let res = engine::run_game(&mut cfg, &mode, &nick, &mc, &loader, ram, &prog);
            match res {
                Ok(()) => {
                    prog.say("Игра запущена");
                    let mut s = shared.lock().unwrap();
                    s.cfg_patch = Some(cfg.clone());
                    if cfg.close_after {
                        s.quit_at = Some(Instant::now() + Duration::from_millis(2500));
                    }
                }
                Err(e) => {
                    if engine::is_cancel(&e) {
                        prog.say("Установка отменена");
                    } else {
                        shared.lock().unwrap().dialogs.push(("Ошибка".to_string(), e.to_string()));
                        prog.say("Ошибка");
                    }
                    prog.set(0, 1);
                }
            }
            running.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    fn open_settings(&mut self) {
        let ri = sys::ram_info();
        let total = ri.map(|r| r.0 as u32).unwrap_or(4096);
        let lo = 512;
        let top = (total.saturating_sub(256)).max(1024);
        let ram = self.cfg.ram.unwrap_or_else(engine::default_ram).clamp(lo, top);
        let size = if self.cfg.win_size.is_empty() {
            format!("{WIN_W}x{WIN_H}")
        } else {
            self.cfg.win_size.clone()
        };
        self.settings = Some(SettingsState {
            ram,
            ram_text: ram.to_string(),
            close_after: self.cfg.close_after,
            size,
            jvm: self.cfg.jvm_extra.clone(),
            total,
            has_ram: ri.is_some(),
            lo,
            top,
        });
    }

    fn save_settings(&mut self, ctx: &egui::Context, s: &SettingsState) {
        let (w, h) = match parse_size(&s.size) {
            Some(x) => x,
            None => {
                self.info("Размер окна", "Впишите размер так: 1024x640");
                return;
            }
        };
        let (w, h) = (w.max(WIN_W), h.max(WIN_H));
        self.cfg.win_size = format!("{w}x{h}");
        self.cfg.ram = Some(s.ram.clamp(s.lo, s.top));
        self.cfg.close_after = s.close_after;
        self.cfg.jvm_extra = s.jvm.trim().to_string();
        engine::save_cfg(&self.cfg);
        if !self.fs {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(w as f32, h as f32)));
        }
        self.settings = None;
    }

    fn ask_clear_logs(&mut self) {
        let size = engine::dir_size("logs") + engine::dir_size("crash-reports");
        self.dialog = Some(Dialog {
            title: "Очистка".to_string(),
            text: format!("Удалить логи и краш-репорты ({:.1} МБ)?", size as f64 / 1_048_576.0),
            confirm: Some(Confirm::ClearLogs),
        });
    }

    // ---------------------------------------------------------------- интерфейс

    fn column(&mut self, ui: &mut egui::Ui, act: &mut Option<Act>) {
        let inner_w = COL_W - 24.0;
        ui.set_width(COL_W);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

        // карточка: аккаунт, версия, кнопки
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.mode, "offline".to_string(), "Офлайн");
                ui.radio_value(&mut self.mode, "ely".to_string(), "Ely.by (скины)");
                if !self.cfg.ely_token.is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("Выйти").fill(BG)).clicked() {
                            *act = Some(Act::Logout);
                        }
                    });
                }
            });
            if self.mode == "offline" {
                ui.label(muted("Ник"));
                ui.add(egui::TextEdit::singleline(&mut self.nick).desired_width(inner_w));
            } else if !self.cfg.ely_token.is_empty() && !self.cfg.ely_name.is_empty() {
                ui.label(muted(format!("Вы вошли как: {}", self.cfg.ely_name)));
            } else {
                ui.label(muted("Не авторизован"));
                ui.label(muted("Логин / Email"));
                ui.add(egui::TextEdit::singleline(&mut self.ely_login).desired_width(inner_w));
                ui.label(muted("Пароль (с 2FA: пароль:код)"));
                ui.add(egui::TextEdit::singleline(&mut self.ely_pass).password(true).desired_width(inner_w));
                if ui.add(egui::Button::new("Войти в Ely.by").fill(BG)).clicked() {
                    *act = Some(Act::Ely);
                }
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(muted("Версия"));
                egui::ComboBox::from_id_source("ver")
                    .width(92.0)
                    .selected_text(self.ver.clone())
                    .show_ui(ui, |ui| {
                        for v in &self.versions {
                            ui.selectable_value(&mut self.ver, v.clone(), v.as_str());
                        }
                    });
                // загрузчик прижат к правому краю, как кнопка "Выйти" и ряд кнопок ниже
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_source("loader")
                        .width(92.0)
                        .selected_text(self.loader.clone())
                        .show_ui(ui, |ui| {
                            for l in ["Vanilla", "Fabric", "Forge", "Quilt"] {
                                ui.selectable_value(&mut self.loader, l.to_string(), l);
                            }
                        });
                    ui.label(muted("Загрузчик"));
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = vec2(2.0, 3.0);
                let w = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
                let items = [
                    ("Настройки", Act::Settings),
                    ("Файлы игры", Act::OpenGame),
                    ("Моды", Act::OpenMods),
                    ("Ресурспаки", Act::OpenRes),
                ];
                for (text, a) in items {
                    let b = egui::Button::new(RichText::new(text).size(11.0)).fill(BG);
                    if ui.add_sized([w, 24.0], b).clicked() {
                        *act = Some(a);
                    }
                }
            });
        });
        ui.add_space(6.0);

        // карточка: память
        card(ui, |ui| {
            let bw = inner_w;
            let (total, avail) = self.ram.unwrap_or((4096, 4096));
            if self.ram.is_some() {
                let used = 1.0 - avail as f32 / (total.max(1)) as f32;
                let col = if used < 0.75 {
                    Color32::from_rgb(0x7b, 0x5c, 0xff)
                } else if used < 0.9 {
                    AMBER
                } else {
                    Color32::from_rgb(0xe0, 0x50, 0x50)
                };
                bar(ui, bw, used, col);
                ui.label(muted(format!(
                    "Занято {:.1} из {:.1} ГБ  ·  свободно {:.1}",
                    total as f32 * used / 1024.0,
                    total as f32 / 1024.0,
                    avail as f32 / 1024.0
                )));
            } else {
                bar(ui, bw, 0.0, ACCENT);
                ui.label(muted("Данные об ОЗУ недоступны"));
            }
            let ram = self.cfg.ram.unwrap_or_else(engine::default_ram);
            let frac = (ram as f32 / total.max(1) as f32).min(1.0);
            let col = if frac > 0.8 { AMBER } else { Color32::from_rgb(0xc6, 0x73, 0xff) };
            bar(ui, bw, frac, col);
            ui.label(muted(format!(
                "Выделено игре: {} МБ ({:.1} ГБ), {:.0}% памяти",
                ram,
                ram as f32 / 1024.0,
                frac * 100.0
            )));
        });
        ui.add_space(6.0);

        // карточка: статус и кнопка
        let installing = self.installing.load(Ordering::SeqCst);
        let running = self.running.load(Ordering::SeqCst);
        let (label, enabled) = if installing {
            if self.cancelling { ("ОТМЕНА...", false) } else { ("ОТМЕНА", true) }
        } else if running {
            ("ЗАПУСК...", false)
        } else if self.installed {
            ("ИГРАТЬ", true)
        } else {
            ("УСТАНОВИТЬ", true)
        };
        let sub = format!("Minecraft {}  ·  {}", self.ver, self.loader);
        let status = self.prog.status();
        let frac = self.prog.frac();
        let bold = self.bold.clone();
        let working = installing || running;
        card(ui, |ui| {
            // статус и полоска нужны только пока идёт установка или запуск
            if working {
                ui.label(muted(status));
                bar(ui, inner_w, frac, ACCENT);
                ui.add_space(4.0);
            }
            if play_button(ui, inner_w, label, &sub, enabled, &bold) {
                *act = Some(Act::Main);
            }
        });
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut s = match self.settings.clone() {
            Some(s) => s,
            None => return,
        };
        let bold = self.bold.clone();
        let fb = move |sz: f32| FontId::new(sz, bold.clone());
        let (mut save, mut close, mut reset, mut clear, mut logs) = (false, false, false, false, false);
        let cur_bg = match self.cfg.bg_mode.as_str() {
            "none" => "none",
            "custom" => "custom",
            _ => "image",
        }
        .to_string();
        let mut bg_pick: Option<&'static str> = None;
        let mut bg_choose = false;
        let has_path = !self.cfg.bg_path.is_empty();
        let picking = self.bg_picking;
        let path_name = {
            let n = std::path::Path::new(&self.cfg.bg_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if n.chars().count() > 24 {
                let head: String = n.chars().take(22).collect();
                format!("{head}…")
            } else {
                n
            }
        };

        egui::Window::new("settings")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.set_width(820.0);
                ui.label(RichText::new("НАСТРОЙКИ").font(fb(11.0)).color(ACCENT));
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(410.0);
                        ui.set_max_width(410.0);
                        ui.label(RichText::new("Оперативная память для игры").font(fb(13.0)));
                        ui.label(
                            RichText::new(format!("{} МБ  ({:.1} ГБ)", s.ram, s.ram as f32 / 1024.0))
                                .font(fb(20.0))
                                .color(ACCENT_H),
                        );
                        ui.spacing_mut().slider_width = 380.0;
                        let before = s.ram;
                        let (lo, top) = (s.lo, s.top);
                        ui.add(egui::Slider::new(&mut s.ram, lo..=top).step_by(128.0).show_value(false));
                        if s.ram != before {
                            s.ram_text = s.ram.to_string();
                        }
                        ui.horizontal(|ui| {
                            ui.label(muted("Или впишите число сами:"));
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut s.ram_text)
                                    .desired_width(60.0)
                                    .horizontal_align(Align::Center),
                            );
                            ui.label(muted("МБ"));
                            if r.changed() {
                                if let Ok(n) = s.ram_text.trim().parse::<u32>() {
                                    s.ram = n.clamp(lo, top);
                                }
                            }
                            if r.lost_focus() {
                                s.ram_text = s.ram.to_string();
                            }
                        });
                        let (rlo, rhi, _) = engine::ram_advice();
                        if s.has_ram && s.ram as f32 > s.total as f32 * 0.8 {
                            ui.label(
                                RichText::new("Много: Windows и другим программам останется мало памяти, возможны тормоза.")
                                    .color(AMBER),
                            );
                        } else {
                            ui.label(muted(format!(
                                "Всего в ноутбуке {:.1} ГБ. Для вашего ноутбука разумно {}-{} МБ.",
                                s.total as f32 / 1024.0,
                                rlo,
                                rhi
                            )));
                        }
                        ui.add_space(12.0);
                        ui.label(RichText::new("Фон лаунчера").font(fb(12.0)));
                        ui.horizontal(|ui| {
                            for (key, text) in [("image", "Авто фон"), ("custom", "Свой фон"), ("none", "Без фона")] {
                                let on = cur_bg == key;
                                let label = RichText::new(text).color(if on { Color32::WHITE } else { FG });
                                let b = egui::Button::new(label)
                                    .fill(if on { ACCENT } else { BG })
                                    .min_size(vec2(104.0, 26.0));
                                if ui.add_enabled(!picking, b).clicked() {
                                    if key == "custom" && !has_path {
                                        bg_choose = true;
                                    } else {
                                        bg_pick = Some(key);
                                    }
                                }
                            }
                        });
                        if cur_bg == "custom" {
                            ui.horizontal(|ui| {
                                let txt = if picking { "Выбираю файл..." } else { "Выбрать другую картинку..." };
                                if ui.add_enabled(!picking, egui::Button::new(txt).fill(BG)).clicked() {
                                    bg_choose = true;
                                }
                                if !path_name.is_empty() {
                                    ui.label(muted(path_name.as_str()).size(12.0));
                                }
                            });
                            ui.label(muted("Подойдут PNG и JPG. Картинка растягивается на всё окно.").size(12.0));
                        }
                    });
                    ui.add_space(28.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Лаунчер").font(fb(12.0)));
                        ui.checkbox(&mut s.close_after, "Закрывать лаунчер после запуска игры");
                        ui.add_space(8.0);
                        ui.label(RichText::new("Размер окна лаунчера").font(fb(12.0)));
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut s.size).desired_width(90.0));
                            egui::ComboBox::from_id_source("sizes")
                                .selected_text("▾")
                                .width(24.0)
                                .show_ui(ui, |ui| {
                                    for z in SIZES {
                                        if ui.selectable_label(false, z).clicked() {
                                            s.size = z.to_string();
                                        }
                                    }
                                });
                            ui.label(muted(format!("или свой (минимум {}x{})", WIN_W, WIN_H)));
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new("Дополнительные аргументы Java").font(fb(12.0)));
                        ui.add(egui::TextEdit::singleline(&mut s.jvm).desired_width(340.0));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Обслуживание").font(fb(12.0)));
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new("Очистить логи и краш-репорты").fill(BG)).clicked() {
                                clear = true;
                            }
                            if ui.add(egui::Button::new("Папка логов").fill(BG)).clicked() {
                                logs = true;
                            }
                        });
                    });
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let b = egui::Button::new(RichText::new("Сохранить").color(Color32::WHITE))
                        .fill(ACCENT)
                        .min_size(vec2(110.0, 26.0));
                    if ui.add(b).clicked() {
                        save = true;
                    }
                    if ui.add(egui::Button::new("По умолчанию").fill(BG)).clicked() {
                        reset = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("Закрыть").fill(BG)).clicked() {
                            close = true;
                        }
                    });
                });
            });

        if let Some(b) = bg_pick {
            self.cfg.bg_mode = b.to_string();
            engine::save_cfg(&self.cfg);
            if b == "custom" {
                if self.custom_bg.is_none() && !self.cfg.bg_path.is_empty() {
                    spawn_bg_decode(self.shared.clone(), ctx.clone(), self.cfg.bg_path.clone());
                }
            } else {
                self.custom_bg = None; // освобождаем память
            }
        }
        if bg_choose && !self.bg_picking {
            self.bg_picking = true;
            spawn_bg_pick(self.shared.clone(), ctx.clone());
        }
        if reset {
            s.size = format!("{WIN_W}x{WIN_H}");
            s.ram = engine::default_ram().clamp(s.lo, s.top);
            s.ram_text = s.ram.to_string();
        }
        if save {
            self.save_settings(ctx, &s);
            return;
        }
        if close {
            self.settings = None;
            return;
        }
        self.settings = Some(s);
        if clear {
            self.ask_clear_logs();
        }
        if logs {
            sys::open_dir(&sys::mc_dir().join("logs"));
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context) {
        let d = match self.dialog.clone() {
            Some(d) => d,
            None => return,
        };
        let title_font = self.bold_font(14.0);
        let (mut yes, mut no) = (false, false);
        egui::Window::new("dialog")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(16.0))
                    .stroke(Stroke::new(1.0, ACCENT)),
            )
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.set_max_width(380.0);
                ui.label(RichText::new(d.title.as_str()).font(title_font).color(ACCENT_H));
                ui.add_space(6.0);
                ui.label(d.text.as_str());
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if d.confirm.is_some() {
                        if ui.add(egui::Button::new("Да").fill(ACCENT).min_size(vec2(70.0, 24.0))).clicked() {
                            yes = true;
                        }
                        if ui.add(egui::Button::new("Нет").fill(BG).min_size(vec2(70.0, 24.0))).clicked() {
                            no = true;
                        }
                    } else if ui.add(egui::Button::new("OK").fill(ACCENT).min_size(vec2(70.0, 24.0))).clicked() {
                        no = true;
                    }
                });
            });
        if yes || no {
            self.dialog = None;
        }
        if yes {
            match d.confirm {
                Some(Confirm::LowRam) => self.start_play(),
                Some(Confirm::ClearLogs) => {
                    let size = engine::dir_size("logs") + engine::dir_size("crash-reports");
                    engine::clear_logs();
                    self.info("Очистка", &format!("Освобождено {:.1} МБ", size as f64 / 1_048_576.0));
                }
                None => {}
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.toggle_fs(ctx);
        }
        if self.fs && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.toggle_fs(ctx);
        }
        if self.ram_at.elapsed() > Duration::from_millis(1900) {
            self.ram = sys::ram_info();
            self.ram_at = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        self.sync_state();

        let screen = ctx.screen_rect();
        let x0 = screen.right() - 14.0 - COL_W;
        let mut act: Option<Act> = None;

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_COL))
            .show(ctx, |ui| {
                let full = ui.max_rect();
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                // режимы фона: "image" (кнопка "Авто фон") - картинка как есть, "custom" ("Свой фон") - картинка пользователя на всё окно,
                // "none" ("Без фона") - только фиолетовый цвет
                let bg_mode = self.cfg.bg_mode.as_str();
                if bg_mode == "custom" {
                    if let Some(t) = &self.custom_bg {
                        let ts = t.size_vec2();
                        let k = (full.width() / ts.x).max(full.height() / ts.y);
                        let r = Rect::from_center_size(full.center(), ts * k);
                        ui.painter().with_clip_rect(full).image(t.id(), r, uv, Color32::WHITE);
                    }
                } else if bg_mode != "none" {
                    if let Some(t) = &self.girl {
                        let size = t.size_vec2();
                        let r = Rect::from_min_size(full.min, size);
                        ui.painter().image(t.id(), r, uv, Color32::WHITE);
                    }
                }
                // клик по свободному фону: волна
                let bg = ui.interact(full, egui::Id::new("bg"), egui::Sense::click());
                if bg.clicked() {
                    if let Some(p) = bg.interact_pointer_pos() {
                        self.ripple = Some((p, Instant::now()));
                    }
                }
                if let Some((pos, t0)) = self.ripple {
                    let t = t0.elapsed().as_secs_f32() / 0.256;
                    if t >= 1.0 {
                        self.ripple = None;
                    } else {
                        let rings = [(8.0 + 40.0 * t, Color32::from_rgb(0x6f, 0x4f, 0xe0)), (3.0 + 26.0 * t, Color32::WHITE)];
                        for (k, (rad, col)) in rings.iter().enumerate() {
                            let wd = (3.0 * (1.0 - t)).floor().max(1.0) + if k == 0 { 1.0 } else { 0.0 };
                            ui.painter().circle_stroke(pos, *rad, Stroke::new(wd, mix(*col, BG_COL, t * 0.9)));
                        }
                        ctx.request_repaint();
                    }
                }
                // логотип над колонкой
                if let Some(l) = &self.logo {
                    let r = Rect::from_min_size(pos2(x0, 14.0), vec2(32.0, 32.0));
                    ui.painter().image(l.id(), r, uv, Color32::WHITE);
                }
                let f = self.bold_font(22.0);
                ui.painter().text(
                    pos2(x0 + 42.0, 32.0),
                    Align2::LEFT_CENTER,
                    "rustx",
                    f.clone(),
                    Color32::from_rgb(0x2a, 0x1a, 0x5e),
                );
                ui.painter().text(pos2(x0 + 40.0, 30.0), Align2::LEFT_CENTER, "rustx", f, Color32::WHITE);
            });

        // плашка профиля справа сверху
        let ely = self.ely_name();
        let prof_name = match &ely {
            Some(n) => n.clone(),
            None => {
                let n = self.nick.trim();
                if n.is_empty() { "Player".to_string() } else { n.to_string() }
            }
        };
        let prof_sub = if ely.is_some() { "Аккаунт Ely.by" } else { "Офлайн-режим" };
        let avatar = if ely.is_some() && self.skin.is_some() && Some(&self.avatar_for) == ely.as_ref() {
            self.skin.as_ref()
        } else {
            self.logo.as_ref()
        };
        let avatar_id = avatar.map(|t| t.id());
        let name_font = self.bold_font(14.0);
        egui::Area::new(egui::Id::new("profile"))
            .anchor(Align2::RIGHT_TOP, vec2(-14.0, 10.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(8.0, 5.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(id) = avatar_id {
                            ui.add(egui::Image::new(egui::load::SizedTexture::new(id, vec2(32.0, 32.0))));
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(prof_name.as_str()).font(name_font).color(FG));
                            ui.label(muted(prof_sub).size(11.0));
                        });
                        ui.add_space(6.0);
                    });
                });
            });

        // правая колонка меню
        egui::Area::new(egui::Id::new("column"))
            .fixed_pos(pos2(x0, 58.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                self.column(ui, &mut act);
            });

        self.settings_ui(ctx);
        self.dialog_ui(ctx);

        match act {
            Some(Act::Main) => self.on_main(),
            Some(Act::Ely) => self.do_ely(),
            Some(Act::Logout) => self.ely_logout(),
            Some(Act::Settings) => self.open_settings(),
            Some(Act::OpenGame) => {
                let mc = sys::mc_dir();
                for d in ["mods", "shaderpacks", "resourcepacks", "config", "saves", "screenshots"] {
                    let _ = std::fs::create_dir_all(mc.join(d));
                }
                sys::open_dir(&mc);
            }
            Some(Act::OpenMods) => sys::open_dir(&sys::mc_dir().join("mods")),
            Some(Act::OpenRes) => sys::open_dir(&sys::mc_dir().join("resourcepacks")),
            None => {}
        }
    }
}

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("icon64.png")).ok()?.to_rgba8();
    Some(egui::IconData { rgba: img.to_vec(), width: img.width(), height: img.height() })
}

fn main() -> eframe::Result<()> {
    let cfg = engine::load_cfg();
    let (w, h) = parse_size(&cfg.win_size)
        .map(|(w, h)| (w.max(WIN_W), h.max(WIN_H)))
        .unwrap_or((WIN_W, WIN_H));
    let mut vp = egui::ViewportBuilder::default()
        .with_title("rustx")
        .with_inner_size([w as f32, h as f32])
        .with_min_inner_size([900.0, 540.0]);
    if let Some(icon) = load_icon() {
        vp = vp.with_icon(icon);
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("rustx", opts, Box::new(|cc| Ok(Box::new(App::new(cc, cfg)))))
}
