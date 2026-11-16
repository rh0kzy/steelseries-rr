use crate::protocol;
use anyhow::Result;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Command {
    SetColor([u8; 3]),
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
}

impl AppState {
    pub fn new() -> Self {
        let config_path = config_path();
        let profiles = load_config(&config_path).profiles;
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
        }
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
        // drain events
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

        // debounce color send
        let cur = self.state.current_color_u8();
        if cur != self.state.last_sent
            && self.state.last_change.elapsed() >= Duration::from_millis(150)
        {
            let _ = self.cmd_tx.send(Command::SetColor(cur));
            self.state.last_sent = cur;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("SteelSeries KLC");
            ui.label(format!("Status: {}", self.state.status));

            if let Some(e) = &self.state.permission_denied {
                ui.colored_label(egui::Color32::RED, "Permission denied");
                ui.label("Missing udev rule. Add:");
                ui.monospace("KERNEL==\"hidraw*\", SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"1038\", ATTRS{idProduct}==\"1122\", MODE=\"0666\"");
                ui.label(format!("Error: {e}"));
            } else if !self.state.connected {
                ui.label("Device not connected. Plug in the keyboard.");
            }

            ui.separator();
            // live preview
            let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 60.0), egui::Sense::hover());
            let c = egui::Color32::from_rgb(
                (self.state.color[0] * 255.0) as u8,
                (self.state.color[1] * 255.0) as u8,
                (self.state.color[2] * 255.0) as u8,
            );
            ui.painter().rect_filled(rect, 4.0, c);
            ui.label("Lighting preview");

            ui.separator();
            ui.label("Color");
            if ui.color_edit_button_rgb(&mut self.state.color).changed() {
                self.state.last_change = Instant::now();
            }

            ui.separator();
            ui.label("Profiles");
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
                        &Config {
                            profiles: self.state.profiles.clone(),
                        },
                    );
                    let _ = self.cmd_tx.send(Command::SetColor(p.color));
                    self.state.last_sent = p.color;
                }
            });

            let mut to_load: Option<Profile> = None;
            let mut to_delete: Option<String> = None;
            for p in &self.state.profiles {
                ui.horizontal(|ui| {
                    let (r, g, b) = (p.color[0], p.color[1], p.color[2]);
                    let col = egui::Color32::from_rgb(r, g, b);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, col);
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
                let _ = self.cmd_tx.send(Command::SetColor(p.color));
                self.state.last_sent = p.color;
                self.state.last_change = Instant::now();
            }
            if let Some(name) = to_delete {
                self.state.profiles.retain(|x| x.name != name);
                save_config(
                    &self.state.config_path,
                    &Config {
                        profiles: self.state.profiles.clone(),
                    },
                );
            }

            ui.separator();
            ui.label(format!("Config: {}", self.state.config_path.display()));
        });
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}