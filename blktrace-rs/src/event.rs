//! Parsed trace events and the record reader.

use std::io::Read;

use crate::error::BlktraceError;
use crate::uapi::{
    blk_io_trace, blktrace_cat_BLK_TC_DISCARD, blktrace_cat_BLK_TC_READ, blktrace_cat_BLK_TC_WRITE,
    BLK_IO_TRACE_MAGIC, BLK_IO_TRACE_VERSION, BLK_TC_SHIFT,
};

/// One parsed `blk_io_trace` record together with its trailing pdu data.
pub struct BlkTraceEvent {
    /// The raw kernel record; field names match the UAPI header.
    pub event: blk_io_trace,
    /// The variable-length pdu data following the record.
    pub data: Vec<u8>,
}

impl std::fmt::Debug for BlkTraceEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlkTraceEvent")
            .field("event", &self.event)
            .field("data_len", &self.data.len())
            .finish()
    }
}

impl BlkTraceEvent {
    /// Whether the event is a read.
    pub fn is_read(&self) -> bool {
        ((self.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_READ) == blktrace_cat_BLK_TC_READ
    }

    /// Whether the event is a write.
    pub fn is_write(&self) -> bool {
        ((self.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_WRITE)
            == blktrace_cat_BLK_TC_WRITE
    }

    /// Whether the event is a discard.
    pub fn is_discard(&self) -> bool {
        ((self.event.action >> BLK_TC_SHIFT) & blktrace_cat_BLK_TC_DISCARD)
            == blktrace_cat_BLK_TC_DISCARD
    }
}

/// Reads one record (header plus pdu data) from `reader`.
///
/// At EOF this returns `BlktraceError::Io` with
/// [`std::io::ErrorKind::UnexpectedEof`]; callers retry while their trace
/// session is still live, because the kernel appends records to the relay
/// channel as they happen.
// The relay-channel reader that calls this is not wired up yet.
#[allow(dead_code)]
pub(crate) fn read_event(reader: &mut impl Read) -> Result<BlkTraceEvent, BlktraceError> {
    // blk_io_trace is plain old data, so the on-disk form is the in-memory
    // form; read the bytes straight into it.
    let mut blk_event = std::mem::MaybeUninit::<blk_io_trace>::uninit();
    let ptr = unsafe {
        &mut *blk_event
            .as_mut_ptr()
            .cast::<[u8; std::mem::size_of::<blk_io_trace>()]>()
    };
    reader.read_exact(ptr)?;

    let blk_event = unsafe { blk_event.assume_init() };
    if blk_event.magic != BLK_IO_TRACE_MAGIC + BLK_IO_TRACE_VERSION {
        return Err(BlktraceError::MagicMismatch);
    }

    let mut data = vec![0; blk_event.pdu_len as usize];
    reader.read_exact(&mut data)?;

    Ok(BlkTraceEvent {
        event: blk_event,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(action: u32, pdu_len: u16, magic: u32) -> blk_io_trace {
        blk_io_trace {
            magic,
            action,
            pdu_len,
            ..Default::default()
        }
    }

    fn valid_trace(action: u32, pdu_len: u16) -> blk_io_trace {
        trace(action, pdu_len, BLK_IO_TRACE_MAGIC + BLK_IO_TRACE_VERSION)
    }

    // The wire format is the raw C struct, so serialize it back the same way
    // the reader deserializes it: a plain byte view of the repr(C) value.
    fn to_bytes(trace: &blk_io_trace, pdu: &[u8]) -> Vec<u8> {
        let header = unsafe {
            std::slice::from_raw_parts(
                trace as *const _ as *const u8,
                std::mem::size_of::<blk_io_trace>(),
            )
        };
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(pdu);
        bytes
    }

    #[test]
    fn read_event_parses_header_and_pdu() {
        let read_action = blktrace_cat_BLK_TC_READ << BLK_TC_SHIFT;
        let bytes = to_bytes(&valid_trace(read_action, 3), b"xyz");
        let mut reader = &bytes[..];

        let event = read_event(&mut reader).unwrap();
        assert_eq!(event.data, b"xyz");
        assert!(event.is_read());
        assert!(!event.is_write());
        assert!(!event.is_discard());
        // The pdu data is fully consumed.
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn read_event_rejects_bad_magic() {
        let bytes = to_bytes(&trace(0, 0, 0), b"");
        let mut reader = &bytes[..];

        assert!(matches!(
            read_event(&mut reader),
            Err(BlktraceError::MagicMismatch)
        ));
    }

    #[test]
    fn read_event_reports_eof_on_empty_input() {
        let mut reader = &b""[..];

        let err = read_event(&mut reader).unwrap_err();
        let kind = match &err {
            BlktraceError::Io(e) => e.kind(),
            _ => panic!("expected an io error, got {err:?}"),
        };
        assert_eq!(kind, std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_event_reports_eof_on_truncated_pdu() {
        let bytes = to_bytes(&valid_trace(0, 4), b"xy");
        let mut reader = &bytes[..];

        let err = read_event(&mut reader).unwrap_err();
        let kind = match &err {
            BlktraceError::Io(e) => e.kind(),
            _ => panic!("expected an io error, got {err:?}"),
        };
        assert_eq!(kind, std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn event_classification_by_action_bits() {
        let read = blk_io_trace {
            action: blktrace_cat_BLK_TC_READ << BLK_TC_SHIFT,
            ..Default::default()
        };
        let write = blk_io_trace {
            action: blktrace_cat_BLK_TC_WRITE << BLK_TC_SHIFT,
            ..Default::default()
        };
        let discard = blk_io_trace {
            action: blktrace_cat_BLK_TC_DISCARD << BLK_TC_SHIFT,
            ..Default::default()
        };

        assert!(BlkTraceEvent {
            event: read,
            data: vec![]
        }
        .is_read());
        assert!(BlkTraceEvent {
            event: write,
            data: vec![]
        }
        .is_write());
        assert!(BlkTraceEvent {
            event: discard,
            data: vec![]
        }
        .is_discard());
    }
}
