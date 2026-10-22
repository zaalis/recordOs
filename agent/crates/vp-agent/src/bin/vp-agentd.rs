//! Headless Vision-Pro agent for development and diagnostics.
//!
//!   vp-agentd [--dev-loopback] [--connect]
//!
//! Pairing requests are printed to the console; type `y` or `n` and press Enter.
//! Debug builds also accept `--approve-all` for automated end-to-end tests, and
//! `--simulate-usb` (with `--dev-loopback`): a tablet "cable" on 127.0.0.2 that `u` plugs/unplugs.

use std::io::BufRead;
use std::sync::Arc;
use vp_agent::agent::Agent;
use vp_agent::events::AgentEvent;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let engine = vp_agent::platform::control_engine();
    let agent = Arc::new(Agent::new(engine)?);

    if has("--dev-loopback") {
        let mut settings = agent.settings();
        settings.advanced.developer_loopback = true;
        agent.update_settings(settings)?;
    }
    let simulate_usb = cfg!(debug_assertions) && has("--simulate-usb");
    if simulate_usb {
        agent.simulate_usb(std::net::Ipv4Addr::new(127, 0, 0, 2));
    }
    agent.start_service()?;

    let status = agent.status();
    println!("\nVision-Pro {} — {}", status.version, status.computer_name);
    println!("  Agent     {}", status.agent_id);
    println!("  Key       {}", status.fingerprint);
    for address in &status.addresses {
        println!("  Listening {address}");
    }
    if agent.settings().advanced.developer_loopback {
        println!(
            "  Dev       http://127.0.0.1:{}",
            vp_agent::server::DEV_LOOPBACK_PORT
        );
    }
    if simulate_usb {
        println!(
            "  USB       simulated cable ws://127.0.0.2:{}/ws — type u to unplug/plug",
            vp_agent::server::DEV_LOOPBACK_PORT
        );
    }
    println!(
        "  Displays  {}",
        status
            .caps
            .displays
            .iter()
            .map(|d| format!("{} ({}×{})", d.name, d.width, d.height))
            .collect::<Vec<_>>()
            .join(", ")
    );

    if has("--connect") {
        let info = agent.open_connect_window(false)?;
        println!("\nConnect your iPad\n  Code  {}", info.code);
        for address in &info.addresses {
            println!("  Open  {} ({:?})", address.url, address.network);
        }
    }

    let approve_all = cfg!(debug_assertions) && has("--approve-all");
    if approve_all {
        println!("\n!! --approve-all: every pairing request is accepted automatically (debug build only) !!");
    }

    let mut events = agent.subscribe();
    let events_agent = agent.clone();
    let mut shown = agent.connect_code();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            while let Ok(event) = events.recv().await {
                match event {
                    AgentEvent::PairingRequested(request) => {
                        println!(
                            "\n>> {} ({}, {}) wants to connect [{}]. Code {} — allow? [y/n]",
                            request.device_name,
                            request.device_kind,
                            request.ip,
                            request.method,
                            request.sas
                        );
                        if approve_all {
                            events_agent.respond_pairing(&request.id, true);
                        }
                    }
                    AgentEvent::PairingResolved { outcome, .. } => println!(">> pairing {outcome}"),
                    AgentEvent::ConnectWindowChanged => {
                        // A used code is replaced: show the new one.
                        if let Some(code) = events_agent
                            .connect_code()
                            .filter(|c| Some(c) != shown.as_ref())
                        {
                            println!("  Code  {code}");
                            shown = Some(code);
                        }
                    }
                    AgentEvent::UsbChanged => {
                        let usb = events_agent.status().usb;
                        println!(
                            ">> usb: {} device(s), links [{}]",
                            usb.devices.len(),
                            usb.links
                                .iter()
                                .map(|l| format!("{} {}", l.device_name, l.url))
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                    AgentEvent::Audit(entry) => {
                        println!("   [{:?}] {}", entry.severity, entry.message)
                    }
                    _ => {}
                }
            }
        });
    });

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?.trim().to_lowercase();
        match line.as_str() {
            "q" | "quit" => {
                agent.shutdown();
                return Ok(());
            }
            "y" | "n" => {
                if let Some(pending) = agent.status().pending_pairing {
                    agent.respond_pairing(&pending.id, line == "y");
                }
            }
            "d" => {
                agent.disconnect_all();
            }
            "u" => match agent.toggle_simulated_usb() {
                Some(true) => println!(">> simulated USB cable plugged in"),
                Some(false) => println!(">> simulated USB cable unplugged"),
                None => println!(">> start with --simulate-usb to simulate a cable"),
            },
            "s" => println!("{}", serde_json::to_string_pretty(&agent.status())?),
            _ => {}
        }
    }
    // stdin closed (e.g. background run): keep serving.
    loop {
        std::thread::park();
    }
}
