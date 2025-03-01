mod cli;
mod error;
mod host;
mod icmp;
mod monitor;
mod output;
mod pinger;
mod stats;

use anyhow::Result;
use cli::Cli;
use clap::Parser;
use colored::Colorize;
use error::PingError;
use host::{load_hosts_from_file, resolve_host, PingTarget};
use monitor::NetworkMonitor;
use output::{print_csv_summary, print_json_summary, print_ping_result, print_ping_start, print_ping_summary};
use pinger::Pinger;
use stats::PingStats;
use std::collections::HashMap;
use std::process;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    // 处理子命令
    if let Some(command) = &cli.command {
        match command {
            cli::Commands::Summary { format } => {
                // 摘要命令需要在收集完统计信息后处理
                // 所以这里不立即返回
            },
            cli::Commands::Monitor { network, interval, format, changes_only, resolve_mac } => {
                // 启动网络监控模式
                match NetworkMonitor::new(network, *interval, *resolve_mac, *changes_only) {
                    Ok(mut monitor) => {
                        if let Err(e) = monitor.start_monitoring().await {
                            eprintln!("Error during network monitoring: {}", e);
                            process::exit(1);
                        }
                        return Ok(());
                    },
                    Err(e) => {
                        eprintln!("Error setting up network monitor: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
    }
    
    // Load hosts from command line or file
    let mut hosts = cli.hosts.clone();
    if let Some(file_path) = &cli.file {
        match load_hosts_from_file(file_path) {
            Ok(file_hosts) => hosts.extend(file_hosts),
            Err(e) => {
                eprintln!("Error loading hosts from file {}: {}", file_path, e);
                process::exit(1);
            }
        }
    }
    
    // 如果没有提供主机且没有使用子命令，显示错误信息
    if hosts.is_empty() && cli.command.is_none() {
        eprintln!("Error: No target hosts specified. Use --help for usage information.");
        process::exit(1);
    }
    
    // 如果没有提供主机但使用了摘要子命令，显示错误信息
    if hosts.is_empty() && cli.command.is_some() {
        if let Some(cli::Commands::Summary { .. }) = &cli.command {
            eprintln!("Error: No target hosts specified for summary. Use --help for usage information.");
            process::exit(1);
        }
    }
    
    // Setup signal handling for graceful termination
    let running = Arc::new(Mutex::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        let mut running = r.lock().unwrap();
        *running = false;
        println!("\nInterrupted, exiting...");
    })?;
    
    // Channel for collecting ping results
    let (tx, mut rx) = mpsc::channel(100);
    
    // Start ping tasks for each host
    let mut tasks = vec![];
    for host_str in hosts {
        match resolve_host(&host_str) {
            Ok(addr) => {
                let target = PingTarget::new(host_str.clone(), addr);
                
                if !cli.quiet {
                    print_ping_start(&target.name, &target.addr.to_string(), cli.size);
                }
                
                match Pinger::new(target.clone(), cli.size, cli.ttl) {
                    Ok(pinger) => {
                        let tx_clone = tx.clone();
                        let task = tokio::spawn(async move {
                            if let Err(e) = pinger.ping_multiple(
                                cli.count,
                                cli.period,
                                cli.timeout,
                                cli.retry,
                                tx_clone,
                            ).await {
                                eprintln!("Error pinging {}: {}", target.name, e);
                            }
                        });
                        tasks.push(task);
                    },
                    Err(PingError::PermissionDenied) => {
                        eprintln!("{}", "Error: Raw sockets require root privileges. Please run with sudo.".red());
                        process::exit(1);
                    },
                    Err(e) => {
                        eprintln!("Error creating pinger for {}: {}", host_str, e);
                    }
                }
            },
            Err(e) => {
                eprintln!("Could not resolve host {}: {}", host_str, e);
            }
        }
    }
    
    // Drop the original sender so the channel can close when all tasks are done
    drop(tx);
    
    // Track statistics for each host
    let mut host_stats: HashMap<String, PingStats> = HashMap::new();
    
    // Process results as they come in
    while let Some(response) = rx.recv().await {
        if !cli.quiet {
            print_ping_result(&response, cli.timestamp);
        }
        
        let stats = host_stats.entry(response.target.name.clone()).or_insert_with(PingStats::new);
        
        if response.is_success() {
            stats.update_with_success(response.seq, response.rtt.unwrap());
        } else {
            stats.update_with_failure(response.seq);
        }
        
        // Check if we should exit early due to Ctrl-C
        if !*running.lock().unwrap() {
            break;
        }
    }
    
    // Print summary for each host
    if let Some(command) = &cli.command {
        match command {
            cli::Commands::Summary { format } => {
                match format.as_str() {
                    "json" => {
                        for (host, stats) in &host_stats {
                            println!("{}", print_json_summary(host, stats));
                        }
                    },
                    "csv" => {
                        // Print header only once
                        println!("host,packets_transmitted,packets_received,packet_loss_percent,rtt_min_ms,rtt_avg_ms,rtt_max_ms");
                        for (host, stats) in &host_stats {
                            let csv = print_csv_summary(host, stats);
                            // Skip the header line
                            if let Some(pos) = csv.find('\n') {
                                println!("{}", &csv[pos+1..]);
                            }
                        }
                    },
                    _ => {
                        for (host, stats) in &host_stats {
                            print_ping_summary(host, stats);
                        }
                    }
                }
            },
            _ => {} // 其他命令已经在前面处理过了
        }
    } else {
        for (host, stats) in &host_stats {
            print_ping_summary(host, stats);
        }
    }
    
    Ok(())
}
