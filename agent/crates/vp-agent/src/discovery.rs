//! Local discovery over mDNS/Bonjour. Advertises this Mac as `_vision-pro._tcp` with a stable
//! `vp-xxxxxxxx.local` hostname (the web app origin) and lists other Vision-Pro Macs nearby.
//! Discovery is informational only: it never grants any access.

use parking_lot::RwLock;
use serde::Serialize;
use std::net::Ipv4Addr;
use std::sync::Arc;

pub const SERVICE_TYPE: &str = "_vision-pro._tcp.local.";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyAgent {
    pub agent_id: String,
    pub name: String,
    pub hostname: String,
    pub port: u16,
}

pub struct Discovery {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl Discovery {
    pub fn start(
        agent_id: &str,
        computer_name: &str,
        hostname: &str,
        ips: &[Ipv4Addr],
        port: u16,
        nearby: Arc<RwLock<Vec<NearbyAgent>>>,
    ) -> anyhow::Result<Discovery> {
        let daemon = mdns_sd::ServiceDaemon::new()?;
        let instance = format!(
            "vp-{}",
            &agent_id[..8].to_lowercase().replace(['_', '-'], "0")
        );
        let host = format!("{hostname}.");
        let name: String = computer_name
            .chars()
            .filter(|c| !c.is_control())
            .take(48)
            .collect();
        let properties = [("id", agent_id), ("name", name.as_str()), ("v", "1")];
        let ip_list: Vec<std::net::IpAddr> =
            ips.iter().map(|ip| std::net::IpAddr::V4(*ip)).collect();
        let info = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &host,
            &ip_list[..],
            port,
            &properties[..],
        )?;
        let fullname = info.get_fullname().to_string();
        daemon.register(info)?;

        let receiver = daemon.browse(SERVICE_TYPE)?;
        let own_id = agent_id.to_string();
        std::thread::Builder::new()
            .name("vp-mdns-browse".into())
            .spawn(move || {
                while let Ok(event) = receiver.recv() {
                    match event {
                        mdns_sd::ServiceEvent::ServiceResolved(info) => {
                            let Some(id) = info.get_property_val_str("id") else {
                                continue;
                            };
                            if id == own_id || id.len() != 22 {
                                continue;
                            }
                            let entry = NearbyAgent {
                                agent_id: id.to_string(),
                                name: info
                                    .get_property_val_str("name")
                                    .unwrap_or("Computer")
                                    .chars()
                                    .take(48)
                                    .collect(),
                                hostname: info.get_hostname().trim_end_matches('.').to_string(),
                                port: info.get_port(),
                            };
                            let mut list = nearby.write();
                            list.retain(|a| a.agent_id != entry.agent_id);
                            list.push(entry);
                            list.truncate(32);
                        }
                        mdns_sd::ServiceEvent::ServiceRemoved(_, fullname) => {
                            nearby.write().retain(|a| {
                                !fullname.starts_with(&format!(
                                    "vp-{}",
                                    &a.agent_id[..8].to_lowercase().replace(['_', '-'], "0")
                                ))
                            });
                        }
                        mdns_sd::ServiceEvent::SearchStopped(_) => break,
                        _ => {}
                    }
                }
            })?;
        Ok(Discovery { daemon, fullname })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
