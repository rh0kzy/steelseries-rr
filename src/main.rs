use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hidapi::HidApi;
use std::thread;
use std::time::Duration;

mod keymap;
mod protocol;
mod ui;
mod wallpaper;

const DEFAULT_VID: u16 = 0x1038;
const DEFAULT_PID: u16 = 0x1122;

#[derive(Parser)]
#[command(name = "steelseries-rr", about = "Linux control for SteelSeries KLC RGB keyboards")]
struct Cli {
    /// USB vendor id (hex)
    #[arg(long, default_value_t = DEFAULT_VID)]
    vid: u16,
    /// USB product id (hex)
    #[arg(long, default_value_t = DEFAULT_PID)]
    pid: u16,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set the whole keyboard to a single color (hex, e.g. ff0000 or #ff0000)
    Set { color: String },
    /// Turn all lighting off
    Off,
    /// Random color per key
    Random,
    /// Latch/apply pending colors
    Refresh,
    /// List connected SteelSeries HID devices
    Devices,
    /// Launch the graphical UI
    Ui,
}

fn parse_color(s: &str) -> Result<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        anyhow::bail!("color must be 6 hex digits, got {s}");
    }
    let v = u32::from_str_radix(s, 16).context("invalid hex color")?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn open_device(vid: u16, pid: u16) -> Result<hidapi::HidDevice> {
    let api = HidApi::new().context("failed to init hidapi")?;
    let dev = api.open(vid, pid).context("failed to open keyboard (check permissions/udev)")?;
    Ok(dev)
}

fn send_all(dev: &hidapi::HidDevice, color: [u8; 3]) -> Result<()> {
    for region in ["alphanum", "enter", "modifiers", "numpad"] {
        let pkt = protocol::make_key_colors_packet(region, color);
        dev.send_feature_report(&pkt)
            .with_context(|| format!("failed to send feature report for {region}"))?;
        thread::sleep(Duration::from_millis(10));
    }
    let refresh = protocol::make_refresh_packet();
    dev.write(&refresh).context("failed to send refresh")?;
    thread::sleep(Duration::from_millis(10));
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Devices => {
            let api = HidApi::new()?;
            for dev in api.device_list() {
                if dev.vendor_id() == cli.vid || dev.vendor_id() == 0x1038 {
                    println!(
                        "{:04x}:{:04x} {} {}",
                        dev.vendor_id(),
                        dev.product_id(),
                        dev.manufacturer_string().unwrap_or("?"),
                        dev.product_string().unwrap_or("?")
                    );
                }
            }
            Ok(())
        }
        Commands::Set { color } => {
            let c = parse_color(&color)?;
            let dev = open_device(cli.vid, cli.pid)?;
            send_all(&dev, c)?;
            println!("set keyboard to #{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
            Ok(())
        }
        Commands::Off => {
            let dev = open_device(cli.vid, cli.pid)?;
            send_all(&dev, [0, 0, 0])?;
            println!("lighting off");
            Ok(())
        }
        Commands::Random => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
            let dev = open_device(cli.vid, cli.pid)?;
            // Simple per-region random: reuse send_all with one random color per call is boring;
            // instead build packets per key.
            for region in ["alphanum", "enter", "modifiers", "numpad"] {
                // generate a color per key slot
                let mut color = [0u8; 3];
                // We'll just send one random color for the whole region for simplicity.
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                color[0] = (seed >> 16) as u8;
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                color[1] = (seed >> 16) as u8;
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                color[2] = (seed >> 16) as u8;
                let pkt = protocol::make_key_colors_packet(region, color);
                dev.send_feature_report(&pkt)?;
                thread::sleep(Duration::from_millis(10));
            }
            let refresh = protocol::make_refresh_packet();
            dev.write(&refresh)?;
            thread::sleep(Duration::from_millis(10));
            println!("random colors applied");
            Ok(())
        }
        Commands::Refresh => {
            let dev = open_device(cli.vid, cli.pid)?;
            let refresh = protocol::make_refresh_packet();
            dev.write(&refresh)?;
            println!("refreshed");
            Ok(())
        }
        Commands::Ui => ui::run_ui(cli.vid, cli.pid),
    }
}