use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardPayload {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ImageClipboardFormat {
    Png,
    Bgra8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageClipboardPayload {
    pub format: ImageClipboardFormat,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardContent {
    Text(ClipboardPayload),
    Image(ImageClipboardPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardUpdate {
    pub source_device_id: Uuid,
    pub sequence: u64,
    pub payload: ClipboardPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardContentUpdate {
    pub source_device_id: Uuid,
    pub sequence: u64,
    pub content: ClipboardContent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClipboardDispatchKind {
    Broadcast,
    Targeted(Uuid),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardDispatch {
    pub dispatch: ClipboardDispatchKind,
    pub update: ClipboardUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardApplyAction {
    IgnoreDisabled,
    IgnoreLoop,
    ApplyRemote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardApplyDecision {
    pub action: ClipboardApplyAction,
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClipboardSyncEngine {
    local_device_id: Uuid,
    enabled: bool,
    next_sequence: u64,
    recent_remote_sequences: HashMap<Uuid, u64>,
    last_local_text: Option<String>,
    last_local_image_checksum: Option<String>,
}

impl ClipboardSyncEngine {
    pub fn new(local_device_id: Uuid) -> Self {
        Self {
            local_device_id,
            enabled: true,
            next_sequence: 1,
            recent_remote_sequences: HashMap::new(),
            last_local_text: None,
            last_local_image_checksum: None,
        }
    }

    pub fn local_device_id(&self) -> Uuid {
        self.local_device_id
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn create_local_update(&mut self, text: impl Into<String>) -> Option<ClipboardDispatch> {
        if !self.enabled {
            return None;
        }

        let text = normalize_text(text.into());
        if text.is_empty() {
            return None;
        }

        if self.last_local_text.as_deref() == Some(text.as_str()) {
            return None;
        }

        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.last_local_text = Some(text.clone());

        Some(ClipboardDispatch {
            dispatch: ClipboardDispatchKind::Broadcast,
            update: ClipboardUpdate {
                source_device_id: self.local_device_id,
                sequence,
                payload: ClipboardPayload { text },
            },
        })
    }

    pub fn apply_remote_update(&mut self, update: &ClipboardUpdate) -> ClipboardApplyDecision {
        if !self.enabled {
            return ClipboardApplyDecision {
                action: ClipboardApplyAction::IgnoreDisabled,
                text: None,
            };
        }

        if update.source_device_id == self.local_device_id {
            return ClipboardApplyDecision {
                action: ClipboardApplyAction::IgnoreLoop,
                text: None,
            };
        }

        let last_seen = self
            .recent_remote_sequences
            .entry(update.source_device_id)
            .or_insert(0);
        if update.sequence <= *last_seen {
            return ClipboardApplyDecision {
                action: ClipboardApplyAction::IgnoreLoop,
                text: None,
            };
        }

        *last_seen = update.sequence;
        let text = normalize_text(update.payload.text.clone());
        self.last_local_text = Some(text.clone());

        ClipboardApplyDecision {
            action: ClipboardApplyAction::ApplyRemote,
            text: Some(text),
        }
    }

    pub fn create_local_image_update(
        &mut self,
        format: ImageClipboardFormat,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    ) -> Result<Option<ClipboardContentUpdate>> {
        if !self.enabled {
            return Ok(None);
        }
        let image = normalize_image_payload(format, width, height, bytes)?;
        if self.last_local_image_checksum.as_deref() == Some(image.checksum_sha256.as_str()) {
            return Ok(None);
        }

        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.last_local_image_checksum = Some(image.checksum_sha256.clone());

        Ok(Some(ClipboardContentUpdate {
            source_device_id: self.local_device_id,
            sequence,
            content: ClipboardContent::Image(image),
        }))
    }

    pub fn apply_remote_content_update(
        &mut self,
        update: &ClipboardContentUpdate,
    ) -> Result<ClipboardApplyContentDecision> {
        if !self.enabled {
            return Ok(ClipboardApplyContentDecision {
                action: ClipboardApplyAction::IgnoreDisabled,
                content: None,
            });
        }
        if update.source_device_id == self.local_device_id {
            return Ok(ClipboardApplyContentDecision {
                action: ClipboardApplyAction::IgnoreLoop,
                content: None,
            });
        }

        let last_seen = self
            .recent_remote_sequences
            .entry(update.source_device_id)
            .or_insert(0);
        if update.sequence <= *last_seen {
            return Ok(ClipboardApplyContentDecision {
                action: ClipboardApplyAction::IgnoreLoop,
                content: None,
            });
        }

        *last_seen = update.sequence;
        let content = normalize_clipboard_content(update.content.clone())?;
        match &content {
            ClipboardContent::Text(payload) => self.last_local_text = Some(payload.text.clone()),
            ClipboardContent::Image(payload) => {
                self.last_local_image_checksum = Some(payload.checksum_sha256.clone());
            }
        }

        Ok(ClipboardApplyContentDecision {
            action: ClipboardApplyAction::ApplyRemote,
            content: Some(content),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardApplyContentDecision {
    pub action: ClipboardApplyAction,
    pub content: Option<ClipboardContent>,
}

pub fn normalize_text(text: String) -> String {
    text.replace("\r\n", "\n")
}

pub fn normalize_clipboard_content(content: ClipboardContent) -> Result<ClipboardContent> {
    match content {
        ClipboardContent::Text(payload) => Ok(ClipboardContent::Text(ClipboardPayload {
            text: normalize_text(payload.text),
        })),
        ClipboardContent::Image(payload) => Ok(ClipboardContent::Image(normalize_image_payload(
            payload.format,
            payload.width,
            payload.height,
            payload.bytes,
        )?)),
    }
}

pub fn normalize_image_payload(
    format: ImageClipboardFormat,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
) -> Result<ImageClipboardPayload> {
    if width == 0 || height == 0 {
        anyhow::bail!("image clipboard payload requires non-zero dimensions");
    }
    if bytes.is_empty() {
        anyhow::bail!("image clipboard payload requires bytes");
    }
    if matches!(format, ImageClipboardFormat::Bgra8) {
        let expected = width as usize * height as usize * 4;
        if bytes.len() != expected {
            anyhow::bail!("BGRA clipboard payload size mismatch: expected {expected}, actual {}", bytes.len());
        }
    }

    Ok(ImageClipboardPayload {
        format,
        width,
        height,
        checksum_sha256: sha256_hex(&bytes),
        bytes,
    })
}

fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(input);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn clipboard_pipeline_latency<F, T>(mut operation: F) -> Result<(T, Duration)>
where
    F: FnMut() -> Result<T>,
{
    let started = Instant::now();
    let result = operation()?;
    Ok((result, started.elapsed()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_update_emits_normalized_payload_once() {
        let local = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);

        let update = engine
            .create_local_update("hello\r\nworld")
            .expect("clipboard update");

        assert_eq!(update.update.payload.text, "hello\nworld");
        assert!(engine.create_local_update("hello\nworld").is_none());
    }

    #[test]
    fn disabled_sync_suppresses_local_and_remote_updates() {
        let local = Uuid::new_v4();
        let remote = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);
        engine.set_enabled(false);

        assert!(engine.create_local_update("blocked").is_none());
        let decision = engine.apply_remote_update(&ClipboardUpdate {
            source_device_id: remote,
            sequence: 1,
            payload: ClipboardPayload {
                text: "remote".into(),
            },
        });
        assert_eq!(decision.action, ClipboardApplyAction::IgnoreDisabled);
    }

    #[test]
    fn remote_updates_apply_once_and_suppress_replays() {
        let local = Uuid::new_v4();
        let remote = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);
        let update = ClipboardUpdate {
            source_device_id: remote,
            sequence: 7,
            payload: ClipboardPayload {
                text: "fresh".into(),
            },
        };

        let first = engine.apply_remote_update(&update);
        let second = engine.apply_remote_update(&update);

        assert_eq!(first.action, ClipboardApplyAction::ApplyRemote);
        assert_eq!(first.text.as_deref(), Some("fresh"));
        assert_eq!(second.action, ClipboardApplyAction::IgnoreLoop);
    }

    #[test]
    fn local_origin_remote_update_is_treated_as_loop() {
        let local = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);

        let decision = engine.apply_remote_update(&ClipboardUpdate {
            source_device_id: local,
            sequence: 2,
            payload: ClipboardPayload {
                text: "echo".into(),
            },
        });

        assert_eq!(decision.action, ClipboardApplyAction::IgnoreLoop);
    }

    #[test]
    fn clipboard_pipeline_completes_under_reasonable_bound() {
        let local = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);

        let (update, elapsed) = clipboard_pipeline_latency(|| {
            engine
                .create_local_update("perf")
                .ok_or_else(|| anyhow::anyhow!("missing update"))
        })
        .expect("pipeline latency");

        println!("clipboard sync elapsed: {elapsed:?}");
        assert_eq!(update.update.payload.text, "perf");
        assert!(elapsed < Duration::from_millis(20), "clipboard sync took {elapsed:?}");
    }

    #[test]
    fn image_clipboard_payload_normalizes_and_checksums_bgra() {
        let image = normalize_image_payload(
            ImageClipboardFormat::Bgra8,
            2,
            2,
            vec![0, 0, 0, 255, 10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255],
        )
        .expect("normalize image payload");

        assert_eq!(image.bytes.len(), 16);
        assert_eq!(image.checksum_sha256.len(), 64);
    }

    #[test]
    fn image_clipboard_rejects_invalid_bgra_size() {
        let error = normalize_image_payload(ImageClipboardFormat::Bgra8, 2, 2, vec![1, 2, 3])
            .expect_err("invalid image should fail");
        assert!(error.to_string().contains("size mismatch"));
    }

    #[test]
    fn local_image_update_suppresses_duplicate_checksum() {
        let local = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);
        let bytes = vec![8_u8; 16];

        let first = engine
            .create_local_image_update(ImageClipboardFormat::Bgra8, 2, 2, bytes.clone())
            .expect("create image update");
        let second = engine
            .create_local_image_update(ImageClipboardFormat::Bgra8, 2, 2, bytes)
            .expect("create duplicate image update");

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn remote_image_content_applies_once_and_suppresses_replay() {
        let local = Uuid::new_v4();
        let remote = Uuid::new_v4();
        let mut engine = ClipboardSyncEngine::new(local);
        let update = ClipboardContentUpdate {
            source_device_id: remote,
            sequence: 11,
            content: ClipboardContent::Image(
                normalize_image_payload(ImageClipboardFormat::Bgra8, 1, 1, vec![1, 2, 3, 4])
                    .expect("normalize image"),
            ),
        };

        let first = engine
            .apply_remote_content_update(&update)
            .expect("apply image update");
        let second = engine
            .apply_remote_content_update(&update)
            .expect("replay image update");

        assert_eq!(first.action, ClipboardApplyAction::ApplyRemote);
        assert!(matches!(first.content, Some(ClipboardContent::Image(_))));
        assert_eq!(second.action, ClipboardApplyAction::IgnoreLoop);
    }
}
