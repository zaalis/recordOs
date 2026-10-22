#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use vp_agent::agent::Agent;
use vp_agent::config::Settings;
use vp_agent::store::Permission;

type SharedAgent = Arc<Agent>;

fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_status(
    app: tauri::AppHandle,
    agent: tauri::State<'_, SharedAgent>,
) -> Result<serde_json::Value, String> {
    let mut status = json(agent.status())?;
    status["version"] = serde_json::json!(app.package_info().version.to_string());
    Ok(status)
}

#[tauri::command]
fn get_settings(agent: tauri::State<'_, SharedAgent>) -> Result<serde_json::Value, String> {
    json(agent.settings())
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    agent: tauri::State<'_, SharedAgent>,
    settings: serde_json::Value,
) -> Result<(), String> {
    let settings: Settings = serde_json::from_value(settings).map_err(|error| error.to_string())?;
    let previous = agent.settings();

    // Autostart is a separate Windows resource. Calling `disable` every time a
    // completely unrelated setting changes fails with ERROR_FILE_NOT_FOUND when
    // Windows has no existing startup entry, and used to prevent every setting
    // from being persisted. Only touch it when the user actually toggles it.
    if previous.advanced.launch_at_login != settings.advanced.launch_at_login {
        let autolaunch = app.autolaunch();
        let enabled = autolaunch.is_enabled().map_err(|error| error.to_string())?;
        if settings.advanced.launch_at_login && !enabled {
            autolaunch.enable().map_err(|error| error.to_string())?;
        } else if !settings.advanced.launch_at_login && enabled {
            autolaunch.disable().map_err(|error| error.to_string())?;
        }
    }
    agent
        .update_settings(settings)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_devices(agent: tauri::State<'_, SharedAgent>) -> Result<serde_json::Value, String> {
    json(agent.devices())
}

#[tauri::command]
fn get_audit(
    agent: tauri::State<'_, SharedAgent>,
    limit: usize,
) -> Result<serde_json::Value, String> {
    json(agent.audit(limit))
}

#[tauri::command]
fn set_service(agent: tauri::State<'_, SharedAgent>, running: bool) -> Result<(), String> {
    if running {
        agent.start_service().map_err(|error| error.to_string())
    } else {
        agent.stop_service();
        Ok(())
    }
}

#[tauri::command]
fn open_connect(
    agent: tauri::State<'_, SharedAgent>,
    refresh: bool,
) -> Result<serde_json::Value, String> {
    json(
        agent
            .open_connect_window(refresh)
            .map_err(|error| error.to_string())?,
    )
}

#[tauri::command]
fn close_connect(agent: tauri::State<'_, SharedAgent>) {
    agent.close_connect_window();
}

#[tauri::command]
fn respond_pairing(agent: tauri::State<'_, SharedAgent>, id: String, approve: bool) -> bool {
    agent.respond_pairing(&id, approve)
}

#[tauri::command]
fn device_action(
    agent: tauri::State<'_, SharedAgent>,
    id: String,
    action: String,
    value: Option<String>,
) -> Result<bool, String> {
    let changed = match action.as_str() {
        "trust" => agent.trust_device(&id),
        "rename" => agent.rename_device(&id, value.as_deref().unwrap_or("iPad")),
        "view" => agent.set_device_permission(&id, Permission::View),
        "control" => agent.set_device_permission(&id, Permission::Control),
        "disconnect" => agent.disconnect_device(&id) > 0,
        "revoke" => agent.revoke_device(&id),
        "forget" => agent.forget_device(&id),
        _ => return Err("Unknown device action".into()),
    };
    Ok(changed)
}

#[tauri::command]
fn emergency_disconnect(agent: tauri::State<'_, SharedAgent>) -> usize {
    agent.disconnect_all()
}

#[tauri::command]
fn unlock_sources(agent: tauri::State<'_, SharedAgent>) {
    agent.unlock_sources();
}

/// Asks Windows (UAC) to treat a tablet's cable network as private and allow Record OS on it.
#[tauri::command]
async fn allow_usb_link(agent: tauri::State<'_, SharedAgent>, id: String) -> Result<(), String> {
    let agent = agent.inner().clone();
    tauri::async_runtime::spawn_blocking(move || agent.allow_usb_link(&id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// Sends the agent's tracing output to `agent.<date>.log` in the `logs` folder of the Record OS
/// data directory (last seven days). The release app has no console, so without a file everything
/// it logs — encoder in use, stream statistics, warnings — is lost. Falls back to standard output
/// if the folder cannot be written. The returned guard flushes the file on exit and must stay
/// alive.
fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let appender = vp_agent::paths::ensure_data_dir().ok().and_then(|dir| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("agent")
            .filename_suffix("log")
            .max_log_files(7)
            .build(dir.join("logs"))
            .ok()
    });
    match appender {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_ansi(false)
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
            None
        }
    }
}

fn main() {
    let _log_guard = init_logging();

    let app = tauri::Builder::default()
        // The service owns a fixed LAN port. Letting a second desktop process
        // start would leave that window alive but without a reconnectable
        // service when the first process exits.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let agent = Arc::new(Agent::new(vp_agent::platform::control_engine())?);
            if agent.settings().advanced.start_service_on_launch {
                if let Err(error) = agent.start_service() {
                    eprintln!("Record OS service did not start: {error}");
                }
            }
            let mut events = agent.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let _ = handle.emit("agent-event", event);
                }
            });
            let emergency_agent = agent.clone();
            let emergency_shortcut = agent.settings().security.emergency_shortcut;
            app.global_shortcut().on_shortcut(
                emergency_shortcut.as_str(),
                move |handle, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let disconnected = emergency_agent.disconnect_all();
                        let _ = handle.emit("emergency-disconnect", disconnected);
                    }
                },
            )?;
            app.manage(agent);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_settings,
            save_settings,
            get_devices,
            get_audit,
            set_service,
            open_connect,
            close_connect,
            respond_pairing,
            device_action,
            emergency_disconnect,
            unlock_sources,
            allow_usb_link,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Record OS");

    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            handle.state::<SharedAgent>().shutdown();
        }
    });
}
