use crate::keymap;
use crate::protocol;
use crate::wallpaper;
use anyhow::Result;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Command {
    SetColor([u8; 3]),
    SetWallpaperColors(Vec<(u8, [u8; 3])>),
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Connected,
    Disconnected,
    PermissionDenied(String),
    CommandOk(String),
    CommandErr(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    profiles: Vec<Profile>,
}

pub struct AppState {
    pub connected: bool,
    pub permission_denied: Option<String>,
    pub color: [f32; 3],
    pub last_sent: [u8; 3],
    pub last_change: Instant,
    pub profiles: Vec<Profile>,
    pub profile_name: String,
    pub status: String,
    pub config_path: PathBuf,
    pub wallpaper_path: Option<PathBuf>,
    pub wallpaper_grid: Option<Vec<Vec<[u8; 3]>>>,
    pub wallpaper_active: bool,
}

impl AppState {
    pub fn new() -> Self {
        let config_path = config_path();
        let profiles = load_config(&config_path).profiles;
        let wallpaper_path = wallpaper::detect();
        let wallpaper_grid = wallpaper_path
            .as_ref()
            .and_then(|p| wallpaper::sample_grid(p, keymap::COLS, keymap::ROWS));
        Self {
            connected: false,
            permission_denied: None,
            color: [0.0, 0.0, 0.0],
            last_sent: [0, 0, 0],
            last_change: Instant::now(),
            profiles,
            profile_name: String::new(),
            status: "Starting…".into(),
            config_path,
            wallpaper_path,
            wallpaper_grid,
            wallpaper_active: false,
        }
    }

    pub fn key_colors(&self) -> Vec<Vec<[u8; 3]>> {
        if self.wallpaper_active {
            if let Some(g) = &self.wallpaper_grid {
                return g.clone();
            }
        }
        let c = self.current_color_u8();
        (0..keymap::ROWS)
            .map(|_| (0..keymap::COLS).map(|_| c).collect())
            .collect()
    }

    pub fn current_color_u8(&self) -> [u8; 3] {
        [
            (self.color[0] * 255.0) as u8,
            (self.color[1] * 255.0) as u8,
            (self.color[2] * 255.0) as u8,
        ]
    }
}

pub struct RivalDeckApp {
    cmd_tx: Sender<Command>,
    evt_rx: Receiver<DeviceEvent>,
    state: AppState,
}

impl RivalDeckApp {
    pub fn new(cmd_tx: Sender<Command>, evt_rx: Receiver<DeviceEvent>) -> Self {
        Self {
            cmd_tx,
            evt_rx,
            state: AppState::new(),
        }
    }
}

fn config_path() -> PathBuf {
    let mut p = dirs_or_home();
    p.push(".config");
    p.push("steelseries-rr");
    let _ = std::fs::create_dir_all(&p);
    p.push("config.toml");
    p
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn load_config(path: &PathBuf) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(path: &PathBuf, cfg: &Config) {
    if let Ok(s) = toml::to_string_pretty(cfg) {
        let _ = std::fs::write(path, s);
    }
}

pub fn run_ui(vid: u16, pid: u16) -> Result<()> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (evt_tx, evt_rx) = mpsc::channel::<DeviceEvent>();

    // Core thread
    std::thread::spawn(move || {
        core_loop(cmd_rx, evt_tx, vid, pid);
    });

    let options = eframe::NativeOptions::default();
    let app = RivalDeckApp::new(cmd_tx, evt_rx);
    eframe::run_native(
        "SteelSeries KLC",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;
    Ok(())
}

fn core_loop(cmd_rx: Receiver<Command>, evt_tx: Sender<DeviceEvent>, vid: u16, pid: u16) {
    let mut device: Option<hidapi::HidDevice> = None;

    // initial connect
    match connect(vid, pid) {
        Ok(d) => {
            device = Some(d);
            let _ = evt_tx.send(DeviceEvent::Connected);
        }
        Err(e) => {
            if e.to_lowercase().contains("permission") || e.to_lowercase().contains("open") {
                let _ = evt_tx.send(DeviceEvent::PermissionDenied(e));
            } else {
                let _ = evt_tx.send(DeviceEvent::Disconnected);
            }
        }
    }

    while let Ok(cmd) = cmd_rx.recv() {
        if device.is_none() {
            match connect(vid, pid) {
                Ok(d) => {
                    device = Some(d);
                    let _ = evt_tx.send(DeviceEvent::Connected);
                }
                Err(e) => {
                    let _ = evt_tx.send(DeviceEvent::CommandErr(format!("not connected: {e}")));
                    continue;
                }
            }
        }
        let dev = device.as_ref().unwrap();
        match cmd {
            Command::SetColor(c) => {
                let mut ok = true;
                for region in ["alphanum", "enter", "modifiers", "numpad"] {
                    let pkt = protocol::make_key_colors_packet(region, c);
                    if let Err(e) = dev.send_feature_report(&pkt) {
                        ok = false;
                        let _ = evt_tx.send(DeviceEvent::CommandErr(e.to_string()));
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                if ok {
                    let refresh = protocol::make_refresh_packet();
                    if let Err(e) = dev.write(&refresh) {
                        let _ = evt_tx.send(DeviceEvent::CommandErr(e.to_string()));
                    } else {
                        let _ = evt_tx.send(DeviceEvent::CommandOk("color applied".into()));
                    }
                }
            }
            Command::SetWallpaperColors(entries) => {
                let pkt = protocol::make_keys_packet("alphanum", &entries);
                match dev.send_feature_report(&pkt) {
                    Ok(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        let refresh = protocol::make_refresh_packet();
                        if let Err(e) = dev.write(&refresh) {
                            let _ = evt_tx.send(DeviceEvent::CommandErr(e.to_string()));
                        } else {
                            let _ = evt_tx.send(DeviceEvent::CommandOk("wallpaper applied".into()));
                        }
                    }
                    Err(e) => {
                        let _ = evt_tx.send(DeviceEvent::CommandErr(e.to_string()));
                    }
                }
            }
        }
    }
    // channel closed
    let _ = evt_tx.send(DeviceEvent::Disconnected);
}

fn connect(vid: u16, pid: u16) -> Result<hidapi::HidDevice, String> {
    let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
    api.open(vid, pid).map_err(|e| e.to_string())
}

impl eframe::App for RivalDeckApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        setup_style(ctx);

        // drain device events
        while let Ok(ev) = self.evt_rx.try_recv() {
            match ev {
                DeviceEvent::Connected => {
                    self.state.connected = true;
                    self.state.permission_denied = None;
                    self.state.status = "Connected".into();
                }
                DeviceEvent::Disconnected => {
                    self.state.connected = false;
                    self.state.status = "Device not connected".into();
                }
                DeviceEvent::PermissionDenied(e) => {
                    self.state.connected = false;
                    self.state.permission_denied = Some(e);
                    self.state.status = "Permission denied".into();
                }
                DeviceEvent::CommandOk(m) => self.state.status = m,
                DeviceEvent::CommandErr(e) => self.state.status = format!("Error: {e}"),
            }
        }

        // debounce solid-color changes
        if !self.state.wallpaper_active {
            let cur = self.state.current_color_u8();
            if cur != self.state.last_sent
                && self.state.last_change.elapsed() >= Duration::from_millis(150)
            {
                let _ = self.cmd_tx.send(Command::SetColor(cur));
                self.state.last_sent = cur;
            }
        }

        // Header
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            egui::Frame {
                inner_margin: egui::Margin::symmetric(18.0, 12.0),
                fill: egui::Color32::from_hex("#14161b").unwrap(),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        egui::RichText::new("RivalDeck")
                            .size(20.0)
                            .color(egui::Color32::from_hex("#f2f4f8").unwrap()),
                    );
                    ui.add_space(8.0);
                    let dot = if self.state.connected {
                        egui::Color32::from_hex("#43d17a").unwrap()
                    } else {
                        egui::Color32::from_hex("#6b7280").unwrap()
                    };
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(r.center(), 5.0, dot);
                    ui.label(
                        egui::RichText::new(&self.state.status)
                            .color(egui::Color32::from_hex("#9aa4b2").unwrap()),
                    );
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(14.0);

                // Device + keyboard preview
                card(ui, "SteelSeries KLC", |ui| {
                    ui.label(
                        egui::RichText::new("Per-key lighting preview")
                            .color(egui::Color32::from_hex("#9aa4b2").unwrap()),
                    );
                    ui.add_space(8.0);
                    let colors = self.state.key_colors();
                    draw_keyboard(ui, &colors);
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("1038:1122  •  HID")
                            .monospace()
                            .color(egui::Color32::from_hex("#5f6875").unwrap()),
                    );
                });

                ui.add_space(12.0);

                // Lighting
                card(ui, "Lighting", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Solid color");
                        if ui.color_edit_button_rgb(&mut self.state.color).changed() {
                            self.state.wallpaper_active = false;
                            self.state.last_change = Instant::now();
                        }
                        if ui.button("Apply color").clicked() {
                            self.state.wallpaper_active = false;
                            let c = self.state.current_color_u8();
                            let _ = self.cmd_tx.send(Command::SetColor(c));
                            self.state.last_sent = c;
                        }
                    });

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new("Wallpaper sync")
                            .strong()
                            .color(egui::Color32::from_hex("#e6e9ef").unwrap()),
                    );
                    ui.label(
                        egui::RichText::new("Map the colors of your desktop wallpaper onto the keys.")
                            .color(egui::Color32::from_hex("#9aa4b2").unwrap()),
                    );
                    if let Some(p) = &self.state.wallpaper_path {
                        ui.label(
                            egui::RichText::new(p.display().to_string())
                                .monospace()
                                .color(egui::Color32::from_hex("#5f6875").unwrap()),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No wallpaper detected (GNOME/XFCE supported).")
                                .color(egui::Color32::from_hex("#b5884a").unwrap()),
                        );
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Apply wallpaper to keys").clicked() {
                            if let Some(path) = self.state.wallpaper_path.clone() {
                                if let Some(grid) = wallpaper::sample_grid(&path, keymap::COLS, keymap::ROWS) {
                                    let mut entries = Vec::new();
                                    for (r, row) in keymap::KEY_ROWS.iter().enumerate() {
                                        for (c, kc) in row.iter().enumerate() {
                                            if let Some(col) = grid.get(r).and_then(|g| g.get(c)) {
                                                entries.push((*kc, *col));
                                            }
                                        }
                                    }
                                    self.state.wallpaper_grid = Some(grid);
                                    self.state.wallpaper_active = true;
                                    let _ = self.cmd_tx.send(Command::SetWallpaperColors(entries));
                                    self.state.status = "Wallpaper applied".into();
                                }
                            }
                        }
                        if self.state.wallpaper_active && ui.button("Back to solid color").clicked() {
                            self.state.wallpaper_active = false;
                            let c = self.state.current_color_u8();
                            let _ = self.cmd_tx.send(Command::SetColor(c));
                            self.state.last_sent = c;
                        }
                    });
                });

                ui.add_space(12.0);

                // Profiles
                card(ui, "Profiles", |ui| {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.state.profile_name);
                        if ui.button("Save current").clicked() && !self.state.profile_name.is_empty() {
                            let p = Profile {
                                name: self.state.profile_name.clone(),
                                color: self.state.current_color_u8(),
                            };
                            self.state.profiles.retain(|x| x.name != p.name);
                            self.state.profiles.push(p.clone());
                            save_config(
                                &self.state.config_path,
                                &Config { profiles: self.state.profiles.clone() },
                            );
                            self.state.wallpaper_active = false;
                            let _ = self.cmd_tx.send(Command::SetColor(p.color));
                            self.state.last_sent = p.color;
                        }
                    });

                    ui.add_space(8.0);
                    if self.state.profiles.is_empty() {
                        ui.label(
                            egui::RichText::new("No profiles yet. Name a color and save it.")
                                .color(egui::Color32::from_hex("#7d8794").unwrap()),
                        );
                    }
                    let mut to_load: Option<Profile> = None;
                    let mut to_delete: Option<String> = None;
                    for p in &self.state.profiles {
                        ui.horizontal(|ui| {
                            let swatch = egui::Color32::from_rgb(p.color[0], p.color[1], p.color[2]);
                            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 4.0, swatch);
                            ui.label(&p.name);
                            if ui.button("Load").clicked() {
                                to_load = Some(p.clone());
                            }
                            if ui.button("Delete").clicked() {
                                to_delete = Some(p.name.clone());
                            }
                        });
                    }
                    if let Some(p) = to_load {
                        self.state.color = [
                            p.color[0] as f32 / 255.0,
                            p.color[1] as f32 / 255.0,
                            p.color[2] as f32 / 255.0,
                        ];
                        self.state.wallpaper_active = false;
                        let _ = self.cmd_tx.send(Command::SetColor(p.color));
                        self.state.last_sent = p.color;
                        self.state.last_change = Instant::now();
                    }
                    if let Some(name) = to_delete {
                        self.state.profiles.retain(|x| x.name != name);
                        save_config(
                            &self.state.config_path,
                            &Config { profiles: self.state.profiles.clone() },
                        );
                    }
                });

                ui.add_space(12.0);

                // Settings / errors
                if let Some(e) = &self.state.permission_denied {
                    card(ui, "Permission denied", |ui| {
                        ui.label("Missing udev rule. Add:");
                        ui.monospace("KERNEL==\"hidraw*\", SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"1038\", ATTRS{idProduct}==\"1122\", MODE=\"0666\"");
                        ui.label(format!("{e}"));
                    });
                }

                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Config: {}", self.state.config_path.display()))
                        .monospace()
                        .color(egui::Color32::from_hex("#4f5866").unwrap()),
                );
                ui.add_space(14.0);
            });
        });

        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.window_fill = egui::Color32::from_hex("#0f1115").unwrap();
    style.visuals.panel_fill = egui::Color32::from_hex("#0f1115").unwrap();
    style.visuals.override_text_color = Some(egui::Color32::from_hex("#e6e9ef").unwrap());
    style.visuals.selection.bg_fill = egui::Color32::from_hex("#5b8cff").unwrap();
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_hex("#242833").unwrap();
    style.visuals.widgets.active.bg_fill = egui::Color32::from_hex("#2f3542").unwrap();
    ctx.set_style(style);
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame {
        inner_margin: egui::Margin::same(16.0),
        rounding: egui::Rounding::same(12.0),
        fill: egui::Color32::from_hex("#181b21").unwrap(),
        stroke: egui::Stroke::new(1.0_f32, egui::Color32::from_hex("#262a33").unwrap()),
        ..Default::default()
    }
    .show(ui, |ui| {
        ui.heading(
            egui::RichText::new(title)
                .size(16.0)
                .color(egui::Color32::from_hex("#f2f4f8").unwrap()),
        );
        ui.add_space(8.0);
        add(ui);
    });
}

fn draw_keyboard(ui: &mut egui::Ui, colors: &[Vec<[u8; 3]>]) {
    let cols = keymap::COLS as usize;
    let rows = keymap::ROWS as usize;
    let width = ui.available_width().min(560.0);
    let gap = 3.0;
    let key_w = (width - gap * (cols - 1) as f32) / cols as f32;
    let key_h = key_w * 0.92;
    let total_h = rows as f32 * key_h + (rows - 1) as f32 * gap;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());

    for r in 0..rows {
        for c in 0..cols {
            let col = colors
                .get(r)
                .and_then(|row| row.get(c))
                .copied()
                .unwrap_or([36, 39, 46]);
            let x = rect.min.x + c as f32 * (key_w + gap);
            let y = rect.min.y + r as f32 * (key_h + gap);
            let rr = egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(key_w, key_h),
            );
            ui.painter().rect_filled(rr, egui::Rounding::same(4.0), egui::Color32::from_rgb(col[0], col[1], col[2]));
        }
    }
}
