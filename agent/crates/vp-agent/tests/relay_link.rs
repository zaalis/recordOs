//! The real agent against the real relay: registration, access token and reconnection.

use std::sync::Arc;
use std::time::{Duration, Instant};
use vp_agent::agent::Agent;
use vp_agent::config::NetworkMode;
use vp_agent::control::NullControlEngine;
use vp_agent::relay::RelayState;
use vp_relay::{RelayConfig, RunningRelay};

const TOKEN: &str = "integration-token-0123";

async fn start_relay(http: &str, turn: &str) -> RunningRelay {
    vp_relay::start(RelayConfig {
        http_bind: http.parse().unwrap(),
        turn_bind: turn.parse().unwrap(),
        turn_tls_bind: None,
        tls: None,
        public_host: "127.0.0.1".into(),
        access_token: Some(TOKEN.into()),
        trust_proxy: false,
    })
    .await
    .unwrap()
}

fn wait_for(agent: &Agent, state: RelayState, within: Duration) -> vp_agent::relay::RelayStatus {
    let deadline = Instant::now() + within;
    loop {
        let status = agent.status().relay;
        if status.state == state {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "relay state {:?} not reached, last status {status:?}",
            state
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn configure(agent: &Agent, relay: &RunningRelay, token: &str) {
    let mut settings = agent.settings();
    settings.network.mode = NetworkMode::RemoteAccess;
    settings.network.advertise_mdns = false;
    settings.network.https_port = 20_000 + (std::process::id() % 20_000) as u16;
    settings.network.relay_url = Some(format!("ws://{}", relay.http_addr));
    settings.network.relay_token = Some(token.into());
    agent.update_settings(settings).unwrap();
}

#[test]
fn agent_registers_and_reconnects() {
    let home = std::env::temp_dir().join(format!("vp-relay-link-{}", std::process::id()));
    std::env::set_var("VISION_PRO_HOME", &home);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let relay = runtime.block_on(start_relay("127.0.0.1:0", "127.0.0.1:0"));
    let http_addr = relay.http_addr;

    let agent = Arc::new(Agent::new(Arc::new(NullControlEngine)).unwrap());
    configure(&agent, &relay, "wrong-token-000000000");
    agent.start_service().unwrap();
    let refused = wait_for(&agent, RelayState::Error, Duration::from_secs(10));
    assert!(
        refused.error.unwrap().contains("jeton"),
        "a bad token must be reported clearly"
    );

    configure(&agent, &relay, TOKEN);
    let connected = wait_for(&agent, RelayState::Connected, Duration::from_secs(10));
    assert_eq!(
        connected.url.as_deref(),
        Some(format!("ws://{http_addr}").as_str())
    );
    assert!(relay.relay.hub.is_online(&agent.status().agent_id));

    let core = agent.core().clone();
    assert_eq!(
        core.relay.public_url().as_deref(),
        Some(format!("ws://{http_addr}").as_str())
    );

    // Relay restart on the same address: the agent reconnects by itself.
    let turn_addr = relay.turn_udp_addr;
    drop(relay);
    wait_for(&agent, RelayState::Error, Duration::from_secs(100));
    assert_eq!(
        core.relay.public_url(),
        None,
        "iPads must not be offered a relay that is down"
    );
    let relay = runtime.block_on(start_relay(&http_addr.to_string(), &turn_addr.to_string()));
    wait_for(&agent, RelayState::Connected, Duration::from_secs(45));
    assert!(relay.relay.hub.is_online(&agent.status().agent_id));

    // Back to Local Only: the relay connection is closed.
    let mut settings = agent.settings();
    settings.network.mode = NetworkMode::LocalOnly;
    agent.update_settings(settings).unwrap();
    wait_for(&agent, RelayState::Disabled, Duration::from_secs(10));
    let deadline = Instant::now() + Duration::from_secs(5);
    while relay.relay.hub.is_online(&agent.status().agent_id) {
        assert!(Instant::now() < deadline, "relay still lists the agent");
        std::thread::sleep(Duration::from_millis(50));
    }

    agent.shutdown();
    drop(relay);
    drop(runtime);
    let _ = std::fs::remove_dir_all(home);
}
