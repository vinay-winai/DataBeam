//! Inspect existing receiver data without connecting to the sender or exporting files.
use iroh_blobs::{
    api::blobs::BlobStatus, format::collection::Collection, store::fs::FsStore, ticket::BlobTicket,
    HashAndFormat,
};
use serde::Serialize;
use std::{path::Path, str::FromStr};

#[derive(Debug, Default, Serialize)]
pub struct CachedReceive {
    pub complete: bool,
    pub bytes: u64,
    pub total: Option<u64>,
    pub names: Vec<String>,
}

pub async fn inspect(ticket: &str, base: &Path) -> anyhow::Result<Option<CachedReceive>> {
    let ticket = BlobTicket::from_str(ticket)?;
    let directory = base.join(format!(".sendme-recv-{}", ticket.hash().to_hex()));
    if !directory.is_dir() {
        return Ok(None);
    }
    let db = FsStore::load(directory).await?;
    let result = async {
        let local = db.remote().local(ticket.hash_and_format()).await?;
        let mut result = CachedReceive {
            complete: local.is_complete(),
            ..Default::default()
        };
        if let Ok(collection) = Collection::load(ticket.hash(), db.as_ref()).await {
            let mut total = Some(0u64);
            for (name, hash) in collection.iter() {
                let entry = db.remote().local(HashAndFormat::raw(*hash)).await?;
                result.bytes += entry.local_bytes();
                let size = match db.status(*hash).await? {
                    BlobStatus::Complete { size } => Some(size),
                    BlobStatus::Partial { size } => size,
                    BlobStatus::NotFound => None,
                };
                total = total.zip(size).map(|(sum, size)| sum.saturating_add(size));
                if result.names.len() < 5 {
                    result.names.push(name.clone());
                }
            }
            result.total = total;
        } else {
            // Collection metadata can itself be partial. Report only verified local ranges.
            result.bytes = local.local_bytes();
        }
        Ok::<_, anyhow::Error>(Some(result))
    }
    .await;
    db.shutdown().await?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_blobs::{BlobFormat, Hash};

    #[tokio::test]
    async fn inspection_distinguishes_partial_complete_and_absent_without_export(
    ) -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let store = FsStore::load(directory.path().join("seed")).await?;
        let present = store.add_bytes(b"already verified".to_vec()).await?;
        let missing = b"still to download";
        let collection = vec![
            ("folder/first.txt".to_string(), present.hash),
            ("folder/second.txt".to_string(), Hash::new(missing)),
        ]
        .into_iter()
        .collect::<Collection>();
        let root = collection.store(&store).await?;
        let ticket = BlobTicket::new(
            iroh::EndpointAddr::new(iroh::SecretKey::from_bytes(&[42; 32]).public()),
            root.hash(),
            BlobFormat::HashSeq,
        )
        .to_string();
        let base = directory.path().join("cache");
        assert!(inspect(&ticket, &base).await?.is_none());
        assert!(!base.exists());
        let target = base.join(format!(".sendme-recv-{}", root.hash().to_hex()));
        store.shutdown().await?;
        std::fs::create_dir_all(&base)?;
        std::fs::rename(directory.path().join("seed"), &target)?;
        let partial = inspect(&ticket, &base).await?.unwrap();
        assert!(!partial.complete);
        assert_eq!(partial.bytes, b"already verified".len() as u64);
        assert_eq!(partial.total, None);
        assert_eq!(partial.names.len(), 2);
        let store = FsStore::load(&target).await?;
        let _added = store.add_bytes(missing.to_vec()).await?;
        store.shutdown().await?;
        let complete = inspect(&ticket, &base).await?.unwrap();
        assert!(complete.complete);
        assert_eq!(
            complete.bytes,
            (b"already verified".len() + missing.len()) as u64
        );
        assert_eq!(complete.total, Some(complete.bytes));
        assert_eq!(
            std::fs::read_dir(directory.path())?.count(),
            1,
            "Inspection must not export files"
        );
        Ok(())
    }
}
