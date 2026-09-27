//! Headless Vision-Pro agent for development and diagnostics.
//!
//!   vp-agentd [--dev-loopback] [--connect]
//!
//! Pairing requests are printed to the console; type `y` or `n` and press Enter.
//! Debug builds also accept `--approve-all` for automated end-to-end tests.

use std::io::BufRead;
use std::sync::Arc;
use vp_agent::agent::Agent;
use vp_agent::events::AgentEvent;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,str0m=warn".into()),
        )
        .init();

    // macOS privacy permissions: without them capture and input are silently blocked by the OS.
    let permissions = vp_agent::platform::permissions::status();
    if !permissions.screen_recording {
        println!("!! Screen Recording is not allowed for this terminal/binary: System Settings → Privacy & Security → Screen & System Audio Recording.");
        vp_agent::platform::permissions::request(
            vp_agent::platform::permissions::Permission::ScreenRecording,
        );
    }
    if !permissions.accessibility {
        println!("!! Accessibility is not allowed for this terminal/binary (needed for remote input): System Settings → Privacy & Security → Accessibility.");
        vp_agent::platform::permissions::request(
            vp_agent::platform::permissions::Permission::Accessibility,
        );
    }

    let engine = vp_agent::platform::media_engine();
    let agent = Arc::new(Agent::new(engine)?);

    if has("--dev-loopback") {
        let mut settings = agent.settings();
        settings.advanced.developer_loopback = true;
        agent.update_settings(settings)?;
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
    println!(
        "  Encoders  {}",
        status
            .caps
            .encoders
            .iter()
            .map(|e| format!("{} ({})", e.name, e.codec.display()))
            .collect::<Vec<_>>()
            .join(", ")
    );

    if has("--connect") {
        let info = agent.open_connect_window(false)?;
        println!(
            "\nConnect your iPad\n  Code  {}\n  Link  {}",
            info.code, info.url
        );
        if let Some(setup) = &info.setup_url {
            println!("  Setup {setup}");
        }
    }

    let approve_all = cfg!(debug_assertions) && has("--approve-all");
    if approve_all {
        println!("\n!! --approve-all: every pairing request is accepted automatically (debug build only) !!");
    }

    let mut events = agent.subscribe();
    let events_agent = agent.clone();
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
            "s" => println!("{}", serde_json::to_string_pretty(&agent.status())?),
            _ => {}
        }
    }
    // stdin closed (e.g. background run): keep serving.
    loop {
        std::thread::park();
    }
}
