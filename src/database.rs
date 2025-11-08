use chrono::{DateTime, Local};
use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// 离线事件数据库记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineEventRecord {
    pub id: i64,
    pub ip: String,
    pub offline_at: DateTime<Local>,
    pub online_at: Option<DateTime<Local>>,
    pub duration_ms: i64,
}

/// 数据库管理器
#[derive(Clone)]
pub struct Database {
    conn: std::sync::Arc<std::sync::Mutex<Connection>>,
}

impl Database {
    /// 创建或打开数据库
    pub fn new(db_path: &str) -> SqlResult<Self> {
        let conn = Connection::open(db_path)?;
        let db = Database {
            conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
        };
        db.init_tables()?;
        Ok(db)
    }

    /// 初始化数据库表
    fn init_tables(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS offline_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ip TEXT NOT NULL,
                offline_at DATETIME NOT NULL,
                online_at DATETIME,
                duration_ms INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_ip_offline_at ON offline_events(ip, offline_at DESC);
            CREATE INDEX IF NOT EXISTS idx_created_at ON offline_events(created_at DESC);
            ",
        )?;
        Ok(())
    }

    /// 记录离线事件
    pub fn record_offline_event(
        &self,
        ip: &IpAddr,
        offline_at: DateTime<Local>,
        online_at: Option<DateTime<Local>>,
        duration_ms: u64,
    ) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO offline_events (ip, offline_at, online_at, duration_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![ip.to_string(), offline_at, online_at, duration_ms as i64,],
        )?;
        Ok(())
    }

    /// 获取指定 IP 的离线事件历史
    pub fn get_offline_events(&self, ip: &IpAddr) -> SqlResult<Vec<OfflineEventRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ip, offline_at, online_at, duration_ms
             FROM offline_events
             WHERE ip = ?1
             ORDER BY offline_at DESC
             LIMIT 100",
        )?;

        let events = stmt.query_map(params![ip.to_string()], |row| {
            Ok(OfflineEventRecord {
                id: row.get(0)?,
                ip: row.get(1)?,
                offline_at: row.get(2)?,
                online_at: row.get(3)?,
                duration_ms: row.get(4)?,
            })
        })?;

        let mut result = Vec::new();
        for event in events {
            result.push(event?);
        }
        Ok(result)
    }

    /// 获取指定 IP 今天的离线次数
    #[allow(dead_code)]
    pub fn get_today_offline_count(&self, ip: &IpAddr) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT COUNT(*) FROM offline_events
             WHERE ip = ?1
             AND DATE(offline_at) = DATE('now', 'localtime')",
        )?;

        let count: i64 = stmt.query_row(params![ip.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    /// 获取指定 IP 今天的平均离线时长（秒）
    #[allow(dead_code)]
    pub fn get_today_avg_offline_duration(&self, ip: &IpAddr) -> SqlResult<f64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT AVG(duration_ms) FROM offline_events
             WHERE ip = ?1
             AND DATE(offline_at) = DATE('now', 'localtime')",
        )?;

        let avg: Option<f64> = stmt.query_row(params![ip.to_string()], |row| row.get(0))?;
        Ok(avg.unwrap_or(0.0) / 1000.0) // 转换为秒
    }

    /// 获取指定 IP 的总离线次数
    #[allow(dead_code)]
    pub fn get_total_offline_count(&self, ip: &IpAddr) -> SqlResult<i64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM offline_events WHERE ip = ?1")?;

        let count: i64 = stmt.query_row(params![ip.to_string()], |row| row.get(0))?;
        Ok(count)
    }

    /// 获取指定 IP 的总离线时长（秒）
    #[allow(dead_code)]
    pub fn get_total_offline_duration(&self, ip: &IpAddr) -> SqlResult<f64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT SUM(duration_ms) FROM offline_events WHERE ip = ?1")?;

        let total: Option<i64> = stmt.query_row(params![ip.to_string()], |row| row.get(0))?;
        Ok(total.unwrap_or(0) as f64 / 1000.0) // 转换为秒
    }

    /// 获取所有设备的离线统计
    #[allow(dead_code)]
    pub fn get_all_devices_stats(&self) -> SqlResult<Vec<(String, i64, f64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ip, COUNT(*) as count, AVG(duration_ms) as avg_duration
             FROM offline_events
             WHERE DATE(offline_at) = DATE('now', 'localtime')
             GROUP BY ip
             ORDER BY count DESC",
        )?;

        let stats = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<f64>>(2)?.unwrap_or(0.0) / 1000.0,
            ))
        })?;

        let mut result = Vec::new();
        for stat in stats {
            result.push(stat?);
        }
        Ok(result)
    }

    /// 清理旧数据（保留最近 30 天）
    #[allow(dead_code)]
    pub fn cleanup_old_data(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM offline_events
             WHERE DATE(offline_at) < DATE('now', '-30 days')",
            [],
        )?;
        Ok(())
    }

    /// 导出数据为 JSON
    #[allow(dead_code)]
    pub fn export_to_json(&self, ip: &IpAddr) -> SqlResult<String> {
        let events = self.get_offline_events(ip)?;
        let json = serde_json::to_string_pretty(&events).unwrap_or_default();
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation() {
        let _db = Database::new(":memory:").unwrap();
        // 测试通过即可
    }
}
