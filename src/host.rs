use anyhow::Result;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use crate::error::PingError;

#[derive(Debug, Clone)]
pub struct PingTarget {
    pub name: String,
    pub addr: IpAddr,
}

impl PingTarget {
    pub fn new(host: String, addr: IpAddr) -> Self {
        Self { name: host, addr }
    }
}

#[derive(Debug)]
pub struct PingResponse {
    pub target: PingTarget,
    pub seq: u16,
    pub rtt: Option<Duration>,
    pub bytes: usize,
    pub ttl: u8,
    pub error: Option<PingError>,
}

impl PingResponse {
    pub fn success(target: PingTarget, seq: u16, rtt: Duration, bytes: usize, ttl: u8) -> Self {
        Self {
            target,
            seq,
            rtt: Some(rtt),
            bytes,
            ttl,
            error: None,
        }
    }

    pub fn failure(target: PingTarget, seq: u16, bytes: usize, ttl: u8, error: PingError) -> Self {
        Self {
            target,
            seq,
            rtt: None,
            bytes,
            ttl,
            error: Some(error),
        }
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

pub fn resolve_host(host: &str) -> Result<IpAddr, PingError> {
    // First try to parse as an IP address
    if let Ok(addr) = host.parse::<IpAddr>() {
        return Ok(addr);
    }

    // Try to resolve using the system resolver
    match (host, 0).to_socket_addrs() {
        Ok(mut addrs) => {
            // Prefer IPv4 addresses
            for addr in addrs.clone() {
                if addr.ip().is_ipv4() {
                    return Ok(addr.ip());
                }
            }

            // Fall back to any address
            if let Some(addr) = addrs.next() {
                return Ok(addr.ip());
            }

            Err(PingError::ResolutionError(format!(
                "No addresses found for {}",
                host
            )))
        }
        Err(_) => Err(PingError::ResolutionError(format!(
            "Failed to resolve {}",
            host
        ))),
    }
}

pub fn load_hosts_from_file(file_path: &str) -> Result<Vec<String>> {
    let file_content = std::fs::read_to_string(file_path)?;
    let mut hosts = Vec::new();

    for line in file_content.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            hosts.push(line.to_string());
        }
    }

    Ok(hosts)
}
