use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const DEFAULT_CHUNK_SIZE_BYTES: u64 = 256 * 1024;
pub const MAX_CHUNK_SIZE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferFileDescriptor {
    pub file_id: Uuid,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferManifest {
    pub transfer_id: Uuid,
    pub source_device_id: Uuid,
    pub target_device_id: Uuid,
    pub files: Vec<TransferFileDescriptor>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferStatus {
    PendingApproval,
    Approved,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferPlan {
    pub manifest: TransferManifest,
    pub chunk_size_bytes: u64,
    pub total_chunks: u64,
    pub status: TransferStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferProgress {
    pub transfer_id: Uuid,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub chunk_index: u64,
    pub total_chunks: u64,
    pub status: TransferStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferChunk {
    pub transfer_id: Uuid,
    pub file_id: Uuid,
    pub chunk_index: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceivedFile {
    pub descriptor: TransferFileDescriptor,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletedTransfer {
    pub manifest: TransferManifest,
    pub files: Vec<ReceivedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileAssembly {
    descriptor: TransferFileDescriptor,
    bytes: Vec<u8>,
    received_ranges: HashSet<(u64, u64)>,
    received_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReceiver {
    plan: TransferPlan,
    files: HashMap<Uuid, FileAssembly>,
    accepted_chunks: HashSet<(Uuid, u64, u64)>,
    status: TransferStatus,
    transferred_bytes: u64,
}

pub fn plan_transfer(
    source_device_id: Uuid,
    target_device_id: Uuid,
    files: Vec<TransferFileDescriptor>,
    chunk_size_bytes: Option<u64>,
) -> Result<TransferPlan> {
    if files.is_empty() {
        anyhow::bail!("transfer requires at least one file");
    }

    let chunk_size_bytes = chunk_size_bytes
        .unwrap_or(DEFAULT_CHUNK_SIZE_BYTES)
        .clamp(64 * 1024, MAX_CHUNK_SIZE_BYTES);
    let total_bytes = files.iter().map(|file| file.size_bytes).sum::<u64>();
    let total_chunks = total_bytes.div_ceil(chunk_size_bytes);

    Ok(TransferPlan {
        manifest: TransferManifest {
            transfer_id: Uuid::new_v4(),
            source_device_id,
            target_device_id,
            files,
            total_bytes,
        },
        chunk_size_bytes,
        total_chunks,
        status: TransferStatus::PendingApproval,
    })
}

pub fn approve_transfer(mut plan: TransferPlan) -> TransferPlan {
    plan.status = TransferStatus::Approved;
    plan
}

pub fn progress_for_chunk(plan: &TransferPlan, chunk_index: u64) -> Result<TransferProgress> {
    if chunk_index >= plan.total_chunks.max(1) {
        anyhow::bail!("chunk index {chunk_index} is out of bounds");
    }

    let transferred_bytes =
        ((chunk_index + 1) * plan.chunk_size_bytes).min(plan.manifest.total_bytes);
    let status = if transferred_bytes >= plan.manifest.total_bytes {
        TransferStatus::Completed
    } else {
        TransferStatus::InProgress
    };

    Ok(TransferProgress {
        transfer_id: plan.manifest.transfer_id,
        transferred_bytes,
        total_bytes: plan.manifest.total_bytes,
        chunk_index,
        total_chunks: plan.total_chunks,
        status,
    })
}

pub fn chunk_bytes(
    plan: &TransferPlan,
    file: &TransferFileDescriptor,
    file_bytes: &[u8],
) -> Result<Vec<TransferChunk>> {
    if plan.status == TransferStatus::Cancelled {
        anyhow::bail!("cannot chunk a cancelled transfer");
    }
    if !plan
        .manifest
        .files
        .iter()
        .any(|candidate| candidate.file_id == file.file_id)
    {
        anyhow::bail!("file {} is not part of transfer {}", file.file_id, plan.manifest.transfer_id);
    }
    if file.size_bytes != file_bytes.len() as u64 {
        anyhow::bail!(
            "file {} size mismatch: descriptor={} actual={}",
            file.file_id,
            file.size_bytes,
            file_bytes.len()
        );
    }

    let chunk_size = usize::try_from(plan.chunk_size_bytes)
        .map_err(|_| anyhow::anyhow!("chunk size exceeds platform capacity"))?;
    if chunk_size == 0 {
        anyhow::bail!("chunk size must be positive");
    }

    Ok(file_bytes
        .chunks(chunk_size)
        .enumerate()
        .map(|(index, bytes)| TransferChunk {
            transfer_id: plan.manifest.transfer_id,
            file_id: file.file_id,
            chunk_index: index as u64,
            offset: (index * chunk_size) as u64,
            bytes: bytes.to_vec(),
            checksum_sha256: checksum_sha256(bytes),
        })
        .collect())
}

pub fn chunk_manifest_files(
    plan: &TransferPlan,
    files: &HashMap<Uuid, Vec<u8>>,
) -> Result<Vec<TransferChunk>> {
    let mut chunks = Vec::new();
    for descriptor in &plan.manifest.files {
        let file_bytes = files
            .get(&descriptor.file_id)
            .ok_or_else(|| anyhow::anyhow!("missing bytes for file {}", descriptor.file_id))?;
        chunks.extend(chunk_bytes(plan, descriptor, file_bytes)?);
    }
    Ok(chunks)
}

pub fn validate_chunk_checksum(chunk: &TransferChunk) -> Result<()> {
    let actual = checksum_sha256(&chunk.bytes);
    if actual != chunk.checksum_sha256 {
        anyhow::bail!(
            "checksum mismatch for transfer {} file {} chunk {}",
            chunk.transfer_id,
            chunk.file_id,
            chunk.chunk_index
        );
    }
    Ok(())
}

pub fn checksum_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn cancel_transfer(mut plan: TransferPlan) -> TransferPlan {
    plan.status = TransferStatus::Cancelled;
    plan
}

impl TransferReceiver {
    pub fn new(plan: TransferPlan) -> Result<Self> {
        if plan.status != TransferStatus::Approved && plan.status != TransferStatus::InProgress {
            anyhow::bail!("receiver requires an approved transfer plan");
        }

        let mut files = HashMap::new();
        for descriptor in &plan.manifest.files {
            let size = usize::try_from(descriptor.size_bytes)
                .map_err(|_| anyhow::anyhow!("file {} exceeds platform capacity", descriptor.file_id))?;
            files.insert(
                descriptor.file_id,
                FileAssembly {
                    descriptor: descriptor.clone(),
                    bytes: vec![0; size],
                    received_ranges: HashSet::new(),
                    received_bytes: 0,
                },
            );
        }

        Ok(Self {
            plan,
            files,
            accepted_chunks: HashSet::new(),
            status: TransferStatus::Approved,
            transferred_bytes: 0,
        })
    }

    pub fn status(&self) -> TransferStatus {
        self.status
    }

    pub fn cancel(&mut self) {
        self.status = TransferStatus::Cancelled;
    }

    pub fn accept_chunk(&mut self, chunk: TransferChunk) -> Result<TransferProgress> {
        if self.status == TransferStatus::Cancelled {
            anyhow::bail!("transfer {} is cancelled", self.plan.manifest.transfer_id);
        }
        if chunk.transfer_id != self.plan.manifest.transfer_id {
            anyhow::bail!("chunk belongs to a different transfer");
        }
        validate_chunk_checksum(&chunk)?;

        let assembly = self
            .files
            .get_mut(&chunk.file_id)
            .ok_or_else(|| anyhow::anyhow!("unknown file id {}", chunk.file_id))?;
        let start = usize::try_from(chunk.offset)
            .map_err(|_| anyhow::anyhow!("chunk offset exceeds platform capacity"))?;
        let end = start
            .checked_add(chunk.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("chunk range overflows"))?;
        if end > assembly.bytes.len() {
            anyhow::bail!("chunk range exceeds file size");
        }

        let range = (chunk.offset, chunk.bytes.len() as u64);
        let chunk_key = (chunk.file_id, chunk.offset, chunk.chunk_index);
        if !self.accepted_chunks.contains(&chunk_key) {
            if assembly.received_ranges.insert(range) {
                assembly.bytes[start..end].copy_from_slice(&chunk.bytes);
                assembly.received_bytes += chunk.bytes.len() as u64;
                self.transferred_bytes += chunk.bytes.len() as u64;
            }
            self.accepted_chunks.insert(chunk_key);
        }

        self.status = if self.transferred_bytes >= self.plan.manifest.total_bytes {
            TransferStatus::Completed
        } else {
            TransferStatus::InProgress
        };

        Ok(self.progress(chunk.chunk_index))
    }

    pub fn progress(&self, chunk_index: u64) -> TransferProgress {
        TransferProgress {
            transfer_id: self.plan.manifest.transfer_id,
            transferred_bytes: self.transferred_bytes,
            total_bytes: self.plan.manifest.total_bytes,
            chunk_index,
            total_chunks: self.plan.total_chunks,
            status: self.status,
        }
    }

    pub fn complete(self) -> Result<CompletedTransfer> {
        if self.status != TransferStatus::Completed {
            anyhow::bail!("transfer is not complete");
        }

        let mut files = Vec::new();
        for descriptor in &self.plan.manifest.files {
            let assembly = self
                .files
                .get(&descriptor.file_id)
                .ok_or_else(|| anyhow::anyhow!("missing assembled file {}", descriptor.file_id))?;
            if assembly.received_bytes != descriptor.size_bytes {
                anyhow::bail!("file {} is incomplete", descriptor.file_id);
            }
            files.push(ReceivedFile {
                descriptor: descriptor.clone(),
                bytes: assembly.bytes.clone(),
            });
        }

        Ok(CompletedTransfer {
            manifest: self.plan.manifest,
            files,
        })
    }
}

pub fn transfer_pipeline_latency<F, T>(mut operation: F) -> Result<(T, Duration)>
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

    fn sample_files() -> Vec<TransferFileDescriptor> {
        vec![
            TransferFileDescriptor {
                file_id: Uuid::new_v4(),
                name: "report.pdf".into(),
                size_bytes: 512 * 1024,
            },
            TransferFileDescriptor {
                file_id: Uuid::new_v4(),
                name: "capture.png".into(),
                size_bytes: 100 * 1024,
            },
        ]
    }

    #[test]
    fn transfer_plan_aggregates_manifest_and_chunks() {
        let plan = plan_transfer(Uuid::new_v4(), Uuid::new_v4(), sample_files(), None)
            .expect("plan transfer");

        assert_eq!(plan.manifest.files.len(), 2);
        assert_eq!(plan.manifest.total_bytes, 612 * 1024);
        assert_eq!(plan.total_chunks, 3);
        assert_eq!(plan.status, TransferStatus::PendingApproval);
    }

    #[test]
    fn approved_plan_tracks_progress_until_completion() {
        let approved = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), sample_files(), Some(256 * 1024))
                .expect("plan transfer"),
        );
        let progress = progress_for_chunk(&approved, 2).expect("progress");

        assert_eq!(progress.transferred_bytes, approved.manifest.total_bytes);
        assert_eq!(progress.status, TransferStatus::Completed);
    }

    #[test]
    fn empty_transfer_is_rejected() {
        let error = plan_transfer(Uuid::new_v4(), Uuid::new_v4(), Vec::new(), None)
            .expect_err("empty transfer should fail");
        assert!(error.to_string().contains("at least one file"));
    }

    #[test]
    fn transfer_pipeline_stays_within_reasonable_bound() {
        let (plan, elapsed) = transfer_pipeline_latency(|| {
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), sample_files(), None)
        })
        .expect("transfer latency");

        println!("file transfer planning elapsed: {elapsed:?}");
        assert_eq!(plan.status, TransferStatus::PendingApproval);
        assert!(elapsed < Duration::from_millis(20), "transfer planning took {elapsed:?}");
    }

    #[test]
    fn single_file_chunks_roundtrip_through_receiver() {
        let source = Uuid::new_v4();
        let target = Uuid::new_v4();
        let file = TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: "clip.mov".into(),
            size_bytes: 300 * 1024,
        };
        let data = vec![7_u8; file.size_bytes as usize];
        let plan = approve_transfer(
            plan_transfer(source, target, vec![file.clone()], Some(128 * 1024)).expect("plan transfer"),
        );
        let chunks = chunk_bytes(&plan, &file, &data).expect("chunk file");
        let mut receiver = TransferReceiver::new(plan).expect("create receiver");

        for chunk in chunks {
            receiver.accept_chunk(chunk).expect("accept chunk");
        }

        let completed = receiver.complete().expect("complete transfer");
        assert_eq!(completed.files[0].descriptor, file);
        assert_eq!(completed.files[0].bytes, data);
    }

    #[test]
    fn multi_file_chunks_can_arrive_out_of_order() {
        let files = sample_files();
        let plan = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), files.clone(), Some(128 * 1024))
                .expect("plan transfer"),
        );
        let mut file_bytes = HashMap::new();
        file_bytes.insert(files[0].file_id, vec![1_u8; files[0].size_bytes as usize]);
        file_bytes.insert(files[1].file_id, vec![2_u8; files[1].size_bytes as usize]);
        let mut chunks = chunk_manifest_files(&plan, &file_bytes).expect("chunk manifest");
        chunks.reverse();
        let mut receiver = TransferReceiver::new(plan).expect("create receiver");

        for chunk in chunks {
            receiver.accept_chunk(chunk).expect("accept chunk");
        }

        let completed = receiver.complete().expect("complete transfer");
        assert_eq!(completed.files.len(), 2);
        assert_eq!(completed.files[0].bytes, file_bytes[&files[0].file_id]);
        assert_eq!(completed.files[1].bytes, file_bytes[&files[1].file_id]);
    }

    #[test]
    fn checksum_mismatch_is_rejected() {
        let file = TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: "tampered.bin".into(),
            size_bytes: 1024,
        };
        let plan = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), vec![file.clone()], None)
                .expect("plan transfer"),
        );
        let data = vec![9_u8; file.size_bytes as usize];
        let mut chunks = chunk_bytes(&plan, &file, &data).expect("chunk file");
        chunks[0].bytes[0] = 10;
        let mut receiver = TransferReceiver::new(plan).expect("create receiver");

        let error = receiver
            .accept_chunk(chunks.remove(0))
            .expect_err("tampered chunk should fail");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn duplicate_chunk_is_idempotent() {
        let file = TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: "repeat.txt".into(),
            size_bytes: 32 * 1024,
        };
        let plan = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), vec![file.clone()], None)
                .expect("plan transfer"),
        );
        let data = vec![3_u8; file.size_bytes as usize];
        let chunk = chunk_bytes(&plan, &file, &data)
            .expect("chunk file")
            .remove(0);
        let mut receiver = TransferReceiver::new(plan).expect("create receiver");

        let first = receiver.accept_chunk(chunk.clone()).expect("first chunk");
        let second = receiver.accept_chunk(chunk).expect("duplicate chunk");

        assert_eq!(first.transferred_bytes, file.size_bytes);
        assert_eq!(second.transferred_bytes, file.size_bytes);
        assert_eq!(receiver.status(), TransferStatus::Completed);
    }

    #[test]
    fn cancellation_blocks_future_chunks() {
        let file = TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: "cancelled.dat".into(),
            size_bytes: 64 * 1024,
        };
        let plan = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), vec![file.clone()], None)
                .expect("plan transfer"),
        );
        let chunk = chunk_bytes(&plan, &file, &vec![4_u8; file.size_bytes as usize])
            .expect("chunk file")
            .remove(0);
        let mut receiver = TransferReceiver::new(plan).expect("create receiver");

        receiver.cancel();
        let error = receiver
            .accept_chunk(chunk)
            .expect_err("cancelled transfer should reject chunks");
        assert!(error.to_string().contains("cancelled"));
    }

    #[test]
    fn several_mb_roundtrip_stays_within_reasonable_bound() {
        let file = TransferFileDescriptor {
            file_id: Uuid::new_v4(),
            name: "archive.zip".into(),
            size_bytes: 5 * 1024 * 1024,
        };
        let data = (0..file.size_bytes)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let plan = approve_transfer(
            plan_transfer(Uuid::new_v4(), Uuid::new_v4(), vec![file.clone()], Some(256 * 1024))
                .expect("plan transfer"),
        );

        let (completed, elapsed) = transfer_pipeline_latency(|| {
            let chunks = chunk_bytes(&plan, &file, &data)?;
            let mut receiver = TransferReceiver::new(plan.clone())?;
            for chunk in chunks {
                receiver.accept_chunk(chunk)?;
            }
            receiver.complete()
        })
        .expect("roundtrip transfer");

        println!("file transfer 5MB memory roundtrip elapsed: {elapsed:?}");
        assert_eq!(completed.files[0].bytes, data);
        assert!(
            elapsed < Duration::from_millis(750),
            "5MB transfer pipeline took {elapsed:?}"
        );
    }
}
