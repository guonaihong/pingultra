use chrono::Local;
use colored::Colorize;
use std::time::Duration;

use crate::host::PingResponse;
use crate::stats::PingStats;

pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1 {
        format!("{} µs", duration.as_micros())
    } else if millis < 1000 {
        format!("{:.2} ms", millis as f64)
    } else {
        format!("{:.2} s", duration.as_secs_f64())
    }
}

pub fn print_ping_start(host: &str, addr: &str, size: usize) {
    println!("PING {} ({}): {} data bytes", host, addr, size);
}

pub fn print_ping_result(response: &PingResponse, show_timestamp: bool) {
    let timestamp_str = if show_timestamp {
        format!("[{}] ", Local::now().format("%H:%M:%S%.3f"))
    } else {
        String::new()
    };
    
    match &response.error {
        None => {
            let rtt = response.rtt.unwrap();
            println!(
                "{}{} bytes from {}: icmp_seq={} ttl={} time={}",
                timestamp_str,
                response.bytes,
                response.target.addr,
                response.seq,
                response.ttl,
                format_duration(rtt).green()
            );
        },
        Some(crate::error::PingError::Timeout) => {
            println!(
                "{}Request timeout for icmp_seq={} ({})",
                timestamp_str,
                response.seq,
                response.target.addr.to_string().red()
            );
        },
        Some(e) => {
            println!(
                "{}Error pinging {} (seq={}): {}",
                timestamp_str,
                response.target.addr.to_string().red(),
                response.seq,
                e
            );
        }
    }
}

pub fn print_ping_summary(host: &str, stats: &PingStats) {
    println!("\n--- {} ping statistics ---", host);
    println!("{} packets transmitted, {} received, {:.1}% packet loss", 
             stats.sent, stats.received, stats.loss_percent());
    
    if stats.received > 0 {
        println!("rtt min/avg/max = {}/{}/{}", 
                 format_duration(stats.min_rtt.unwrap()),
                 format_duration(stats.avg_rtt().unwrap()),
                 format_duration(stats.max_rtt.unwrap()));
    }
}

pub fn print_json_summary(host: &str, stats: &PingStats) -> String {
    let min = stats.min_rtt.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let avg = stats.avg_rtt().map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let max = stats.max_rtt.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    
    format!(
        r#"{{
  "host": "{}",
  "packets_transmitted": {},
  "packets_received": {},
  "packet_loss_percent": {:.1},
  "rtt_ms": {{
    "min": {:.3},
    "avg": {:.3},
    "max": {:.3}
  }}
}}"#,
        host, stats.sent, stats.received, stats.loss_percent(),
        min, avg, max
    )
}

pub fn print_csv_summary(host: &str, stats: &PingStats) -> String {
    let min = stats.min_rtt.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let avg = stats.avg_rtt().map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    let max = stats.max_rtt.map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    
    format!(
        "host,packets_transmitted,packets_received,packet_loss_percent,rtt_min_ms,rtt_avg_ms,rtt_max_ms\n{},{},{},{:.1},{:.3},{:.3},{:.3}",
        host, stats.sent, stats.received, stats.loss_percent(),
        min, avg, max
    )
}
