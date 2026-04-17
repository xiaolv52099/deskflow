use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use uuid::Uuid;

pub const CURRENT_PROTOCOL_VERSION: u32 = 1;
pub const MIN_SUPPORTED_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelKind {
    Control,
    Input,
    Clipboard,
    Diagnostic,
    FileTransfer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    FileTransfer = 1,
    Diagnostic = 2,
    Clipboard = 3,
    Control = 4,
    Input = 5,
}

impl MessagePriority {
    pub fn for_channel(channel: ChannelKind) -> Self {
        match channel {
            ChannelKind::Control => Self::Control,
            ChannelKind::Input => Self::Input,
            ChannelKind::Clipboard => Self::Clipboard,
            ChannelKind::Diagnostic => Self::Diagnostic,
            ChannelKind::FileTransfer => Self::FileTransfer,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionNegotiation {
    pub current: u32,
    pub min_supported: u32,
}

impl Default for VersionNegotiation {
    fn default() -> Self {
        Self {
            current: CURRENT_PROTOCOL_VERSION,
            min_supported: MIN_SUPPORTED_PROTOCOL_VERSION,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub address: String,
    pub port: u16,
    pub fingerprint_sha256: String,
    pub certificate_pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingCode {
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferFileDescriptor {
    pub file_id: Uuid,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferManifest {
    pub transfer_id: Uuid,
    pub source_device_id: Uuid,
    pub target_device_id: Uuid,
    pub files: Vec<FileTransferFileDescriptor>,
    pub total_bytes: u64,
    pub chunk_size_bytes: u64,
    pub total_chunks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferChunkPayload {
    pub transfer_id: Uuid,
    pub file_id: Uuid,
    pub chunk_index: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferProgressPayload {
    pub transfer_id: Uuid,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub chunk_index: u64,
    pub total_chunks: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProtocolMessage {
    VersionHello(VersionNegotiation),
    Ping,
    Pong,
    Shutdown,
    Ready {
        pid: u32,
    },
    DiscoverProbe(DeviceDescriptor),
    DiscoverAnnounce(DeviceDescriptor),
    DiscoverWithdraw {
        device_id: String,
    },
    PairRequest {
        device: DeviceDescriptor,
        pairing_code: PairingCode,
    },
    PairAccept {
        device_id: String,
    },
    PairReject {
        device_id: String,
        reason: String,
    },
    SessionHeartbeat {
        device_id: String,
        sequence: u64,
    },
    SessionResume {
        device_id: String,
        session_id: String,
    },
    FileTransferOffer(FileTransferManifest),
    FileTransferAccept {
        transfer_id: Uuid,
        target_device_id: Uuid,
    },
    FileTransferReject {
        transfer_id: Uuid,
        reason: String,
    },
    FileTransferChunk(FileTransferChunkPayload),
    FileTransferProgress(FileTransferProgressPayload),
    FileTransferCancel {
        transfer_id: Uuid,
        reason: String,
    },
    Diagnostic {
        message: String,
    },
}

impl ProtocolMessage {
    pub fn channel(&self) -> ChannelKind {
        match self {
            ProtocolMessage::VersionHello(_) => ChannelKind::Control,
            ProtocolMessage::Ping => ChannelKind::Control,
            ProtocolMessage::Pong => ChannelKind::Control,
            ProtocolMessage::Shutdown => ChannelKind::Control,
            ProtocolMessage::Ready { .. } => ChannelKind::Control,
            ProtocolMessage::DiscoverProbe(_) => ChannelKind::Control,
            ProtocolMessage::DiscoverAnnounce(_) => ChannelKind::Control,
            ProtocolMessage::DiscoverWithdraw { .. } => ChannelKind::Control,
            ProtocolMessage::PairRequest { .. } => ChannelKind::Control,
            ProtocolMessage::PairAccept { .. } => ChannelKind::Control,
            ProtocolMessage::PairReject { .. } => ChannelKind::Control,
            ProtocolMessage::SessionHeartbeat { .. } => ChannelKind::Control,
            ProtocolMessage::SessionResume { .. } => ChannelKind::Control,
            ProtocolMessage::FileTransferOffer(_) => ChannelKind::FileTransfer,
            ProtocolMessage::FileTransferAccept { .. } => ChannelKind::FileTransfer,
            ProtocolMessage::FileTransferReject { .. } => ChannelKind::FileTransfer,
            ProtocolMessage::FileTransferChunk(_) => ChannelKind::FileTransfer,
            ProtocolMessage::FileTransferProgress(_) => ChannelKind::FileTransfer,
            ProtocolMessage::FileTransferCancel { .. } => ChannelKind::FileTransfer,
            ProtocolMessage::Diagnostic { .. } => ChannelKind::Diagnostic,
        }
    }

    pub fn priority(&self) -> MessagePriority {
        MessagePriority::for_channel(self.channel())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolFrame {
    pub version: u32,
    pub channel: ChannelKind,
    pub priority: MessagePriority,
    pub message: ProtocolMessage,
}

impl ProtocolFrame {
    pub fn new(message: ProtocolMessage) -> Self {
        Self {
            version: CURRENT_PROTOCOL_VERSION,
            channel: message.channel(),
            priority: message.priority(),
            message,
        }
    }

    pub fn encode_json_line(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(self).context("serialize protocol frame")?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode_json_line(input: &[u8]) -> Result<Self> {
        let bytes = input.strip_suffix(b"\n").unwrap_or(input);
        serde_json::from_slice(bytes).context("deserialize protocol frame")
    }
}

pub fn negotiate_protocol(local: VersionNegotiation, remote: VersionNegotiation) -> Result<u32> {
    if local.current < remote.min_supported || remote.current < local.min_supported {
        anyhow::bail!(
            "protocol negotiation failed: local current={} min={}, remote current={} min={}",
            local.current,
            local.min_supported,
            remote.current,
            remote.min_supported
        );
    }

    Ok(local.current.min(remote.current))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedFrame {
    pub sequence: u64,
    pub frame: ProtocolFrame,
}

impl Ord for QueuedFrame {
    fn cmp(&self, other: &Self) -> Ordering {
        self.frame
            .priority
            .cmp(&other.frame.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for QueuedFrame {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn drain_priority_order(frames: Vec<ProtocolFrame>) -> Vec<ProtocolFrame> {
    let mut heap = BinaryHeap::new();

    for (sequence, frame) in frames.into_iter().enumerate() {
        heap.push(QueuedFrame {
            sequence: sequence as u64,
            frame,
        });
    }

    let mut ordered = Vec::new();
    while let Some(item) = heap.pop() {
        ordered.push(item.frame);
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_frame_roundtrip() {
        let frame = ProtocolFrame::new(ProtocolMessage::Ping);
        let encoded = frame.encode_json_line().expect("encode frame");
        let decoded = ProtocolFrame::decode_json_line(&encoded).expect("decode frame");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn protocol_negotiation_succeeds_for_matching_versions() {
        let agreed =
            negotiate_protocol(VersionNegotiation::default(), VersionNegotiation::default())
                .expect("negotiate protocol");
        assert_eq!(agreed, CURRENT_PROTOCOL_VERSION);
    }

    #[test]
    fn priority_order_prefers_higher_priority_first() {
        let frames = vec![
            ProtocolFrame::new(ProtocolMessage::Diagnostic {
                message: "diag".into(),
            }),
            ProtocolFrame {
                version: CURRENT_PROTOCOL_VERSION,
                channel: ChannelKind::Input,
                priority: MessagePriority::Input,
                message: ProtocolMessage::Pong,
            },
            ProtocolFrame::new(ProtocolMessage::Ping),
        ];

        let ordered = drain_priority_order(frames);
        assert_eq!(ordered[0].priority, MessagePriority::Input);
        assert_eq!(ordered[1].priority, MessagePriority::Control);
        assert_eq!(ordered[2].priority, MessagePriority::Diagnostic);
    }

    #[test]
    fn session_messages_roundtrip() {
        let frame = ProtocolFrame::new(ProtocolMessage::PairRequest {
            device: DeviceDescriptor {
                device_id: "device-1".into(),
                display_name: "Deskflow Controller".into(),
                platform: "windows".into(),
                address: "192.168.1.20".into(),
                port: 24800,
                fingerprint_sha256: "abc123".into(),
                certificate_pem: "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----"
                    .into(),
            },
            pairing_code: PairingCode {
                value: "123456".into(),
            },
        });

        let encoded = frame.encode_json_line().expect("encode pair request");
        let decoded = ProtocolFrame::decode_json_line(&encoded).expect("decode pair request");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn file_transfer_messages_use_file_transfer_channel() {
        let transfer_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();
        let message = ProtocolMessage::FileTransferOffer(FileTransferManifest {
            transfer_id,
            source_device_id: Uuid::new_v4(),
            target_device_id: Uuid::new_v4(),
            files: vec![FileTransferFileDescriptor {
                file_id,
                name: "archive.zip".into(),
                size_bytes: 4096,
            }],
            total_bytes: 4096,
            chunk_size_bytes: 1024,
            total_chunks: 4,
        });

        let frame = ProtocolFrame::new(message);
        assert_eq!(frame.channel, ChannelKind::FileTransfer);
        assert_eq!(frame.priority, MessagePriority::FileTransfer);
    }

    #[test]
    fn file_transfer_chunk_roundtrip_preserves_payload() {
        let frame = ProtocolFrame::new(ProtocolMessage::FileTransferChunk(
            FileTransferChunkPayload {
                transfer_id: Uuid::new_v4(),
                file_id: Uuid::new_v4(),
                chunk_index: 3,
                offset: 3072,
                bytes: vec![1, 2, 3, 4],
                checksum_sha256: "abcd".into(),
            },
        ));

        let encoded = frame.encode_json_line().expect("encode file chunk");
        let decoded = ProtocolFrame::decode_json_line(&encoded).expect("decode file chunk");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn priority_order_keeps_input_ahead_of_file_transfer() {
        let frames = vec![
            ProtocolFrame::new(ProtocolMessage::FileTransferProgress(
                FileTransferProgressPayload {
                    transfer_id: Uuid::new_v4(),
                    transferred_bytes: 128,
                    total_bytes: 1024,
                    chunk_index: 0,
                    total_chunks: 8,
                    status: "InProgress".into(),
                },
            )),
            ProtocolFrame {
                version: CURRENT_PROTOCOL_VERSION,
                channel: ChannelKind::Input,
                priority: MessagePriority::Input,
                message: ProtocolMessage::Pong,
            },
            ProtocolFrame::new(ProtocolMessage::Ping),
        ];

        let ordered = drain_priority_order(frames);
        assert_eq!(ordered[0].priority, MessagePriority::Input);
        assert_eq!(ordered[1].priority, MessagePriority::Control);
        assert_eq!(ordered[2].priority, MessagePriority::FileTransfer);
    }
}
