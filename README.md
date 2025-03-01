# PingUltra

PingUltra 是一个用 Rust 实现的高性能网络 ping 工具，是 fping 的兼容升级版本。它支持并行 ping 多个主机，提供丰富的统计信息和多种输出格式。

## 特性

- 高性能并行 ping 多个主机
- 支持 IPv4 和 IPv6
- 自定义 ICMP 包大小
- 可配置的超时和重试机制
- 多种输出格式（文本、JSON、CSV）
- 详细的统计信息
- 支持从文件读取目标主机列表
- **内网设备监控**：检测局域网中设备的新增和消失

## 安装

### 从源码编译

确保你已经安装了 Rust 和 Cargo：

```bash
# 克隆仓库
git clone https://github.com/guonaihong/pingultra.git
cd pingultra

# 编译
cargo build --release

# 安装（可选）
sudo cp target/release/pingultra /usr/local/bin/
```

## 使用方法

由于 PingUltra 需要创建原始套接字（raw socket）来发送 ICMP 包，所以在大多数系统上需要 root 权限运行：

```bash
# 基本用法
sudo pingultra 8.8.8.8

# 指定 ping 次数
sudo pingultra -c 5 8.8.8.8

# 同时 ping 多个主机
sudo pingultra 8.8.8.8 1.1.1.1 example.com

# 从文件读取目标主机
sudo pingultra -f hosts.txt

# 设置超时时间（毫秒）
sudo pingultra -w 1000 8.8.8.8

# 设置 ping 间隔（毫秒）
sudo pingultra -p 500 8.8.8.8

# 设置 ICMP 包大小
sudo pingultra -s 100 8.8.8.8

# 设置 TTL 值
sudo pingultra -t 32 8.8.8.8

# 生成 JSON 格式的摘要报告
sudo pingultra 8.8.8.8 summary --format json

# 监控内网设备变化（新增/消失）
sudo pingultra monitor -n 192.168.1.0/24

# 监控内网设备变化，并解析 MAC 地址厂商信息
sudo pingultra monitor -n 192.168.1.0/24 -m

# 只显示设备变化（新增/消失），不显示稳定设备
sudo pingultra monitor -n 192.168.1.0/24 --changes-only
```

## 命令行选项

```
选项:
  -f, --file <FILE>          从文件读取目标主机
  -c, --count <COUNT>        发送 ping 的次数 [默认: 3]
  -p, --period <PERIOD>      ping 间隔时间（毫秒）[默认: 1000]
  -w, --timeout <TIMEOUT>    超时时间（毫秒）[默认: 5000]
  -r, --retry <RETRY>        失败重试次数 [默认: 1]
  -s, --size <SIZE>          ICMP 包大小（字节）[默认: 56]
  -t, --ttl <TTL>            生存时间值 [默认: 64]
  -q, --quiet                安静模式 - 只显示摘要
  -T, --timestamp            显示时间戳
  -h, --help                 显示帮助信息
  -V, --version              显示版本信息

子命令:
  summary                    生成摘要报告
  monitor                    监控内网设备变化
  help                       显示命令帮助
```

### 监控子命令选项

```
选项:
  -n, --network <NETWORK>    要扫描的 IP 范围（CIDR 格式，如 192.168.1.0/24）
  -i, --interval <INTERVAL>  扫描间隔时间（秒）[默认: 60]
  -f, --format <FORMAT>      输出格式（text, json, csv）[默认: text]
  -c, --changes-only         只显示变化（不显示稳定设备）
  -m, --resolve-mac          尽可能解析 MAC 地址到厂商名称
```

## 项目结构

```
src/
├── cli.rs         # 命令行参数处理
├── error.rs       # 错误类型定义
├── host.rs        # 主机和响应处理
├── icmp.rs        # ICMP 包构造和解析
├── main.rs        # 程序入口
├── monitor.rs     # 内网设备监控功能
├── output.rs      # 输出格式化
├── pinger.rs      # 核心 ping 功能实现
└── stats.rs       # 统计信息收集和处理
```

## 性能

PingUltra 使用 Rust 的异步编程模型和高效的网络库，能够以最小的资源消耗同时 ping 大量主机。与传统的 ping 工具相比，PingUltra 在处理大量主机时表现出色。

## 贡献

欢迎提交 Pull Request 或 Issue 来帮助改进这个项目。

## 许可证

MIT License
