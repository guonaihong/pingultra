use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PingStats {
    pub sent: u32,
    pub received: u32,
    pub min_rtt: Option<Duration>,
    pub max_rtt: Option<Duration>,
    pub sum_rtt: Duration,
    pub last_seq: u16,
}

impl PingStats {
    pub fn new() -> Self {
        PingStats {
            sent: 0,
            received: 0,
            min_rtt: None,
            max_rtt: None,
            sum_rtt: Duration::from_secs(0),
            last_seq: 0,
        }
    }
    
    pub fn update_with_success(&mut self, seq: u16, rtt: Duration) {
        self.sent += 1;
        self.received += 1;
        self.last_seq = seq;
        self.sum_rtt += rtt;
        
        if let Some(min_rtt) = self.min_rtt {
            if rtt < min_rtt {
                self.min_rtt = Some(rtt);
            }
        } else {
            self.min_rtt = Some(rtt);
        }
        
        if let Some(max_rtt) = self.max_rtt {
            if rtt > max_rtt {
                self.max_rtt = Some(rtt);
            }
        } else {
            self.max_rtt = Some(rtt);
        }
    }
    
    pub fn update_with_failure(&mut self, seq: u16) {
        self.sent += 1;
        self.last_seq = seq;
    }
    
    pub fn avg_rtt(&self) -> Option<Duration> {
        if self.received > 0 {
            Some(self.sum_rtt / self.received)
        } else {
            None
        }
    }
    
    pub fn loss_percent(&self) -> f64 {
        if self.sent > 0 {
            (1.0 - (self.received as f64 / self.sent as f64)) * 100.0
        } else {
            0.0
        }
    }
}
