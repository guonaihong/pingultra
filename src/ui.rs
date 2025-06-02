use chrono::{DateTime, Local};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::monitor::DeviceInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceUIStatus {
    Online,
    Offline,
    New,
    Lost,
}

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
    pub offline_at: Option<DateTime<Local>>,
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
            offline_at: None,
        }
    }
}

#[derive(Clone)]
pub struct CharacterUI {
    devices: Arc<Mutex<HashMap<IpAddr, DeviceUIInfo>>>,
    running: Arc<Mutex<bool>>,
    show_details: bool,
    sort_by_ip: bool,
    highlight_index: usize,
    scroll_offset: usize,
}

impl CharacterUI {
    pub fn new(running: Arc<Mutex<bool>>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            running,
            show_details: true,
            sort_by_ip: false,
            highlight_index: 0,
            scroll_offset: 0,
        }
    }

    pub fn update_device(&mut self, device: &DeviceInfo, status: DeviceUIStatus) {
        let now = Instant::now();
        let mut devices = self.devices.lock().unwrap();

        if let Some(existing) = devices.get_mut(&device.ip) {
            existing.mac = device.mac.clone();
            existing.hostname = device.hostname.clone();
            existing.vendor = device.vendor.clone();
            existing.last_seen = device.last_seen;

            if existing.status != status {
                existing.status = status;
                existing.last_status_change = now;
            }
        } else {
            let mut ui_info = DeviceUIInfo::from(device);
            ui_info.status = status;
            ui_info.last_status_change = now;
            devices.insert(device.ip, ui_info);
        }
    }

    pub fn mark_device_offline(&mut self, ip: &IpAddr) {
        let mut devices = self.devices.lock().unwrap();
        if let Some(device) = devices.get_mut(ip) {
            if device.status != DeviceUIStatus::Offline && device.status != DeviceUIStatus::Lost {
                device.status = DeviceUIStatus::Offline;
                device.last_status_change = Instant::now();
                device.offline_at = Some(Local::now());
            }
        }
    }

    pub fn mark_device_lost(&mut self, ip: &IpAddr) {
        let mut devices = self.devices.lock().unwrap();
        if let Some(device) = devices.get_mut(ip) {
            if device.status == DeviceUIStatus::Offline {
                device.status = DeviceUIStatus::Lost;
                device.last_status_change = Instant::now();
            }
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;

        let mut stdout = io::stdout();
        self.render(&mut stdout)?;

        let mut last_render = Instant::now();
        while *self.running.lock().unwrap() {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(KeyEvent { code, .. }) = event::read()? {
                    match code {
                        KeyCode::Char('q') | KeyCode::Esc => *self.running.lock().unwrap() = false,
                        KeyCode::Char('d') => {
                            self.show_details = !self.show_details;
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Char('s') => {
                            self.sort_by_ip = !self.sort_by_ip;
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Up => self.handle_up_key(),
                        KeyCode::Down => self.handle_down_key()?,
                        KeyCode::PageUp => self.handle_page_up()?,
                        KeyCode::PageDown => self.handle_page_down()?,
                        _ => {}
                    }
                }
            }

            if Instant::now().duration_since(last_render) >= Duration::from_secs(1) {
                self.render(&mut stdout)?;
                last_render = Instant::now();
            }
        }

        execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    fn handle_up_key(&mut self) {
        if self.highlight_index > 0 {
            self.highlight_index -= 1;
            if self.highlight_index < self.scroll_offset {
                self.scroll_offset = self.highlight_index;
            }
        }
    }

    fn handle_down_key(&mut self) -> io::Result<()> {
        let device_count = self.devices.lock().unwrap().len();
        if device_count == 0 {
            return Ok(());
        }

        if self.highlight_index < device_count - 1 {
            self.highlight_index += 1;
            let (_, height) = terminal::size()?;
            let visible_rows = (height as usize).saturating_sub(6);
            if self.highlight_index >= self.scroll_offset + visible_rows {
                self.scroll_offset = self.highlight_index + 1 - visible_rows;
            }
        }
        Ok(())
    }

    fn handle_page_up(&mut self) -> io::Result<()> {
        let (_, height) = terminal::size()?;
        let visible_rows = (height as usize).saturating_sub(6);
        if self.highlight_index > 0 {
            if self.highlight_index >= visible_rows {
                self.highlight_index -= visible_rows;
            } else {
                self.highlight_index = 0;
            }
            self.scroll_offset = self.highlight_index;
        }
        Ok(())
    }

    fn handle_page_down(&mut self) -> io::Result<()> {
        let device_count = self.devices.lock().unwrap().len();
        if device_count == 0 {
            return Ok(());
        }
        let (_, height) = terminal::size()?;
        let visible_rows = (height as usize).saturating_sub(6);
        if self.highlight_index < device_count - 1 {
            self.highlight_index = (self.highlight_index + visible_rows).min(device_count - 1);
            self.scroll_offset = self.highlight_index + 1 - visible_rows;
        }
        Ok(())
    }

    fn render(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        execute!(
            stdout,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        let (width, height) = terminal::size()?;
        let visible_rows = (height as usize).saturating_sub(6);

        self.render_title(stdout, width)?;
        self.render_table_header(stdout, 16, 13, 18, 13)?;

        let devices = self.get_sorted_devices();
        let device_count = devices.len();
        let (start_idx, end_idx, highlight_index, scroll_offset) = Self::calculate_visible_range_and_highlight(
            device_count,
            visible_rows,
            self.highlight_index,
            self.scroll_offset,
        );
        let visible_devices = &devices[start_idx..end_idx];

        for (idx, device) in visible_devices.iter().enumerate() {
            let absolute_idx = start_idx + idx;
            self.render_device_row(
                stdout,
                device,
                absolute_idx == highlight_index,
                16,
                13,
                18,
                13,
                width,
                idx,
            )?;
        }

        self.fill_remaining_rows(stdout, visible_devices.len(), visible_rows, width)?;
        self.render_footer(stdout, &devices, width, height)?;
        stdout.flush()
    }

    fn render_title(&self, stdout: &mut io::Stdout, width: u16) -> io::Result<()> {
        let title = " PingUltra Network Monitor ";
        let title_len = title.len() as u16;
        let padding = (width.saturating_sub(title_len)) / 2;

        execute!(
            stdout,
            cursor::MoveTo(0, 0),
            style::Print(" ".repeat(padding as usize)),
            style::PrintStyledContent(
                style::style(title)
                    .with(style::Color::Black)
                    .on(style::Color::White)
                    .bold()
            ),
        )?;
        self.render_separator(stdout, width, 1)
    }

    fn render_table_header(
        &self,
        stdout: &mut io::Stdout,
        ip_width: usize,
        mac_width: usize,
        hostname_width: usize,
        vendor_width: usize,
    ) -> io::Result<()> {
        let ip_width: usize = 16;
        let alive_width: usize = 12;
        let mac_width: usize = 13;
        let hostname_width: usize = 18;
        let vendor_width: usize = 13;
        let header = if self.show_details {
            format!(
                "{:<ip_w$} {:<alive_w$} {:<mac_w$} {:<host_w$} {:<vendor_w$} {}",
                "IP",
                "存活时间",
                "MAC",
                "Hostname",
                "Vendor",
                "Status",
                ip_w = ip_width,
                alive_w = alive_width,
                mac_w = mac_width,
                host_w = hostname_width,
                vendor_w = vendor_width
            )
        } else {
            format!("{:<ip_w$} {}", "IP", "Status", ip_w = ip_width)
        };

        execute!(
            stdout,
            cursor::MoveTo(0, 2),
            style::PrintStyledContent(header.bold()),
        )?;
        self.render_separator(stdout, 0, 3)
    }

    fn render_device_row(
        &self,
        stdout: &mut io::Stdout,
        device: &DeviceUIInfo,
        is_highlighted: bool,
        _ip_width: usize,
        _mac_width: usize,
        _hostname_width: usize,
        _vendor_width: usize,
        terminal_width: u16,
        row_idx: usize,
    ) -> io::Result<()> {
        let ip_width: usize = 16;
        let alive_width: usize = 12;
        let mac_width: usize = 13;
        let hostname_width: usize = 18;
        let vendor_width: usize = 13;
        let (status_str, status_style) = match device.status {
            DeviceUIStatus::Online => (" Online ", Color::Green),
            DeviceUIStatus::Offline => (" Offline ", Color::Red),
            DeviceUIStatus::New => (" New ", Color::Yellow),
            DeviceUIStatus::Lost => (" Lost ", Color::Red),
        };

        let status_display = style::style(format!("{:^8}", status_str)).with(status_style).bold();

        let mac = device
            .mac
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(mac_width.saturating_sub(2))
            .collect::<String>();
        let hostname = device
            .hostname
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(hostname_width.saturating_sub(2))
            .collect::<String>();
        let vendor = device
            .vendor
            .as_deref()
            .unwrap_or("-")
            .chars()
            .take(vendor_width.saturating_sub(2))
            .collect::<String>();

        // 计算存活时间
        let duration = device.last_seen.signed_duration_since(device.first_seen);
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;
        let seconds = duration.num_seconds() % 60;
        let alive_str = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        let row_content = if self.show_details {
            format!(
                "{:<ip_w$} {:<alive_w$} {:<mac_w$} {:<host_w$} {:<vendor_w$}",
                device.ip.to_string(),
                alive_str,
                mac,
                hostname,
                vendor,
                ip_w = ip_width,
                alive_w = alive_width,
                mac_w = mac_width,
                host_w = hostname_width,
                vendor_w = vendor_width
            )
        } else {
            format!(
                "{:<ip_w$}",
                device.ip.to_string(),
                ip_w = ip_width
            )
        };

        let y_pos = 4 + row_idx as u16;

        execute!(
            stdout,
            cursor::MoveTo(0, y_pos),
            style::SetBackgroundColor(if is_highlighted {
                Color::DarkBlue
            } else {
                Color::Reset
            }),
            style::Print(&row_content),
            style::Print(" "),
            style::PrintStyledContent(status_display),
            style::Print(" ".repeat(terminal_width.saturating_sub(row_content.len() as u16 + 10) as usize)),
            style::SetBackgroundColor(Color::Reset),
        )?;

        Ok(())
    }

    fn fill_remaining_rows(
        &self,
        stdout: &mut io::Stdout,
        rendered: usize,
        visible_rows: usize,
        width: u16,
    ) -> io::Result<()> {
        let remaining = visible_rows.saturating_sub(rendered);
        for i in 0..remaining {
            execute!(
                stdout,
                cursor::MoveTo(0, (4 + rendered + i) as u16),
                terminal::Clear(ClearType::UntilNewLine),
            )?;
        }
        Ok(())
    }

    fn render_footer(
        &self,
        stdout: &mut io::Stdout,
        devices: &[DeviceUIInfo],
        width: u16,
        height: u16,
    ) -> io::Result<()> {
        let online = devices
            .iter()
            .filter(|d| d.status == DeviceUIStatus::Online)
            .count();
        let offline = devices
            .iter()
            .filter(|d| d.status == DeviceUIStatus::Offline)
            .count();
        let new = devices
            .iter()
            .filter(|d| d.status == DeviceUIStatus::New)
            .count();
        let lost = devices
            .iter()
            .filter(|d| d.status == DeviceUIStatus::Lost)
            .count();

        let stats = format!(
            "设备总数: {} | 在线: {} | 离线: {} | 新设备: {} | 丢失: {}",
            devices.len(),
            online,
            offline,
            new,
            lost
        );

        let help = "按键: [q]退出 [d]切换详情 [s]切换排序 [↑/↓]导航";

        execute!(
            stdout,
            cursor::MoveTo(0, (height - 2) as u16),
            style::Print(&stats),
            cursor::MoveTo(0, (height - 1) as u16),
            style::Print(help),
        )
    }

    fn render_separator(&self, stdout: &mut io::Stdout, width: u16, y_pos: u16) -> io::Result<()> {
        execute!(
            stdout,
            cursor::MoveTo(0, y_pos),
            style::Print("─".repeat(width as usize)),
        )
    }

    fn get_sorted_devices(&self) -> Vec<DeviceUIInfo> {
        let devices = self.devices.lock().unwrap();
        let mut online: Vec<DeviceUIInfo> = devices.values().cloned().filter(|d| d.status == DeviceUIStatus::Online || d.status == DeviceUIStatus::New).collect();
        let mut offline: Vec<DeviceUIInfo> = devices.values().cloned().filter(|d| d.status == DeviceUIStatus::Offline || d.status == DeviceUIStatus::Lost).collect();
        online.sort_by_key(|d| d.ip);
        offline.sort_by(|a, b| b.offline_at.cmp(&a.offline_at).then_with(|| a.ip.cmp(&b.ip)));
        online.extend(offline);
        online
    }

    /// 计算可见范围，并修正 highlight_index 和 scroll_offset，返回 (start_idx, end_idx, highlight_index, scroll_offset)
    fn calculate_visible_range_and_highlight(
        device_count: usize,
        visible_rows: usize,
        highlight_index: usize,
        scroll_offset: usize,
    ) -> (usize, usize, usize, usize) {
        let mut hi = highlight_index.min(device_count.saturating_sub(1));
        let mut so = scroll_offset.min(hi);
        if hi < so {
            so = hi;
        }
        let start_idx = so.min(device_count.saturating_sub(1));
        let end_idx = (start_idx + visible_rows).min(device_count);
        (start_idx, end_idx, hi, so)
    }

    pub fn format_device_info(&self, device: &DeviceUIInfo) -> String {
        let mut parts = Vec::new();

        // 添加IP地址
        parts.push(device.ip.to_string());

        // 添加存活时间信息
        let duration = device.last_seen.signed_duration_since(device.first_seen);
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;
        let seconds = duration.num_seconds() % 60;
        parts.push(format!("存活时间: {:02}:{:02}:{:02}", hours, minutes, seconds));

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
}
