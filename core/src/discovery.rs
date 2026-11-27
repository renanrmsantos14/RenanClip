//! Descoberta automática na rede local por mDNS (`_renanclip._tcp`).

use crate::crypto::{Error, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::{IpAddr, SocketAddr};

pub const SERVICE: &str = "_renanclip._tcp.local.";

#[derive(Clone, Debug)]
pub struct Nearby {
    pub id: String,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
}

pub struct Discovery {
    daemon: ServiceDaemon,
}

fn err(e: mdns_sd::Error) -> Error {
    Error::Proto(format!("mdns: {e}"))
}

impl Discovery {
    pub fn start(
        my_id: &str,
        name: &str,
        port: u16,
        on_up: impl Fn(Nearby) + Send + 'static,
        on_down: impl Fn(String) + Send + 'static,
    ) -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(err)?;
        let host = format!("rc-{}.local.", &my_id[..8]);
        let props = [("id", my_id), ("n", name), ("v", "1")];
        let info = ServiceInfo::new(SERVICE, my_id, &host, "", port, &props[..]).map_err(err)?.enable_addr_auto();
        daemon.register(info).map_err(err)?;
        let rx = daemon.browse(SERVICE).map_err(err)?;
        let me = my_id.to_string();
        std::thread::Builder::new()
            .name("mdns-events".into())
            .spawn(move || {
                while let Ok(ev) = rx.recv() {
                    match ev {
                        ServiceEvent::ServiceResolved(info) => {
                            let Some(id) = info.get_property_val_str("id").map(str::to_string) else { continue };
                            if id == me || id.len() != 32 {
                                continue;
                            }
                            let name = info.get_property_val_str("n").unwrap_or("?").to_string();
                            let port = info.get_port();
                            let addrs = info.get_addresses_v4().into_iter().map(|a| SocketAddr::new(IpAddr::V4(*a), port)).collect::<Vec<_>>();
                            if !addrs.is_empty() {
                                on_up(Nearby { id, name, addrs });
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, full) => {
                            if let Some(id) = full.split('.').next() {
                                on_down(id.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            })
            .map_err(Error::Io)?;
        Ok(Self { daemon })
    }

    pub fn stop(&self) {
        let _ = self.daemon.shutdown();
    }
}
