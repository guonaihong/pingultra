use chrono::{DateTime, Local};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{self, Color, Stylize},
    terminal::{self, ClearType},
};
use std::cmp::Ordering;
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
    Unstable,
    New,
    Lost,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Ip,
    AliveDuration,
}

#[derive(Debug, Clone)]
pub struct OfflineEvent {
    pub offline_at: DateTime<Local>,
    pub online_at: Option<DateTime<Local>>,
    pub duration_ms: u64,
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
    pub offline_events: Vec<OfflineEvent>,
    pub consecutive_failures: u32,
    pub last_failure_time: Option<Instant>,
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
            offline_events: Vec::new(),
            consecutive_failures: 0,
            last_failure_time: None,
        }
    }
}

fn status_rank(status: &DeviceUIStatus) -> u8 {
    match status {
        DeviceUIStatus::Online | DeviceUIStatus::New => 0,
        DeviceUIStatus::Unstable => 1,
        DeviceUIStatus::Offline | DeviceUIStatus::Lost => 2,
    }
}

#[derive(Clone, PartialEq)]
pub enum UIViewMode {
    List,   // 设备列表视图
    Detail, // 设备详情视图
}

#[derive(Clone)]
pub struct CharacterUI {
    devices: Arc<Mutex<HashMap<IpAddr, DeviceUIInfo>>>,
    running: Arc<Mutex<bool>>,
    sort_mode: SortMode,
    highlight_index: usize,
    scroll_offset: usize,
    view_mode: UIViewMode,
    detail_scroll_offset: usize,
    db: Option<Arc<crate::database::Database>>,
}

impl CharacterUI {
    pub fn new(running: Arc<Mutex<bool>>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            running,
            sort_mode: SortMode::Ip,
            highlight_index: 0,
            scroll_offset: 0,
            view_mode: UIViewMode::List,
            detail_scroll_offset: 0,
            db: None,
        }
    }

    pub fn with_database(mut self, db: crate::database::Database) -> Self {
        self.db = Some(Arc::new(db));
        self
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

    #[allow(dead_code)]
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

    pub fn update_device_status(
        &mut self,
        ip: &IpAddr,
        ping_success: bool,
    ) -> Option<(DateTime<Local>, Option<DateTime<Local>>, u64)> {
        const UNSTABLE_THRESHOLD: u32 = 2;
        const OFFLINE_THRESHOLD: u32 = 5;

        let mut devices = self.devices.lock().unwrap();
        if let Some(device) = devices.get_mut(ip) {
            if ping_success {
                // 恢复在线
                if device.consecutive_failures > 0 {
                    if let Some(last_event) = device.offline_events.last_mut() {
                        if last_event.online_at.is_none() {
                            last_event.online_at = Some(Local::now());
                            last_event.duration_ms = last_event
                                .online_at
                                .unwrap()
                                .signed_duration_since(last_event.offline_at)
                                .num_milliseconds()
                                as u64;

                            // 返回离线事件信息供数据库保存
                            let result = (
                                last_event.offline_at,
                                last_event.online_at,
                                last_event.duration_ms,
                            );
                            device.consecutive_failures = 0;
                            device.status = DeviceUIStatus::Online;
                            device.last_failure_time = None;
                            return Some(result);
                        }
                    }
                }
                device.consecutive_failures = 0;
                device.status = DeviceUIStatus::Online;
                device.last_failure_time = None;
            } else {
                device.consecutive_failures += 1;
                device.last_failure_time = Some(Instant::now());

                if device.consecutive_failures == UNSTABLE_THRESHOLD {
                    device.status = DeviceUIStatus::Unstable;
                    device.offline_events.push(OfflineEvent {
                        offline_at: Local::now(),
                        online_at: None,
                        duration_ms: 0,
                    });
                } else if device.consecutive_failures >= OFFLINE_THRESHOLD {
                    device.status = DeviceUIStatus::Offline;
                }
            }
        }
        None
    }

    #[allow(dead_code)]
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
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if self.view_mode == UIViewMode::Detail {
                                self.view_mode = UIViewMode::List;
                                self.detail_scroll_offset = 0;
                            } else {
                                *self.running.lock().unwrap() = false;
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Enter => {
                            if self.view_mode == UIViewMode::List {
                                self.view_mode = UIViewMode::Detail;
                                self.detail_scroll_offset = 0;
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Char('s') => {
                            if self.view_mode == UIViewMode::List {
                                self.sort_mode = match self.sort_mode {
                                    SortMode::Ip => SortMode::AliveDuration,
                                    SortMode::AliveDuration => SortMode::Ip,
                                };
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if self.view_mode == UIViewMode::List {
                                self.handle_up_key();
                            } else if self.detail_scroll_offset > 0 {
                                self.detail_scroll_offset -= 1;
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if self.view_mode == UIViewMode::List {
                                self.handle_down_key()?;
                            } else {
                                self.detail_scroll_offset += 1;
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::PageUp => {
                            if self.view_mode == UIViewMode::List {
                                self.handle_page_up()?;
                            }
                            self.render(&mut stdout)?;
                        }
                        KeyCode::PageDown => {
                            if self.view_mode == UIViewMode::List {
                                self.handle_page_down()?;
                            }
                            self.render(&mut stdout)?;
                        }
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
        match self.view_mode {
            UIViewMode::List => self.render_list_view(stdout),
            UIViewMode::Detail => self.render_detail_view(stdout),
        }
    }

    fn render_list_view(&self, stdout: &mut io::Stdout) -> io::Result<()> {
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
        let (start_idx, end_idx, highlight_index, _scroll_offset) =
            Self::calculate_visible_range_and_highlight(
                device_count,
                visible_rows,
                self.highlight_index,
                self.scroll_offset,
            );
        let visible_devices = &devices[start_idx..end_idx];

        for (idx, device) in visible_devices.iter().enumerate() {
            let absolute_idx = start_idx + idx;
            self.render_device_row(stdout, device, absolute_idx == highlight_index, width, idx)?;
        }

        self.fill_remaining_rows(stdout, visible_devices.len(), visible_rows, width)?;
        self.render_footer(stdout, &devices, width, height)?;
        stdout.flush()
    }

    fn render_detail_view(&self, stdout: &mut io::Stdout) -> io::Result<()> {
        execute!(
            stdout,
            terminal::Clear(ClearType::All),
            cursor::MoveTo(0, 0)
        )?;

        let (width, height) = terminal::size()?;
        let devices = self.get_sorted_devices();

        if devices.is_empty() {
            execute!(stdout, cursor::MoveTo(0, 0), style::Print("没有设备"))?;
            return stdout.flush();
        }

        if self.highlight_index >= devices.len() {
            return stdout.flush();
        }

        let device = &devices[self.highlight_index];
        self.render_device_detail(stdout, device, width, height)?;
        stdout.flush()
    }

    fn render_device_detail(
        &self,
        stdout: &mut io::Stdout,
        device: &DeviceUIInfo,
        width: u16,
        height: u16,
    ) -> io::Result<()> {
        let _status_str = match device.status {
            DeviceUIStatus::Online => "Online",
            DeviceUIStatus::Offline => "Offline",
            DeviceUIStatus::Unstable => "Unstable",
            DeviceUIStatus::New => "New",
            DeviceUIStatus::Lost => "Lost",
        };

        // 标题
        execute!(
            stdout,
            cursor::MoveTo(0, 0),
            style::PrintStyledContent(format!("┌─ {} 详情 ", device.ip).bold().with(Color::Cyan)),
            style::Print("─".repeat(
                (width as usize).saturating_sub(format!("┌─ {} 详情 ", device.ip).len() + 1)
            )),
            style::Print("┐"),
        )?;

        let mut y = 1;

        // MAC 地址
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ MAC: "),
            style::PrintStyledContent(device.mac.as_deref().unwrap_or("-").to_string().green()),
            style::Print(" ".repeat(
                (width as usize).saturating_sub(8 + device.mac.as_deref().unwrap_or("-").len() + 1)
            )),
            style::Print("│"),
        )?;
        y += 1;

        // 厂商
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ 厂商: "),
            style::PrintStyledContent(device.vendor.as_deref().unwrap_or("-").to_string().yellow()),
            style::Print(
                " ".repeat(
                    (width as usize)
                        .saturating_sub(8 + device.vendor.as_deref().unwrap_or("-").len() + 1)
                )
            ),
            style::Print("│"),
        )?;
        y += 1;

        // 主机名
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ Hostname: "),
            style::PrintStyledContent(device.hostname.as_deref().unwrap_or("-").to_string().blue()),
            style::Print(
                " ".repeat(
                    (width as usize)
                        .saturating_sub(12 + device.hostname.as_deref().unwrap_or("-").len() + 1)
                )
            ),
            style::Print("│"),
        )?;
        y += 1;

        // 状态
        let (_status_color, status_display) = match device.status {
            DeviceUIStatus::Online => (Color::Green, "Online"),
            DeviceUIStatus::Offline => (Color::Red, "Offline"),
            DeviceUIStatus::Unstable => (Color::Yellow, "Unstable"),
            DeviceUIStatus::New => (Color::Yellow, "New"),
            DeviceUIStatus::Lost => (Color::Red, "Lost"),
        };

        let status_line = format!(
            "│ 状态: {} (连续失败 {} 次)",
            status_display, device.consecutive_failures
        );
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print(&status_line),
            style::Print(" ".repeat((width as usize).saturating_sub(status_line.len() + 1))),
            style::Print("│"),
        )?;
        y += 2;

        // 离线事件历史标题
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ 离线事件历史:"),
            style::Print(" ".repeat((width as usize).saturating_sub(16))),
            style::Print("│"),
        )?;
        y += 1;

        // 离线事件列表
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ ┌"),
            style::Print("─".repeat((width as usize).saturating_sub(4))),
            style::Print("┐ │"),
        )?;
        y += 1;

        let max_events = (height as usize).saturating_sub(y as usize + 5);

        // 首先显示内存中的离线事件
        let mut event_count = 0;
        for (idx, event) in device
            .offline_events
            .iter()
            .enumerate()
            .skip(self.detail_scroll_offset)
            .take(max_events)
        {
            let offline_time = event.offline_at.format("%H:%M:%S").to_string();
            let online_time = event
                .online_at
                .map(|t| t.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "(进行中)".to_string());
            let duration_str = if event.duration_ms > 0 {
                format!("{}s", event.duration_ms / 1000)
            } else {
                format!("{}ms", event.duration_ms)
            };
            let status_icon = if event.online_at.is_some() {
                "恢复 ✓"
            } else {
                "离线中 ⏱️"
            };

            let event_line = format!(
                "│ │ #{} | {} - {} | {:<5} | {}",
                idx + 1,
                offline_time,
                online_time,
                duration_str,
                status_icon
            );

            execute!(
                stdout,
                cursor::MoveTo(0, y),
                style::Print(&event_line),
                style::Print(" ".repeat((width as usize).saturating_sub(event_line.len() + 1))),
                style::Print("│"),
            )?;
            y += 1;
            event_count += 1;
        }

        // 如果有数据库，显示历史离线事件
        if let Some(ref db) = self.db {
            if let Ok(db_events) = db.get_offline_events(&device.ip) {
                let remaining_space = max_events.saturating_sub(event_count);
                for (idx, db_event) in db_events.iter().enumerate().take(remaining_space) {
                    let offline_time = db_event.offline_at.format("%H:%M:%S").to_string();
                    let online_time = db_event
                        .online_at
                        .map(|t| t.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "(进行中)".to_string());
                    let duration_str = if db_event.duration_ms > 0 {
                        format!("{}s", db_event.duration_ms / 1000)
                    } else {
                        format!("{}ms", db_event.duration_ms)
                    };
                    let status_icon = if db_event.online_at.is_some() {
                        "恢复 ✓"
                    } else {
                        "离线中 ⏱️"
                    };

                    let event_line = format!(
                        "│ │ #{} | {} - {} | {:<5} | {}",
                        device.offline_events.len() + idx + 1,
                        offline_time,
                        online_time,
                        duration_str,
                        status_icon
                    );

                    execute!(
                        stdout,
                        cursor::MoveTo(0, y),
                        style::Print(&event_line),
                        style::Print(
                            " ".repeat((width as usize).saturating_sub(event_line.len() + 1))
                        ),
                        style::Print("│"),
                    )?;
                    y += 1;
                }
            }
        }

        // 关闭事件列表框
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("│ └"),
            style::Print("─".repeat((width as usize).saturating_sub(4))),
            style::Print("┘ │"),
        )?;
        y += 2;

        // 统计信息
        let total_offline = device.offline_events.len();
        let total_duration: u64 = device.offline_events.iter().map(|e| e.duration_ms).sum();
        let avg_duration = if total_offline > 0 {
            total_duration / total_offline as u64
        } else {
            0
        };

        let stats_line = format!(
            "│ 统计: 共 {} 次离线，平均时长 {:.1}s",
            total_offline,
            avg_duration as f64 / 1000.0
        );
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print(&stats_line),
            style::Print(" ".repeat((width as usize).saturating_sub(stats_line.len() + 1))),
            style::Print("│"),
        )?;
        y += 1;

        // 底部边框
        execute!(
            stdout,
            cursor::MoveTo(0, y),
            style::Print("└"),
            style::Print("─".repeat((width as usize).saturating_sub(2))),
            style::Print("┘"),
        )?;

        // 帮助信息
        let help = "按键: [q/ESC]返回列表 [↑/↓/j/k]滚动";
        execute!(stdout, cursor::MoveTo(0, height - 1), style::Print(help),)?;

        Ok(())
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
        _ip_width: usize,
        _mac_width: usize,
        _hostname_width: usize,
        _vendor_width: usize,
    ) -> io::Result<()> {
        let ip_width: usize = 16;
        let alive_width: usize = 12;
        let mac_width: usize = 13;
        let hostname_width: usize = 18;
        let vendor_width: usize = 13;
        let (ip_label, alive_label) = match self.sort_mode {
            SortMode::Ip => ("IP*", "存活时间"),
            SortMode::AliveDuration => ("IP", "存活时间*"),
        };
        let header = format!(
            "{:<ip_w$} {:<alive_w$} {:<mac_w$} {:<host_w$} {:<vendor_w$} {}",
            ip_label,
            alive_label,
            "MAC",
            "Hostname",
            "Vendor",
            "Status",
            ip_w = ip_width,
            alive_w = alive_width,
            mac_w = mac_width,
            host_w = hostname_width,
            vendor_w = vendor_width
        );

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
        terminal_width: u16,
        row_idx: usize,
    ) -> io::Result<()> {
        let ip_width: usize = 16;
        let alive_width: usize = 12;
        let mac_width: usize = 13;
        let hostname_width: usize = 18;
        let vendor_width: usize = 13;
        // 检查设备是否在10秒内新上线
        let is_recently_online = device.last_status_change.elapsed().as_secs() <= 10
            && (device.status == DeviceUIStatus::Online || device.status == DeviceUIStatus::New);

        let (status_str, status_style) = match device.status {
            DeviceUIStatus::Online => (
                " Online ",
                if is_recently_online {
                    Color::Cyan
                } else {
                    Color::Green
                },
            ),
            DeviceUIStatus::Offline => (" Offline ", Color::Red),
            DeviceUIStatus::Unstable => (" Unstable ", Color::Yellow),
            DeviceUIStatus::New => (
                " New ",
                if is_recently_online {
                    Color::Cyan
                } else {
                    Color::Yellow
                },
            ),
            DeviceUIStatus::Lost => (" Lost ", Color::Red),
        };

        let status_display = style::style(format!("{:^8}", status_str))
            .with(status_style)
            .bold();

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

        let row_content = format!(
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
        );

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
            style::Print(
                " ".repeat(terminal_width.saturating_sub(row_content.len() as u16 + 10) as usize)
            ),
            style::SetBackgroundColor(Color::Reset),
        )?;

        Ok(())
    }

    fn fill_remaining_rows(
        &self,
        stdout: &mut io::Stdout,
        rendered: usize,
        visible_rows: usize,
        _width: u16,
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
        _width: u16,
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
        let unstable = devices
            .iter()
            .filter(|d| d.status == DeviceUIStatus::Unstable)
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
            "设备总数: {} | 在线: {} | 不稳定: {} | 离线: {} | 新设备: {} | 丢失: {}",
            devices.len(),
            online,
            unstable,
            offline,
            new,
            lost
        );

        let help = "按键: [q]退出 [Enter]详情 [s]切换排序 [↑/↓/j/k]导航 | 青色Status=10秒内新上线";

        execute!(
            stdout,
            cursor::MoveTo(0, height - 2),
            style::Print(&stats),
            cursor::MoveTo(0, height - 1),
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
        let mut devices: Vec<DeviceUIInfo> = devices.values().cloned().collect();

        devices.sort_by(|a, b| {
            let rank_a = status_rank(&a.status);
            let rank_b = status_rank(&b.status);

            rank_a
                .cmp(&rank_b)
                .then_with(|| match self.sort_mode {
                    SortMode::Ip => a.ip.cmp(&b.ip),
                    SortMode::AliveDuration => {
                        let da = a.last_seen.signed_duration_since(a.first_seen);
                        let db = b.last_seen.signed_duration_since(b.first_seen);
                        db.cmp(&da).then_with(|| a.ip.cmp(&b.ip))
                    }
                })
                .then_with(|| {
                    if matches!(a.status, DeviceUIStatus::Offline | DeviceUIStatus::Lost)
                        && matches!(b.status, DeviceUIStatus::Offline | DeviceUIStatus::Lost)
                    {
                        b.offline_at.cmp(&a.offline_at)
                    } else {
                        Ordering::Equal
                    }
                })
        });

        devices
    }

    /// 计算可见范围，并修正 highlight_index 和 scroll_offset，返回 (start_idx, end_idx, highlight_index, scroll_offset)
    fn calculate_visible_range_and_highlight(
        device_count: usize,
        visible_rows: usize,
        highlight_index: usize,
        scroll_offset: usize,
    ) -> (usize, usize, usize, usize) {
        let hi = highlight_index.min(device_count.saturating_sub(1));
        let mut so = scroll_offset.min(hi);
        if hi < so {
            so = hi;
        }
        let start_idx = so.min(device_count.saturating_sub(1));
        let end_idx = (start_idx + visible_rows).min(device_count);
        (start_idx, end_idx, hi, so)
    }

    #[allow(dead_code)]
    pub fn format_device_info(&self, device: &DeviceUIInfo) -> String {
        let mut parts = Vec::new();

        // 添加IP地址
        parts.push(device.ip.to_string());

        // 添加存活时间信息
        let duration = device.last_seen.signed_duration_since(device.first_seen);
        let hours = duration.num_hours();
        let minutes = duration.num_minutes() % 60;
        let seconds = duration.num_seconds() % 60;
        parts.push(format!(
            "存活时间: {:02}:{:02}:{:02}",
            hours, minutes, seconds
        ));

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
