use std::collections::HashMap;

const FIXED_HEADER: usize = 11;
const START_FLAG: u8 = 0x80;
const DATA_ID_MASK: u8 = 0x3f;
const LENGTH_HIGH_MASK: u8 = 0x0f;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataMessage {
    pub data_id: u8,
    pub source_id: u32,
    pub payload: Vec<u8>,
}

#[derive(Default)]
struct Pending {
    source_id: u32,
    remaining: u8,
    next_sequence: u16,
    payload: Vec<u8>,
    started: bool,
}

/// Joins the fragments of the Teams framing on the `main-channel` SCTP data channel.
#[derive(Default)]
pub struct Depacketizer {
    pending: HashMap<u8, Pending>,
}

impl Depacketizer {
    pub fn push(&mut self, frame: &[u8]) -> Option<DataMessage> {
        if frame.len() < FIXED_HEADER {
            return None;
        }
        let header_length = (usize::from(frame[0] & LENGTH_HIGH_MASK) << 8) | usize::from(frame[1]);
        if header_length < FIXED_HEADER || header_length > frame.len() {
            return None;
        }
        let data_id = frame[2] & DATA_ID_MASK;
        let sequence = u16::from_be_bytes([frame[3], frame[4]]);
        let remaining = frame[5];
        let source_id = u32::from_be_bytes([frame[6], frame[7], frame[8], frame[9]]);
        let pending = self.pending.entry(data_id).or_default();
        if frame[2] & START_FLAG != 0 {
            *pending = Pending { source_id, remaining, next_sequence: sequence, payload: Vec::new(), started: true };
        } else if !pending.started || pending.next_sequence != sequence {
            *pending = Pending::default();
            return None;
        } else {
            pending.remaining = pending.remaining.saturating_sub(1);
        }
        pending.next_sequence = sequence.wrapping_add(1);
        pending.payload.extend_from_slice(&frame[header_length..]);
        if pending.remaining != 0 {
            return None;
        }
        let finished = std::mem::take(pending);
        Some(DataMessage { data_id, source_id: finished.source_id, payload: finished.payload })
    }
}

#[cfg(test)]
pub fn frame(data_id: u8, sequence: u16, remaining: u8, start: bool, source_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x10, FIXED_HEADER as u8, data_id | if start { START_FLAG } else { 0 }];
    bytes.extend_from_slice(&sequence.to_be_bytes());
    bytes.push(remaining);
    bytes.extend_from_slice(&source_id.to_be_bytes());
    bytes.push(0);
    bytes.extend_from_slice(payload);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_frame_is_one_message() {
        let mut depacketizer = Depacketizer::default();
        let message = depacketizer.push(&frame(3, 0, 0, true, 77, b"{\"a\":1}")).unwrap();
        assert_eq!(message, DataMessage { data_id: 3, source_id: 77, payload: b"{\"a\":1}".to_vec() });
    }

    #[test]
    fn fragments_join_in_sequence_and_other_ids_do_not_interfere() {
        let mut depacketizer = Depacketizer::default();
        assert!(depacketizer.push(&frame(3, 10, 2, true, 5, b"he")).is_none());
        assert!(depacketizer.push(&frame(9, 0, 0, true, 6, b"x")).is_some());
        assert!(depacketizer.push(&frame(3, 11, 2, false, 5, b"ll")).is_none());
        let message = depacketizer.push(&frame(3, 12, 2, false, 5, b"o")).unwrap();
        assert_eq!(message.payload, b"hello");
        assert_eq!(message.source_id, 5);
    }

    #[test]
    fn a_gap_in_the_sequence_drops_the_message() {
        let mut depacketizer = Depacketizer::default();
        assert!(depacketizer.push(&frame(3, 1, 1, true, 5, b"a")).is_none());
        assert!(depacketizer.push(&frame(3, 5, 1, false, 5, b"b")).is_none());
        assert!(depacketizer.push(&frame(3, 6, 1, false, 5, b"c")).is_none());
    }

    #[test]
    fn a_continuation_without_a_start_and_short_frames_are_ignored() {
        let mut depacketizer = Depacketizer::default();
        assert!(depacketizer.push(&frame(3, 0, 0, false, 5, b"a")).is_none());
        assert!(depacketizer.push(&[0x10, 11, 0x83]).is_none());
        assert!(depacketizer.push(&[]).is_none());
    }

    #[test]
    fn recipients_in_the_header_are_skipped() {
        let mut bytes = frame(3, 0, 0, true, 5, b"");
        bytes[1] = (FIXED_HEADER + 4) as u8;
        bytes[10] = 1;
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(b"body");
        let message = Depacketizer::default().push(&bytes).unwrap();
        assert_eq!(message.payload, b"body");
    }
}
