use std::collections::BTreeMap;

pub const MAX_PIPELINE_REQUESTS: usize = 8;
pub const MAX_PIPELINE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_READ_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_READ_CHUNK_BYTES: u64 = 256 * 1024;
const SFTP_DATA_PACKET_OVERHEAD: u64 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub offset: u64,
    pub len: u32,
}

pub fn effective_read_chunk(server_packet_len: Option<u64>, server_read_len: Option<u64>) -> u64 {
    let packet_data_len = server_packet_len
        .map(|packet| packet.saturating_sub(SFTP_DATA_PACKET_OVERHEAD))
        .filter(|packet| *packet > 0);
    server_read_len
        .or(packet_data_len)
        .unwrap_or(DEFAULT_READ_CHUNK_BYTES)
        .min(packet_data_len.unwrap_or(MAX_READ_CHUNK_BYTES))
        .min(MAX_READ_CHUNK_BYTES)
        .max(1)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineStats {
    pub max_in_flight: usize,
    pub max_buffered_bytes: u64,
}

impl PipelineStats {
    pub fn observe(&mut self, in_flight: usize, buffered_bytes: u64) {
        self.max_in_flight = self.max_in_flight.max(in_flight);
        self.max_buffered_bytes = self.max_buffered_bytes.max(buffered_bytes);
    }
}

#[cfg(test)]
pub fn plan_window(total: u64, requested_chunk_bytes: u64, max_requests: usize) -> Vec<Chunk> {
    plan_window_from(0, total, requested_chunk_bytes, max_requests)
}

pub fn plan_window_from(
    start: u64,
    total: u64,
    requested_chunk_bytes: u64,
    max_requests: usize,
) -> Vec<Chunk> {
    if start >= total || requested_chunk_bytes == 0 || max_requests == 0 {
        return Vec::new();
    }
    let chunk_bytes = requested_chunk_bytes
        .min(MAX_READ_CHUNK_BYTES)
        .min(u32::MAX as u64);
    let request_limit = max_requests.min(MAX_PIPELINE_REQUESTS);
    let mut chunks = Vec::with_capacity(request_limit);
    let mut offset = start;
    let mut window_bytes = 0_u64;
    while offset < total && chunks.len() < request_limit && window_bytes < MAX_PIPELINE_BYTES {
        let len = total
            .saturating_sub(offset)
            .min(chunk_bytes)
            .min(MAX_PIPELINE_BYTES - window_bytes);
        if len == 0 {
            break;
        }
        chunks.push(Chunk {
            offset,
            len: len as u32,
        });
        offset = offset.saturating_add(len);
        window_bytes = window_bytes.saturating_add(len);
    }
    chunks
}

#[derive(Debug)]
pub struct OrderedChunkBuffer {
    next_offset: u64,
    chunks: BTreeMap<u64, Vec<u8>>,
    buffered_bytes: u64,
}

impl OrderedChunkBuffer {
    pub fn new(next_offset: u64) -> Self {
        Self {
            next_offset,
            chunks: BTreeMap::new(),
            buffered_bytes: 0,
        }
    }

    pub fn insert(&mut self, offset: u64, data: Vec<u8>) -> bool {
        if data.is_empty() || offset < self.next_offset || self.chunks.contains_key(&offset) {
            return false;
        }
        self.buffered_bytes = self.buffered_bytes.saturating_add(data.len() as u64);
        self.chunks.insert(offset, data);
        true
    }

    pub fn drain_ready(&mut self) -> Vec<(u64, Vec<u8>)> {
        let mut drained = Vec::new();
        while let Some(data) = self.chunks.remove(&self.next_offset) {
            let offset = self.next_offset;
            self.next_offset = self.next_offset.saturating_add(data.len() as u64);
            self.buffered_bytes = self.buffered_bytes.saturating_sub(data.len() as u64);
            drained.push((offset, data));
        }
        drained
    }

    #[cfg(test)]
    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }

    pub fn buffered_bytes(&self) -> u64 {
        self.buffered_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_read_chunk, plan_window, plan_window_from, Chunk, OrderedChunkBuffer,
        MAX_PIPELINE_BYTES,
    };

    #[test]
    fn plans_bounded_chunks_without_crossing_file_size() {
        let chunks = plan_window(900_000, 262_144, 8);

        assert_eq!(
            chunks[0],
            Chunk {
                offset: 0,
                len: 262_144
            }
        );
        assert_eq!(
            chunks.last().unwrap().offset + chunks.last().unwrap().len as u64,
            900_000
        );
        assert!(chunks.len() <= 8);
        assert!(chunks.iter().map(|chunk| chunk.len as u64).sum::<u64>() <= MAX_PIPELINE_BYTES);
    }

    #[test]
    fn handles_empty_files_server_limits_client_caps_and_single_request_windows() {
        assert!(plan_window(0, 262_144, 8).is_empty());
        assert!(plan_window(256_000, 64 * 1024, 8)
            .iter()
            .all(|chunk| chunk.len <= 64 * 1024));
        assert!(plan_window(8_000_000, u64::MAX, 8)
            .iter()
            .all(|chunk| chunk.len <= 2 * 1024 * 1024));
        assert_eq!(plan_window(900_000, 262_144, 1).len(), 1);
        assert_eq!(effective_read_chunk(Some(64 * 1024), None), 64 * 1024 - 9);
        assert_eq!(
            effective_read_chunk(None, Some(4 * 1024 * 1024)),
            2 * 1024 * 1024
        );
    }

    #[test]
    fn does_not_overflow_offsets_near_u64_max() {
        let chunks = plan_window_from(u64::MAX - 10, u64::MAX, 8, 8);

        assert_eq!(
            chunks,
            vec![
                Chunk {
                    offset: u64::MAX - 10,
                    len: 8
                },
                Chunk {
                    offset: u64::MAX - 2,
                    len: 2
                }
            ]
        );
    }

    #[test]
    fn drains_out_of_order_completions_in_ascending_offset_order() {
        let mut buffer = OrderedChunkBuffer::new(0);
        assert!(buffer.insert(4, vec![4, 5]));
        assert!(buffer.drain_ready().is_empty());
        assert!(buffer.insert(0, vec![0, 1, 2, 3]));

        let drained = buffer.drain_ready();

        assert_eq!(drained, vec![(0, vec![0, 1, 2, 3]), (4, vec![4, 5])]);
        assert_eq!(buffer.next_offset(), 6);
    }
}
