use pnet::packet::icmp::echo_request::{EchoRequestPacket, MutableEchoRequestPacket};
use pnet::packet::icmp::{IcmpPacket, IcmpTypes};
use pnet::packet::Packet;
use std::time::{Duration, Instant};

use crate::error::PingError;

pub struct IcmpEchoRequest {
    pub identifier: u16,
    #[allow(dead_code)]
    pub sequence: u16,
    pub payload_size: usize,
}

pub struct IcmpEchoReply {
    #[allow(dead_code)]
    pub identifier: u16,
    #[allow(dead_code)]
    pub sequence: u16,
    pub ttl: u8,
    pub size: usize,
    pub rtt: Duration,
}

impl IcmpEchoRequest {
    pub fn new(identifier: u16, sequence: u16, payload_size: usize) -> Self {
        Self {
            identifier,
            sequence,
            payload_size,
        }
    }

    pub fn create_packet<'a>(
        &self,
        buffer: &'a mut [u8],
    ) -> Result<MutableEchoRequestPacket<'a>, PingError> {
        // 首先填充payload
        let payload_offset = EchoRequestPacket::minimum_packet_size();
        let payload_size = self.payload_size.saturating_sub(payload_offset);

        if payload_size > 0 && buffer.len() >= payload_offset + payload_size {
            for i in 0..payload_size {
                buffer[payload_offset + i] = (i % 256) as u8;
            }
        }

        // 然后创建packet
        let mut packet =
            MutableEchoRequestPacket::new(buffer).ok_or(PingError::PacketConstructionError)?;

        packet.set_icmp_type(IcmpTypes::EchoRequest);
        packet.set_icmp_code(pnet::packet::icmp::IcmpCode::new(0));
        packet.set_sequence_number(self.sequence);
        packet.set_identifier(self.identifier);

        // Calculate and set the checksum
        let checksum = pnet::packet::icmp::checksum(&IcmpPacket::new(packet.packet()).unwrap());
        packet.set_checksum(checksum);

        Ok(packet)
    }
}

pub fn parse_echo_reply(
    buffer: &[u8],
    offset: usize,
    expected_id: u16,
    expected_seq: u16,
    start_time: Instant,
    ttl: u8,
) -> Option<IcmpEchoReply> {
    if buffer.len() < offset + IcmpPacket::minimum_packet_size() {
        return None;
    }

    let icmp_packet = IcmpPacket::new(&buffer[offset..])?;

    if icmp_packet.get_icmp_type() != IcmpTypes::EchoReply {
        return None;
    }

    // For ICMP Echo Reply, we need to cast to EchoRequestPacket to access sequence and identifier
    // This is a bit of a hack, but the underlying structure is the same
    let echo_packet = EchoRequestPacket::new(&buffer[offset..])?;

    let seq = echo_packet.get_sequence_number();
    let id = echo_packet.get_identifier();

    if id != expected_id || seq != expected_seq {
        return None;
    }

    Some(IcmpEchoReply {
        identifier: id,
        sequence: seq,
        ttl,
        size: buffer.len() - offset,
        rtt: start_time.elapsed(),
    })
}
