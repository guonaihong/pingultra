use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;
use std::process::Command;
use tokio::time;
use anyhow::Result;
use ipnetwork::IpNetwork;
use chrono::{DateTime, Local};
use colored::Colorize;
use tokio::task;
use futures::future::join_all;

use crate::error::PingError;
use crate::host::PingTarget;
use crate::pinger::Pinger;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceInfo {
    pub ip: IpAddr,
    pub mac: Option<String>,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub enum DeviceStatus {
    Added(DeviceInfo),
    Removed(DeviceInfo),
    Stable(DeviceInfo),
}

pub struct NetworkMonitor {
    network: IpNetwork,
    scan_interval: Duration,
    resolve_mac: bool,
    changes_only: bool,
    devices: HashMap<IpAddr, DeviceInfo>,
    last_scan: Option<DateTime<Local>>,
}

impl NetworkMonitor {
    pub fn new(
        network: &str,
        scan_interval_secs: u64,
        resolve_mac: bool,
        changes_only: bool,
    ) -> Result<Self, PingError> {
        let network = network.parse::<IpNetwork>()
            .map_err(|_| PingError::InvalidAddress(format!("Invalid network address: {}", network)))?;
        
        Ok(Self {
            network,
            scan_interval: Duration::from_secs(scan_interval_secs),
            resolve_mac,
            changes_only,
            devices: HashMap::new(),
            last_scan: None,
        })
    }
    
    pub async fn start_monitoring(&mut self) -> Result<(), PingError> {
        println!("Starting network monitoring for {}", self.network);
        println!("Press Ctrl+C to stop monitoring\n");
        
        loop {
            let changes = self.scan_network().await?;
            self.report_changes(&changes);
            
            // 异步发送设备下线通知
            let mut notification_tasks = Vec::new();
            
            for status in &changes {
                if let DeviceStatus::Removed(device) = status {
                    let device_clone = device.clone();
                    notification_tasks.push(task::spawn(async move {
                        Self::send_offline_notification_async(&device_clone).await;
                    }));
                }
            }
            
            // 等待所有通知任务完成
            if !notification_tasks.is_empty() {
                join_all(notification_tasks).await;
            }
            
            self.last_scan = Some(Local::now());
            time::sleep(self.scan_interval).await;
        }
    }
    
    async fn scan_network(&mut self) -> Result<Vec<DeviceStatus>, PingError> {
        let now = Local::now();
        let mut current_devices = HashSet::new();
        let mut changes = Vec::new();
        
        println!("Scanning network {} at {}", self.network, now.format("%Y-%m-%d %H:%M:%S"));
        
        // 创建一个任务集合，用于存储所有的异步ping任务
        let mut ping_tasks = Vec::new();
        
        // 并行扫描网络中的所有IP地址
        for ip in self.network.iter() {
            let target = PingTarget {
                name: ip.to_string(),
                addr: ip,
            };
            
            // 创建一个异步任务来ping这个IP
            ping_tasks.push(task::spawn(async move {
                // 使用较短的超时时间来加快扫描速度
                match Pinger::new(target.clone(), 56, 64) {
                    Ok(pinger) => {
                        let response = pinger.ping_once(0, 500).await;
                        (ip, response.is_success(), target)
                    },
                    Err(e) => {
                        println!("Error creating pinger for {}: {}", ip, e);
                        (ip, false, target)
                    }
                }
            }));
        }
        
        // 等待所有ping任务完成
        let ping_results = join_all(ping_tasks).await;
        
        // 处理ping结果
        for result in ping_results {
            if let Ok((ip, is_up, target)) = result {
                if is_up {
                    println!("Host {} is up", ip);
                    
                    // 创建异步任务来获取设备信息
                    let mac_future = if self.resolve_mac {
                        Some(self.get_mac_address(ip))
                    } else {
                        None
                    };
                    
                    let hostname_future = self.resolve_hostname(ip);
                    
                    // 并行获取MAC地址和主机名
                    let (mac, hostname) = match (mac_future, hostname_future) {
                        (Some(mac_fut), hostname_fut) => {
                            let (mac_res, hostname_res) = tokio::join!(mac_fut, hostname_fut);
                            (mac_res, hostname_res)
                        },
                        (None, hostname_fut) => {
                            let hostname_res = hostname_fut.await;
                            (None, hostname_res)
                        }
                    };
                    
                    let vendor = if let Some(ref mac_addr) = mac {
                        self.lookup_vendor(mac_addr)
                    } else {
                        None
                    };
                    
                    let device_info = if let Some(existing) = self.devices.get(&ip) {
                        // 更新现有设备的最后一次看到的时间
                        DeviceInfo {
                            ip,
                            mac,
                            hostname,
                            vendor,
                            first_seen: existing.first_seen,
                            last_seen: now,
                        }
                    } else {
                        // 新设备
                        let new_device = DeviceInfo {
                            ip,
                            mac,
                            hostname,
                            vendor,
                            first_seen: now,
                            last_seen: now,
                        };
                        
                        changes.push(DeviceStatus::Added(new_device.clone()));
                        new_device
                    };
                    
                    self.devices.insert(ip, device_info.clone());
                    current_devices.insert(ip);
                    
                    if !self.changes_only {
                        changes.push(DeviceStatus::Stable(device_info));
                    }
                }
            }
        }
        
        // 检查消失的设备
        let previous_ips: HashSet<IpAddr> = self.devices.keys().cloned().collect();
        let removed_ips = previous_ips.difference(&current_devices);
        
        for &ip in removed_ips {
            if let Some(device) = self.devices.remove(&ip) {
                println!("Host {} is down", ip);
                changes.push(DeviceStatus::Removed(device));
            }
        }
        
        Ok(changes)
    }
    
    fn report_changes(&self, changes: &[DeviceStatus]) {
        if changes.is_empty() {
            println!("No changes detected in the network.");
            return;
        }
        
        println!("Network scan at {}", Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!("{:-<60}", "");
        
        for status in changes {
            match status {
                DeviceStatus::Added(device) => {
                    let info = self.format_device_info(device);
                    println!("{} {}", "[+]".green().bold(), info);
                },
                DeviceStatus::Removed(device) => {
                    let info = self.format_device_info(device);
                    println!("{} {}", "[-]".red().bold(), info);
                },
                DeviceStatus::Stable(device) => {
                    if !self.changes_only {
                        let info = self.format_device_info(device);
                        println!("{} {}", "[=]".blue(), info);
                    }
                },
            }
        }
        
        println!("{:-<60}\n", "");
    }
    
    fn format_device_info(&self, device: &DeviceInfo) -> String {
        let mut parts = Vec::new();
        
        parts.push(device.ip.to_string());
        
        if let Some(ref mac) = device.mac {
            parts.push(format!("MAC: {}", mac));
        }
        
        if let Some(ref hostname) = device.hostname {
            parts.push(format!("Host: {}", hostname));
        }
        
        if let Some(ref vendor) = device.vendor {
            parts.push(format!("Vendor: {}", vendor));
        }
        
        parts.join(" | ")
    }
    
    // 异步发送设备下线通知
    async fn send_offline_notification_async(device: &DeviceInfo) {
        let title = "设备下线通知";
        let mut message = format!("设备 {} 已下线", device.ip);
        
        if let Some(ref hostname) = device.hostname {
            message.push_str(&format!("\n主机名: {}", hostname));
        }
        
        if let Some(ref mac) = device.mac {
            message.push_str(&format!("\nMAC地址: {}", mac));
        }
        
        if let Some(ref vendor) = device.vendor {
            message.push_str(&format!("\n厂商: {}", vendor));
        }
        
        message.push_str(&format!("\n最后一次在线时间: {}", device.last_seen.format("%Y-%m-%d %H:%M:%S")));
        
        // 根据操作系统选择合适的通知方式
        #[cfg(target_os = "macos")]
        {
            // 在 macOS 上使用 osascript 发送通知
            let _ = tokio::process::Command::new("osascript")
                .arg("-e")
                .arg(format!("display notification \"{}\" with title \"{}\"", message, title))
                .output()
                .await;
        }
        
        #[cfg(target_os = "linux")]
        {
            // 在 Linux 上使用 notify-send 发送通知
            let _ = tokio::process::Command::new("notify-send")
                .arg(title)
                .arg(message)
                .output()
                .await;
        }
        
        #[cfg(target_os = "windows")]
        {
            // 在 Windows 上，可以使用 PowerShell 发送通知
            let ps_script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $notify = New-Object System.Windows.Forms.NotifyIcon; $notify.Icon = [System.Drawing.SystemIcons]::Information; $notify.Visible = $true; $notify.ShowBalloonTip(0, '{}', '{}', [System.Windows.Forms.ToolTipIcon]::None)",
                title, message
            );
            
            let _ = tokio::process::Command::new("powershell")
                .arg("-Command")
                .arg(ps_script)
                .output()
                .await;
        }
        
        // 同时在控制台输出通知信息
        println!("\n{}", "设备下线通知".red().bold());
        println!("{}", message);
        println!();
    }
    
    // 保留原来的同步方法以兼容其他代码
    fn send_offline_notification(&self, device: &DeviceInfo) {
        let title = "设备下线通知";
        let mut message = format!("设备 {} 已下线", device.ip);
        
        if let Some(ref hostname) = device.hostname {
            message.push_str(&format!("\n主机名: {}", hostname));
        }
        
        if let Some(ref mac) = device.mac {
            message.push_str(&format!("\nMAC地址: {}", mac));
        }
        
        if let Some(ref vendor) = device.vendor {
            message.push_str(&format!("\n厂商: {}", vendor));
        }
        
        message.push_str(&format!("\n最后一次在线时间: {}", device.last_seen.format("%Y-%m-%d %H:%M:%S")));
        
        // 根据操作系统选择合适的通知方式
        #[cfg(target_os = "macos")]
        {
            // 在 macOS 上使用 osascript 发送通知
            let _ = Command::new("osascript")
                .arg("-e")
                .arg(format!("display notification \"{}\" with title \"{}\"", message, title))
                .output();
        }
        
        #[cfg(target_os = "linux")]
        {
            // 在 Linux 上使用 notify-send 发送通知
            let _ = Command::new("notify-send")
                .arg(title)
                .arg(message)
                .output();
        }
        
        #[cfg(target_os = "windows")]
        {
            // 在 Windows 上，可以使用 PowerShell 发送通知
            let ps_script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $notify = New-Object System.Windows.Forms.NotifyIcon; $notify.Icon = [System.Drawing.SystemIcons]::Information; $notify.Visible = $true; $notify.ShowBalloonTip(0, '{}', '{}', [System.Windows.Forms.ToolTipIcon]::None)",
                title, message
            );
            
            let _ = Command::new("powershell")
                .arg("-Command")
                .arg(ps_script)
                .output();
        }
        
        // 同时在控制台输出通知信息
        println!("\n{}", "设备下线通知".red().bold());
        println!("{}", message);
        println!();
    }
    
    async fn get_mac_address(&self, ip: IpAddr) -> Option<String> {
        // 使用系统命令获取MAC地址
        // 在Linux上使用arp命令，在macOS上也可以使用arp命令
        let ip_str = ip.to_string();
        
        match tokio::process::Command::new("arp")
            .arg("-n")
            .arg(&ip_str)
            .output()
            .await {
                Ok(output) => {
                    if output.status.success() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        
                        // 解析arp命令输出，提取MAC地址
                        // 格式通常是: IP地址 (?) MAC地址 (?) 接口
                        for line in output_str.lines() {
                            if line.contains(&ip_str) {
                                // 尝试提取MAC地址，格式通常是xx:xx:xx:xx:xx:xx
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 3 {
                                    let mac = parts[2];
                                    if mac.contains(':') && mac.len() >= 17 { // 标准MAC地址长度
                                        return Some(mac.to_string());
                                    }
                                }
                            }
                        }
                    }
                    None
                },
                Err(_) => None,
            }
    }
    
    async fn resolve_hostname(&self, ip: IpAddr) -> Option<String> {
        // 使用反向DNS查询获取主机名
        match tokio::process::Command::new("host")
            .arg(ip.to_string())
            .output()
            .await {
                Ok(output) => {
                    if output.status.success() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        
                        // 解析host命令输出，提取主机名
                        // 格式通常是: IP地址 domain name pointer hostname.
                        for line in output_str.lines() {
                            if line.contains("domain name pointer") {
                                let parts: Vec<&str> = line.split("domain name pointer").collect();
                                if parts.len() >= 2 {
                                    let hostname = parts[1].trim().trim_end_matches('.');
                                    return Some(hostname.to_string());
                                }
                            }
                        }
                    }
                    None
                },
                Err(_) => None,
            }
    }
    
    fn lookup_vendor(&self, mac: &str) -> Option<String> {
        // 简化实现：根据MAC地址前缀判断厂商
        // 实际应用中应该使用MAC地址厂商数据库
        let prefix = mac.split(':').take(3).collect::<Vec<&str>>().join(":");
        
        match prefix.as_str() {
            "00:0c:29" => Some("VMware".to_string()),
            "00:50:56" => Some("VMware".to_string()),
            "00:1a:11" => Some("Google".to_string()),
            "00:1e:c2" | "00:16:cb" | "00:17:f2" | "00:1f:5b" | "00:21:e9" | 
            "00:22:41" | "00:23:12" | "00:23:32" | "00:25:00" | "00:26:08" | 
            "00:26:b0" | "00:26:bb" | "00:30:65" | "00:3e:e1" | "00:0d:93" | 
            "00:11:24" | "00:14:51" | "00:19:e3" | "00:1b:63" | "00:1c:b3" | 
            "00:1d:4f" | "00:1e:52" | "00:1f:f3" => Some("Apple".to_string()),
            "00:1c:42" => Some("Parallels".to_string()),
            "52:54:00" => Some("QEMU/KVM".to_string()),
            "00:15:5d" => Some("Microsoft".to_string()),
            _ => None,
        }
    }
}

// 导出为JSON格式
pub fn export_to_json(devices: &[DeviceStatus]) -> Result<String, serde_json::Error> {
    let json_data = serde_json::json!({
        "timestamp": Local::now().to_rfc3339(),
        "devices": devices.iter().map(|status| {
            match status {
                DeviceStatus::Added(device) => {
                    serde_json::json!({
                        "status": "added",
                        "ip": device.ip.to_string(),
                        "mac": device.mac,
                        "hostname": device.hostname,
                        "vendor": device.vendor,
                        "first_seen": device.first_seen.to_rfc3339(),
                        "last_seen": device.last_seen.to_rfc3339()
                    })
                },
                DeviceStatus::Removed(device) => {
                    serde_json::json!({
                        "status": "removed",
                        "ip": device.ip.to_string(),
                        "mac": device.mac,
                        "hostname": device.hostname,
                        "vendor": device.vendor,
                        "first_seen": device.first_seen.to_rfc3339(),
                        "last_seen": device.last_seen.to_rfc3339()
                    })
                },
                DeviceStatus::Stable(device) => {
                    serde_json::json!({
                        "status": "stable",
                        "ip": device.ip.to_string(),
                        "mac": device.mac,
                        "hostname": device.hostname,
                        "vendor": device.vendor,
                        "first_seen": device.first_seen.to_rfc3339(),
                        "last_seen": device.last_seen.to_rfc3339()
                    })
                }
            }
        }).collect::<Vec<_>>()
    });
    
    serde_json::to_string_pretty(&json_data)
}

// 导出为CSV格式
pub fn export_to_csv(devices: &[DeviceStatus]) -> Result<String, csv::Error> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    
    // 写入标题行
    match wtr.write_record(&["status", "ip", "mac", "hostname", "vendor", "first_seen", "last_seen"]) {
        Ok(_) => (),
        Err(e) => return Err(e),
    }
    
    for status in devices {
        match status {
            DeviceStatus::Added(device) => {
                match wtr.write_record(&[
                    "added",
                    &device.ip.to_string(),
                    &device.mac.clone().unwrap_or_default(),
                    &device.hostname.clone().unwrap_or_default(),
                    &device.vendor.clone().unwrap_or_default(),
                    &device.first_seen.to_rfc3339(),
                    &device.last_seen.to_rfc3339(),
                ]) {
                    Ok(_) => (),
                    Err(e) => return Err(e),
                }
            },
            DeviceStatus::Removed(device) => {
                match wtr.write_record(&[
                    "removed",
                    &device.ip.to_string(),
                    &device.mac.clone().unwrap_or_default(),
                    &device.hostname.clone().unwrap_or_default(),
                    &device.vendor.clone().unwrap_or_default(),
                    &device.first_seen.to_rfc3339(),
                    &device.last_seen.to_rfc3339(),
                ]) {
                    Ok(_) => (),
                    Err(e) => return Err(e),
                }
            },
            DeviceStatus::Stable(device) => {
                match wtr.write_record(&[
                    "stable",
                    &device.ip.to_string(),
                    &device.mac.clone().unwrap_or_default(),
                    &device.hostname.clone().unwrap_or_default(),
                    &device.vendor.clone().unwrap_or_default(),
                    &device.first_seen.to_rfc3339(),
                    &device.last_seen.to_rfc3339(),
                ]) {
                    Ok(_) => (),
                    Err(e) => return Err(e),
                }
            },
        }
    }
    
    match wtr.into_inner() {
        Ok(inner) => match String::from_utf8(inner) {
            Ok(data) => Ok(data),
            Err(_) => Err(csv::Error::from(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid UTF-8 sequence"
            ))),
        },
        Err(e) => Err(csv::Error::from(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to get inner writer: {}", e)
        ))),
    }
}
