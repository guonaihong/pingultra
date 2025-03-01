use std::collections::HashMap;
use std::io::{self, Write};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use chrono::{DateTime, Local};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};

use crate::monitor::DeviceInfo;

/// 设备状态枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceUIStatus {
    Online,
    Offline,
    New,
    Lost,
}

/// 设备UI信息结构体
#[derive(Debug, Clone)]
pub struct DeviceUIInfo {
    pub ip: IpAddr,
    pub mac: Option<String>,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub status: DeviceUIStatus,
    pub first_seen: DateTime<Local>,
    pub last_seen: DateTime<Local>,
    pub last_status_change: Instant,
}

impl From<&DeviceInfo> for DeviceUIInfo {
    fn from(info: &DeviceInfo) -> Self {
        Self {
            ip: info.ip,
            mac: info.mac.clone(),
            hostname: info.hostname.clone(),
            vendor: info.vendor.clone(),
            status: DeviceUIStatus::Online,
            first_seen: info.first_seen,
            last_seen: info.last_seen,
            last_status_change: Instant::now(),
        }
    }
}

/// 字符UI管理器
#[derive(Clone)]
pub struct CharacterUI {
    devices: HashMap<IpAddr, DeviceUIInfo>,
    running: Arc<Mutex<bool>>,
    show_details: bool,
    sort_by_ip: bool,
    highlight_index: usize,
    scroll_offset: usize,
}

impl CharacterUI {
    /// 创建一个新的字符UI管理器
    pub fn new(running: Arc<Mutex<bool>>) -> Self {
        Self {
            devices: HashMap::new(),
            running,
            show_details: true,
            sort_by_ip: true,
            highlight_index: 0,
            scroll_offset: 0,
        }
    }

    /// 更新设备状态
    pub fn update_device(&mut self, device: &DeviceInfo, status: DeviceUIStatus) {
        let now = Instant::now();
        
        if let Some(existing) = self.devices.get_mut(&device.ip) {
            // 更新现有设备
            existing.mac = device.mac.clone();
            existing.hostname = device.hostname.clone();
            existing.vendor = device.vendor.clone();
            existing.last_seen = device.last_seen;
            
            // 只有当状态改变时才更新状态变更时间
            if existing.status != status {
                existing.status = status;
                existing.last_status_change = now;
            }
        } else {
            // 添加新设备
            let mut ui_info = DeviceUIInfo::from(device);
            ui_info.status = status;
            ui_info.last_status_change = now;
            self.devices.insert(device.ip, ui_info);
        }
    }

    /// 标记设备为离线
    pub fn mark_device_offline(&mut self, ip: &IpAddr) {
        if let Some(device) = self.devices.get_mut(ip) {
            if device.status != DeviceUIStatus::Offline && device.status != DeviceUIStatus::Lost {
                device.status = DeviceUIStatus::Offline;
                device.last_status_change = Instant::now();
            }
        }
    }

    /// 标记设备为丢失（长时间离线）
    pub fn mark_device_lost(&mut self, ip: &IpAddr) {
        if let Some(device) = self.devices.get_mut(ip) {
            if device.status == DeviceUIStatus::Offline {
                device.status = DeviceUIStatus::Lost;
                device.last_status_change = Instant::now();
            }
        }
    }

    /// 启动UI循环
    pub fn run(&mut self) -> io::Result<()> {
        // 设置终端
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide
        )?;

        // 清屏
        execute!(stdout, terminal::Clear(ClearType::All))?;

        let mut last_render = Instant::now();
        let render_interval = Duration::from_millis(500); // 每500毫秒更新一次屏幕

        // 主循环
        while *self.running.lock().unwrap() {
            // 处理输入
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            *self.running.lock().unwrap() = false;
                        },
                        KeyCode::Char('d') => {
                            self.show_details = !self.show_details;
                        },
                        KeyCode::Char('s') => {
                            self.sort_by_ip = !self.sort_by_ip;
                        },
                        KeyCode::Up => {
                            if self.highlight_index > 0 {
                                self.highlight_index -= 1;
                                if self.highlight_index < self.scroll_offset {
                                    self.scroll_offset = self.highlight_index;
                                }
                            }
                        },
                        KeyCode::Down => {
                            let device_count = self.devices.len();
                            if device_count > 0 && self.highlight_index < device_count - 1 {
                                self.highlight_index += 1;
                                
                                // 获取终端高度以计算可见行数
                                if let Ok((_, height)) = terminal::size() {
                                    let visible_rows = (height as usize).saturating_sub(7); // 减去标题和底部信息的行数
                                    if self.highlight_index >= self.scroll_offset + visible_rows {
                                        self.scroll_offset = self.highlight_index - visible_rows + 1;
                                    }
                                }
                            }
                        },
                        _ => {}
                    }
                }
            }

            // 定时渲染UI
            let now = Instant::now();
            if now.duration_since(last_render) >= render_interval {
                self.render(&mut stdout)?;
                last_render = now;
            }
        }

        // 恢复终端
        execute!(
            stdout,
            terminal::LeaveAlternateScreen,
            cursor::Show
        )?;
        terminal::disable_raw_mode()?;

        Ok(())
    }

    /// 渲染UI
    fn render(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        // 清屏
        execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;

        // 获取终端大小
        let (width, height) = terminal::size()?;
        let visible_rows = (height as usize).saturating_sub(7); // 减去标题和底部信息的行数

        // 渲染标题
        let title = " PingUltra Network Monitor ";
        writeln!(stdout, "{}", style::style(title).black().on_white().bold())?;
        writeln!(stdout, "{}", "─".repeat(width as usize))?;

        // 表头
        let header = if self.show_details {
            format!("{:<15} {:<17} {:<20} {:<15} {:<10}", "IP", "MAC", "Hostname", "Vendor", "Status")
        } else {
            format!("{:<15} {:<10}", "IP", "Status")
        };
        writeln!(stdout, "{}", style::style(header).bold())?;

        // 获取排序后的设备列表
        let mut devices: Vec<&DeviceUIInfo> = self.devices.values().collect();
        if self.sort_by_ip {
            devices.sort_by_key(|d| d.ip);
        } else {
            devices.sort_by(|a, b| {
                // 先按状态排序（在线 > 新设备 > 离线 > 丢失）
                let status_order = |s: &DeviceUIStatus| -> u8 {
                    match s {
                        DeviceUIStatus::Online => 0,
                        DeviceUIStatus::New => 1,
                        DeviceUIStatus::Offline => 2,
                        DeviceUIStatus::Lost => 3,
                    }
                };
                
                let a_order = status_order(&a.status);
                let b_order = status_order(&b.status);
                
                if a_order != b_order {
                    a_order.cmp(&b_order)
                } else {
                    // 状态相同时按IP排序
                    a.ip.cmp(&b.ip)
                }
            });
        }

        // 计算可见设备
        let end_idx = (self.scroll_offset + visible_rows).min(devices.len());
        let visible_devices = &devices[self.scroll_offset..end_idx];

        // 渲染设备列表
        for (idx, device) in visible_devices.iter().enumerate() {
            let is_highlighted = idx + self.scroll_offset == self.highlight_index;
            
            // 根据状态设置颜色
            let status_str = match device.status {
                DeviceUIStatus::Online => style::style("在线").green().to_string(),
                DeviceUIStatus::Offline => style::style("离线").red().to_string(),
                DeviceUIStatus::New => style::style("新设备").yellow().to_string(),
                DeviceUIStatus::Lost => style::style("丢失").red().bold().to_string(),
            };
            
            let line = if self.show_details {
                format!(
                    "{:<15} {:<17} {:<20} {:<15} {:<10}",
                    device.ip.to_string(),
                    device.mac.as_deref().unwrap_or("-"),
                    device.hostname.as_deref().unwrap_or("-"),
                    device.vendor.as_deref().unwrap_or("-"),
                    status_str
                )
            } else {
                format!(
                    "{:<15} {:<10}",
                    device.ip.to_string(),
                    status_str
                )
            };
            
            // 高亮显示选中的行
            if is_highlighted {
                execute!(stdout, style::SetBackgroundColor(Color::DarkBlue))?;
                writeln!(stdout, "{}", line)?;
                execute!(stdout, style::ResetColor)?;
            } else {
                writeln!(stdout, "{}", line)?;
            }
        }

        // 填充剩余空间
        let remaining_rows = visible_rows.saturating_sub(visible_devices.len());
        for _ in 0..remaining_rows {
            writeln!(stdout)?;
        }

        // 底部信息和帮助
        writeln!(stdout, "{}", "─".repeat(width as usize))?;
        writeln!(
            stdout,
            "设备总数: {} | 在线: {} | 离线: {} | 新设备: {} | 丢失: {}",
            devices.len(),
            devices.iter().filter(|d| d.status == DeviceUIStatus::Online).count(),
            devices.iter().filter(|d| d.status == DeviceUIStatus::Offline).count(),
            devices.iter().filter(|d| d.status == DeviceUIStatus::New).count(),
            devices.iter().filter(|d| d.status == DeviceUIStatus::Lost).count()
        )?;
        writeln!(
            stdout,
            "按键: [q]退出 [d]切换详情 [s]切换排序 [↑/↓]导航"
        )?;

        stdout.flush()?;
        Ok(())
    }
}
