//! Hardware self-test for the real engine. Run on the Windows PC:
//!
//! ```text
//! cargo run -p warble-engine --example selftest -- 10
//! ```
//!
//! Lists the audio devices, starts the engine with the defaults (VB-CABLE auto-detected) and
//! the Robot preset, then prints its status once a second for N seconds (default 5).
//! Monitoring stays off and at zero volume, so nothing plays through your speakers or
//! headphones; the robot voice goes only to VB-CABLE. To hear it, open Windows Sound settings >
//! Recording > "CABLE Output" > Listen, or pick "CABLE Output" as the input in Discord.

use std::time::Duration;

use warble_engine::{Config, Engine, Preset, find_virtual_cable, list_devices};

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5);

    let devices = list_devices();
    let default = |d: bool| if d { "  (default)" } else { "" };
    println!("Inputs:");
    for d in &devices.inputs {
        println!("  {}{}", d.name, default(d.is_default));
    }
    println!("Outputs:");
    for d in &devices.outputs {
        println!("  {}{}", d.name, default(d.is_default));
    }
    println!(
        "Virtual cable: {}",
        find_virtual_cable(&devices)
            .as_deref()
            .unwrap_or("not found")
    );
    println!();

    let engine = Engine::start(Config::default());
    engine.set_monitor(false);
    engine.set_monitor_volume(0.0);
    engine.set_preset(Preset::Robot);

    for second in 1..=seconds {
        std::thread::sleep(Duration::from_secs(1));
        let s = engine.status();
        let name = |n: &Option<String>| n.clone().unwrap_or_else(|| "-".into());
        println!(
            "[{second:>3}s] running={} mic={} cable={} monitor={} in={:.2} out={:.2} underruns={} error={}",
            s.running,
            name(&s.input_name),
            name(&s.cable_name),
            name(&s.monitor_name),
            s.input_level,
            s.output_level,
            s.underruns,
            s.error.as_deref().unwrap_or("none"),
        );
    }
}
