use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::Ipv4Packet;
use pnet::packet::Packet;
use rand::random;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time;
use std::mem::MaybeUninit;

use crate::error::PingError;
use crate::host::{PingResponse, PingTarget};
use crate::icmp::{IcmpEchoRequest, parse_echo_reply};

/// Pinger结构体，用于发送和接收ICMP包
pub struct Pinger {
    /// ICMP包的标识符
    #[allow(dead_code)]
    identifier: u16,
    /// socket对象，用于发送和接收ICMP包
    socket: Socket,
    /// 目标主机的信息
    target: PingTarget,
    /// ICMP包的大小
    size: usize,
    /// TTL值
    ttl: u32,
}

impl Pinger {
    /// 创建一个新的Pinger对象
    ///
    /// # 参数
    ///
    /// * `target`: 目标主机的信息
    /// * `size`: ICMP包的大小
    /// * `ttl`: TTL值
    ///
    /// # 返回值
    ///
    /// * `Result<Self, PingError>`: 如果创建成功，返回Pinger对象；如果创建失败，返回错误信息
    pub fn new(target: PingTarget, size: usize, ttl: u32) -> Result<Self, PingError> {
        let identifier = random::<u16>();
        
        let socket = match target.addr {
            IpAddr::V4(_) => {
                let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            PingError::PermissionDenied
                        } else {
                            PingError::SendError(e)
                        }
                    })?;
                socket.set_ttl(ttl)?;
                socket
            },
            IpAddr::V6(_) => {
                let socket = Socket::new(Domain::IPV6, Type::RAW, Some(Protocol::ICMPV6))
                    .map_err(|e| {
                        if e.kind() == std::io::ErrorKind::PermissionDenied {
                            PingError::PermissionDenied
                        } else {
                            PingError::SendError(e)
                        }
                    })?;
                socket.set_unicast_hops_v6(ttl)?;
                socket
            },
        };
        
        socket.set_nonblocking(true)?;
        
        Ok(Self {
            identifier,
            socket,
            target,
            size,
            ttl,
        })
    }
    
    /// 发送一个ICMP包并等待响应
    ///
    /// # 参数
    ///
    /// * `seq`: ICMP包的序号
    /// * `timeout_ms`: 等待响应的超时时间（毫秒）
    ///
    /// # 返回值
    ///
    /// * `PingResponse`: ICMP包的响应信息
    pub async fn ping_once(&self, seq: u16, timeout_ms: u64) -> PingResponse {
        let mut buffer = vec![0; self.size];
        let request = IcmpEchoRequest::new(self.identifier, seq, self.size);
        
        match request.create_packet(&mut buffer) {
            Ok(packet) => {
                let socket_addr = SocketAddr::new(self.target.addr, 0);
                let start = Instant::now();
                
                match self.socket.send_to(packet.packet(), &socket_addr.into()) {
                    Ok(_) => {
                        // Create a buffer for receiving with MaybeUninit
                        let mut recv_buffer = [MaybeUninit::new(0u8); 2048];
                        
                        // Wait for response with timeout
                        let timeout_duration = Duration::from_millis(timeout_ms);
                        let timeout_instant = start + timeout_duration;
                        
                        loop {
                            let now = Instant::now();
                            if now >= timeout_instant {
                                return PingResponse::failure(
                                    self.target.clone(),
                                    seq,
                                    self.size,
                                    self.ttl as u8,
                                    PingError::Timeout,
                                );
                            }
                            
                            // Use socket2's recv with MaybeUninit buffer
                            match self.socket.recv(&mut recv_buffer) {
                                Ok(len) => {
                                    // Convert MaybeUninit buffer to initialized buffer for processing
                                    let recv_data = unsafe {
                                        std::slice::from_raw_parts(
                                            recv_buffer.as_ptr() as *const u8,
                                            len
                                        )
                                    };
                                    
                                    // Parse the received packet
                                    if len >= Ipv4Packet::minimum_packet_size() {
                                        if let Some(ipv4_packet) = Ipv4Packet::new(recv_data) {
                                            if ipv4_packet.get_next_level_protocol() == IpNextHeaderProtocols::Icmp {
                                                let icmp_packet_offset = (ipv4_packet.get_header_length() * 4) as usize;
                                                
                                                if let Some(reply) = parse_echo_reply(
                                                    recv_data,
                                                    icmp_packet_offset,
                                                    self.identifier,
                                                    seq,
                                                    start,
                                                    ipv4_packet.get_ttl(),
                                                ) {
                                                    return PingResponse::success(
                                                        self.target.clone(),
                                                        seq,
                                                        reply.rtt,
                                                        reply.size,
                                                        reply.ttl,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    
                                    // Continue waiting if this wasn't our packet
                                    time::sleep(Duration::from_millis(1)).await;
                                },
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                    // No data available yet, wait a bit and try again
                                    time::sleep(Duration::from_millis(1)).await;
                                },
                                Err(e) => {
                                    return PingResponse::failure(
                                        self.target.clone(),
                                        seq,
                                        self.size,
                                        self.ttl as u8,
                                        PingError::SendError(e),
                                    );
                                }
                            }
                        }
                    },
                    Err(e) => {
                        PingResponse::failure(
                            self.target.clone(),
                            seq,
                            self.size,
                            self.ttl as u8,
                            PingError::SendError(e),
                        )
                    }
                }
            },
            Err(e) => {
                // 这里的e是PingError类型，直接传递
                PingResponse::failure(
                    self.target.clone(),
                    seq,
                    self.size,
                    self.ttl as u8,
                    e,
                )
            }
        }
    }
    
    /// 发送多个ICMP包并等待响应
    ///
    /// # 参数
    ///
    /// * `count`: 发送的ICMP包数量
    /// * `period_ms`: 发送ICMP包之间的间隔时间（毫秒）
    /// * `timeout_ms`: 等待响应的超时时间（毫秒）
    /// * `retry`: 如果发送失败，重试的次数
    /// * `tx`: 用于发送响应信息的通道
    ///
    /// # 返回值
    ///
    /// * `Result<(), PingError>`: 如果发送成功，返回Ok(());如果发送失败，返回错误信息
    pub async fn ping_multiple(
        &self,
        count: u32,
        period_ms: u64,
        timeout_ms: u64,
        retry: u32,
        tx: mpsc::Sender<PingResponse>,
    ) -> Result<(), PingError> {
        let mut seq_num = 0;
        
        for _ in 0..count {
            let mut retry_count = 0;
            let mut success = false;
            
            while retry_count <= retry && !success {
                let response = self.ping_once(seq_num, timeout_ms).await;
                
                if response.is_success() {
                    success = true;
                } else {
                    retry_count += 1;
                }
                
                match tx.send(response).await {
                    Ok(_) => {},
                    Err(_) => {
                        // 接收方已关闭，我们可以安全地退出
                        return Ok(());
                    }
                }
                
                if !success && retry_count <= retry {
                    // Wait a short time before retrying
                    time::sleep(Duration::from_millis(100)).await;
                }
            }
            
            seq_num += 1;
            
            // Wait for the specified period before sending the next ping
            if seq_num < count as u16 {
                time::sleep(Duration::from_millis(period_ms)).await;
            }
        }
        
        Ok(())
    }
}
