//! Sender-side logical completion inferred from a receiver's missing chunks.
//! This is display accounting, never evidence that a request completed.
use std::{collections::HashMap, time::Instant};

use iroh_blobs::protocol::{ChunkRanges, ChunkRangesSeq};

#[derive(Default)]
struct Peer {
    done: Vec<u64>,
    sent: u64,
    rate: Option<super::live_rate::LiveRate>,
}

struct Request {
    ranges: ChunkRangesSeq,
    sent: HashMap<u64, u64>,
}

pub(super) struct SenderProgress {
    sizes: Vec<u64>,
    peers: HashMap<u64, Peer>,
    requests: HashMap<(u64, u64), Request>,
    latest: Option<u64>,
}

impl SenderProgress {
    pub(super) fn new(sizes: Vec<u64>) -> Self {
        Self {
            sizes,
            peers: HashMap::new(),
            requests: HashMap::new(),
            latest: None,
        }
    }

    pub(super) fn begin(&mut self, connection: u64, request: u64, ranges: &ChunkRangesSeq) {
        // Root/collection-only requests do not describe the payload cache.
        if !self
            .sizes
            .iter()
            .enumerate()
            .any(|(i, _)| !ranges[i as u64 + 2].is_empty())
        {
            return;
        }
        let peer = self.peers.entry(connection).or_insert_with(|| Peer {
            done: vec![0; self.sizes.len()],
            rate: Some(super::live_rate::LiveRate::new(0, Instant::now())),
            ..Default::default()
        });
        for (i, size) in self.sizes.iter().copied().enumerate() {
            let requested = requested_bytes(&ranges[i as u64 + 2], size, size);
            // Empty ranges include fully cached files. Count them once even
            // when several requests/retries describe the same file.
            peer.done[i] = peer.done[i].max(size.saturating_sub(requested));
        }
        self.latest = Some(connection);
        self.requests.insert(
            (connection, request),
            Request {
                ranges: ranges.clone(),
                sent: HashMap::new(),
            },
        );
    }

    pub(super) fn advance(&mut self, connection: u64, request: u64, index: u64, end: u64) {
        let Some(file) = index.checked_sub(2).and_then(|i| usize::try_from(i).ok()) else {
            return;
        };
        let Some(&size) = self.sizes.get(file) else {
            return;
        };
        let Some(request) = self.requests.get_mut(&(connection, request)) else {
            return;
        };
        let Some(peer) = self.peers.get_mut(&connection) else {
            return;
        };
        let ranges = &request.ranges[index];
        let wanted = requested_bytes(ranges, size, size);
        let sent = requested_bytes(ranges, size, end);
        let previous = request.sent.entry(index).or_default();
        peer.sent = peer.sent.saturating_add(sent.saturating_sub(*previous));
        if let Some(rate) = &mut peer.rate { rate.observe(peer.sent, Instant::now()); }
        *previous = (*previous).max(sent);
        peer.done[file] =
            peer.done[file].max(size.saturating_sub(wanted).saturating_add(sent).min(size));
    }

    pub(super) fn complete_blob(&mut self, connection: u64, request: u64, index: u64) {
        self.advance(connection, request, index, u64::MAX);
    }

    pub(super) fn finish_request(&mut self, connection: u64, request: u64) {
        self.requests.remove(&(connection, request));
    }

    pub(super) fn close(&mut self, connection: u64) {
        self.peers.remove(&connection);
        self.requests.retain(|(peer, _), _| *peer != connection);
        if self.latest == Some(connection) {
            self.latest = None;
        }
    }

    pub(super) fn snapshot(&self) -> Option<(u64, f64)> {
        let peer = self.peers.get(&self.latest?)?;
        let done = peer.done.iter().copied().fold(0_u64, u64::saturating_add);
        // Cached bytes advance completion but never inflate upload speed.
        Some((done, peer.rate.as_ref()?.current(Instant::now())))
    }
}

fn requested_bytes(ranges: &ChunkRanges, size: u64, end: u64) -> u64 {
    let limit = size.min(end);
    ranges
        .boundaries()
        .chunks(2)
        .map(|pair| {
            let start = pair[0].0.saturating_mul(1024).min(limit);
            let end = pair
                .get(1)
                .map(|chunk| chunk.0.saturating_mul(1024))
                .unwrap_or(size)
                .min(limit);
            end.saturating_sub(start)
        })
        .fold(0, u64::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_blobs::protocol::ChunkRangesExt;

    #[test]
    fn resumed_files_and_holes_are_counted_once_without_inflating_rate() {
        let mut progress = SenderProgress::new(vec![4096, 4096]);
        let ranges = ChunkRangesSeq::from_ranges([
            ChunkRanges::empty(),
            ChunkRanges::empty(),
            ChunkRanges::empty(),
            ChunkRanges::bytes(1024..2048) | ChunkRanges::bytes(3072..),
        ]);
        progress.begin(1, 1, &ranges);
        assert_eq!(progress.snapshot().unwrap(), (6144, 0.0));
        progress.advance(1, 1, 3, 2048);
        assert_eq!(progress.snapshot().unwrap().0, 7168);
        // A repeated offset and a gap do not add bytes.
        progress.advance(1, 1, 3, 2048);
        progress.advance(1, 1, 3, 3072);
        assert_eq!(progress.snapshot().unwrap().0, 7168);
        progress.complete_blob(1, 1, 3);
        assert_eq!(progress.snapshot().unwrap().0, 8192);
        progress.finish_request(1, 1);
        progress.begin(1, 2, &ranges);
        assert_eq!(progress.snapshot().unwrap().0, 8192);
        progress.complete_blob(1, 2, 3);
        assert_eq!(progress.snapshot().unwrap().0, 8192);
        // A different receiver has a different cache; old completion cannot leak.
        progress.begin(2, 1, &ChunkRangesSeq::all());
        assert_eq!(progress.snapshot().unwrap(), (0, 0.0));
        progress.close(1);
        progress.close(2);
        assert!(progress.requests.is_empty() && progress.peers.is_empty());
    }

    #[test]
    fn metadata_empty_files_and_partial_last_chunks_are_bounded() {
        let mut progress = SenderProgress::new(vec![0, 1500]);
        progress.begin(1, 1, &ChunkRangesSeq::root());
        assert!(progress.snapshot().is_none());
        let ranges = ChunkRangesSeq::from_ranges([
            ChunkRanges::empty(),
            ChunkRanges::empty(),
            ChunkRanges::all(),
            ChunkRanges::bytes(1024..),
        ]);
        progress.begin(1, 2, &ranges);
        assert_eq!(progress.snapshot().unwrap(), (1024, 0.0));
        progress.complete_blob(1, 2, 3);
        assert_eq!(progress.snapshot().unwrap().0, 1500);
    }
}
