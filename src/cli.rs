use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about = "A fast ping utility implemented in Rust", long_about = None)]
pub struct Cli {
    /// Target hosts (IP addresses or hostnames)
    #[arg(required = false)]
    pub hosts: Vec<String>,

    /// Read targets from a file
    #[arg(short, long)]
    pub file: Option<String>,

    /// Number of pings to send to each target
    #[arg(short = 'c', long, default_value = "3")]
    pub count: u32,

    /// Time between pings in milliseconds
    #[arg(short = 'p', long, default_value = "1000")]
    pub period: u64,

    /// Timeout in milliseconds
    #[arg(short = 'w', long, default_value = "5000")]
    pub timeout: u64,

    /// Retry count for failed pings
    #[arg(short = 'r', long, default_value = "1")]
    pub retry: u32,

    /// Size of the ICMP packet in bytes
    #[arg(short = 's', long, default_value = "56")]
    pub size: usize,

    /// Time to live
    #[arg(short = 't', long, default_value = "64")]
    pub ttl: u32,

    /// Quiet mode - only show summary
    #[arg(short, long)]
    pub quiet: bool,

    /// Show timestamps
    #[arg(short = 'T', long)]
    pub timestamp: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate a summary report
    Summary {
        /// Output format (text, json, csv)
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Monitor network for device changes (additions/removals)
    Monitor {
        /// IP range to scan (CIDR notation, e.g., 192.168.1.0/24)
        #[arg(short = 'n', long, required = true)]
        network: String,

        /// Scan interval in seconds
        #[arg(short, long, default_value = "60")]
        interval: u64,

        /// Output format (text, json, csv)
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Only show changes (don't display stable devices)
        #[arg(short, long)]
        changes_only: bool,

        /// Resolve MAC addresses to vendor names when possible
        #[arg(short = 'm', long)]
        resolve_mac: bool,

        /// Use character-based UI for monitoring
        #[arg(short = 'u', long)]
        ui: bool,
    },
}
