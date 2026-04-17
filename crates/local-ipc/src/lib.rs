use core_protocol::{ProtocolFrame, ProtocolMessage, VersionNegotiation};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

pub const DEFAULT_ADDR: &str = "127.0.0.1:45821";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiToCoreCommand {
    Ping,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreToUiEvent {
    Pong,
    Ready { protocol_version: u32, pid: u32 },
    ShuttingDown,
}

pub fn core_service_addr() -> String {
    DEFAULT_ADDR.to_string()
}

pub fn core_service_bin(root: &Path) -> PathBuf {
    let mut exe = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("debug")
        .join("core-service");
    if cfg!(windows) {
        exe.set_extension("exe");
    }
    exe
}

pub async fn send_command(command: UiToCoreCommand) -> anyhow::Result<CoreToUiEvent> {
    let stream = timeout(Duration::from_millis(300), TcpStream::connect(core_service_addr()))
        .await
        .map_err(|_| anyhow::anyhow!("connect to core-service timed out"))??;
    let (reader, mut writer) = stream.into_split();

    let hello = ProtocolFrame::new(ProtocolMessage::VersionHello(VersionNegotiation::default()));
    timeout(Duration::from_millis(300), writer.write_all(&hello.encode_json_line()?))
        .await
        .map_err(|_| anyhow::anyhow!("write hello to core-service timed out"))??;
    timeout(Duration::from_millis(300), writer.flush())
        .await
        .map_err(|_| anyhow::anyhow!("flush hello to core-service timed out"))??;

    let protocol_message = match command {
        UiToCoreCommand::Ping => ProtocolMessage::Ping,
        UiToCoreCommand::Shutdown => ProtocolMessage::Shutdown,
    };
    let payload = ProtocolFrame::new(protocol_message);
    timeout(Duration::from_millis(300), writer.write_all(&payload.encode_json_line()?))
        .await
        .map_err(|_| anyhow::anyhow!("write command to core-service timed out"))??;
    timeout(Duration::from_millis(300), writer.flush())
        .await
        .map_err(|_| anyhow::anyhow!("flush command to core-service timed out"))??;

    let mut lines = BufReader::new(reader).lines();
    let Some(hello_ack_line) = timeout(Duration::from_millis(300), lines.next_line())
        .await
        .map_err(|_| anyhow::anyhow!("read version acknowledgement from core-service timed out"))?? else {
        anyhow::bail!("core-service closed connection before version acknowledgement");
    };
    let hello_ack = ProtocolFrame::decode_json_line(hello_ack_line.as_bytes())?;
    let ProtocolMessage::VersionHello(negotiated) = hello_ack.message else {
        anyhow::bail!("core-service returned unexpected handshake frame");
    };

    let Some(line) = timeout(Duration::from_millis(300), lines.next_line())
        .await
        .map_err(|_| anyhow::anyhow!("read command response from core-service timed out"))?? else {
        anyhow::bail!("core-service closed connection without response");
    };

    let frame = ProtocolFrame::decode_json_line(line.as_bytes())?;
    match frame.message {
        ProtocolMessage::Pong => Ok(CoreToUiEvent::Pong),
        ProtocolMessage::Ready { pid } => Ok(CoreToUiEvent::Ready {
            protocol_version: negotiated.current,
            pid,
        }),
        ProtocolMessage::Shutdown => Ok(CoreToUiEvent::ShuttingDown),
        other => anyhow::bail!("unexpected protocol message from core-service: {other:?}"),
    }
}

pub async fn bind_listener() -> anyhow::Result<TcpListener> {
    Ok(TcpListener::bind(core_service_addr()).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_roundtrip() {
        let frame = ProtocolFrame::new(ProtocolMessage::Ping);
        let encoded = frame.encode_json_line().expect("encode ping");
        let decoded = ProtocolFrame::decode_json_line(&encoded).expect("decode ping");
        assert_eq!(decoded.message, ProtocolMessage::Ping);
    }
}
