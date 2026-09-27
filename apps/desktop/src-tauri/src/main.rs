//! Vision-Pro for macOS: the menu-bar-less desktop app that hosts the agent and its settings UI.

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use vp_agent::agent::Agent;
use vp_agent::config::Settings;
use vp_agent::platform::permissions;
use vp_agent::store::Permission;

type SharedAgent = Arc<Agent>;

fn json<T: serde::Serialize>(value: T) -> Result<serde_json::Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_status(agent: tauri::State<'_, SharedAgent>) -> Result<serde_json::Value, String> {
    json(agent.status())
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

    // "Open at login" is a separate LaunchAgent (~/Library/LaunchAgents). Only
    // touch it when the user actually toggles it, so an unrelated setting can
    // never fail because the login item is missing or managed elsewhere.
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

/// Screen Recording and Accessibility state (macOS privacy permissions).
#[tauri::command]
fn get_permissions() -> Result<serde_json::Value, String> {
    json(permissions::status())
}

/// Shows the system prompt when possible, otherwise opens the matching System Settings pane.
#[tauri::command]
fn request_permission(kind: String) -> Result<(), String> {
    let permission =
        permissions::Permission::parse(&kind).ok_or_else(|| "Unknown permission".to_string())?;
    permissions::request(permission);
    Ok(())
}

#[tauri::command]
fn open_permission_settings(kind: String) -> Result<(), String> {
    let permission =
        permissions::Permission::parse(&kind).ok_or_else(|| "Unknown permission".to_string())?;
    permissions::open_settings(permission);
    Ok(())
}

/// Kept for API parity with the other platforms: macOS needs no network or firewall change for a
/// tablet's USB network, so this only re-checks the link.
#[tauri::command]
async fn allow_usb_link(agent: tauri::State<'_, SharedAgent>, id: String) -> Result<(), String> {
    let agent = agent.inner().clone();
    tauri::async_runtime::spawn_blocking(move || agent.allow_usb_link(&id))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,str0m=warn".into()),
        )
        .init();

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
            let agent = Arc::new(Agent::new(vp_agent::platform::media_engine())?);
            if agent.settings().advanced.start_service_on_launch {
                if let Err(error) = agent.start_service() {
                    eprintln!("Vision-Pro service did not start: {error}");
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
            // ⌘⌥⇧⌫ by default. A shortcut already taken by another app must not
            // prevent Vision-Pro from starting: the Disconnect button still works.
            if let Err(error) = app.global_shortcut().on_shortcut(
                emergency_shortcut.as_str(),
                move |handle, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let disconnected = emergency_agent.disconnect_all();
                        let _ = handle.emit("emergency-disconnect", disconnected);
                    }
                },
            ) {
                eprintln!("Vision-Pro emergency shortcut {emergency_shortcut} unavailable: {error}");
            }
            app.manage(agent);
            Ok(())
        })
        // Like other Mac utilities, closing the window keeps the service available;
        // the Dock icon reopens it and ⌘Q quits.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
            get_permissions,
            request_permission,
            open_permission_settings,
            allow_usb_link,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Vision-Pro");

    app.run(|handle, event| match event {
        tauri::RunEvent::Reopen { .. } => {
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }
        tauri::RunEvent::Exit => handle.state::<SharedAgent>().shutdown(),
        _ => {}
    });
}
