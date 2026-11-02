//! Parsed trace events and the record reader.

use std::io::Read;

use crate::error::BlktraceError;
use crate::uapi::{
    BlkIoTrace, BlkIoTrace2, BLK_IO_TRACE2_VERSION, BLK_IO_TRACE_MAGIC, BLK_IO_TRACE_VERSION,
    BLK_TC_DISCARD, BLK_TC_FLUSH, BLK_TC_READ, BLK_TC_SHIFT, BLK_TC_WRITE, BLK_TC_WRITE_ZEROES,
};

/// The kind of I/O a trace event represents.
///
/// Classified from the record's trace category bits. One record can carry
/// several category bits (a write-zeroes, discard or flush request also
/// has the write bit set); the classifier picks the most specific kind,
/// so [`BlkIoKind::WriteZeroes`] beats [`BlkIoKind::Write`] and so on.
///
/// Kernels before 6.19 trace write-zeroes requests with only the plain
/// write category, so on those a write-zeroes arrives as
/// [`BlkIoKind::Write`]; that is expected, not a parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlkIoKind {
    /// Ordinary read.
    Read,
    /// Ordinary write.
    Write,
    /// `REQ_OP_WRITE_ZEROES` (visible as this kind only on kernels with
    /// the v2 trace format).
    WriteZeroes,
    /// Discard / TRIM.
    Discard,
    /// Cache flush.
    Flush,
    /// Anything else the session recorded.
    ///
    /// Carries the record's raw `action` field: the full 64 bits of a v2
    /// record, zero-extended for a v1 one. The trace category bits sit at
    /// `action >> 16` (BLK_TC_SHIFT) and the low 16 bits select the
    /// concrete action (queue, issue, complete, ...), so callers can
    /// classify categories this enum does not name on their own.
    Other(u64),
}

/// One parsed trace record, normalized across the v1 and v2 record
/// formats, together with its trailing pdu data.
pub struct BlkTraceEvent {
    /// Kind of I/O, classified from the record's trace categories.
    pub kind: BlkIoKind,
    /// Disk offset in 512-byte sectors.
    pub sector: u64,
    /// Transfer length in bytes.
    pub bytes: u32,
    /// Process that issued the I/O.
    pub pid: u32,
    /// CPU the event happened on.
    pub cpu: u32,
    /// Device number.
    pub device: u32,
    /// Kernel-assigned event number.
    pub sequence: u32,
    /// Event timestamp in nanoseconds.
    pub time_ns: u64,
    /// Completion error, if any.
    pub error: u16,
    /// The variable-length pdu data following the record.
    pub data: Vec<u8>,
}

impl std::fmt::Debug for BlkTraceEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlkTraceEvent")
            .field("kind", &self.kind)
            .field("sector", &self.sector)
            .field("bytes", &self.bytes)
            .field("pid", &self.pid)
            .field("cpu", &self.cpu)
            .field("device", &self.device)
            .field("sequence", &self.sequence)
            .field("time_ns", &self.time_ns)
            .field("error", &self.error)
            .field("data_len", &self.data.len())
            .finish()
    }
}

impl BlkTraceEvent {
    /// Whether the event is a read.
    pub fn is_read(&self) -> bool {
        self.kind == BlkIoKind::Read
    }

    /// Whether the event is an ordinary write. Write-zeroes requests on
    /// kernels without the v2 trace format count as writes here, which is
    /// what those kernels report.
    pub fn is_write(&self) -> bool {
        self.kind == BlkIoKind::Write
    }

    /// Whether the event is a write-zeroes request. Only kernels with the
    /// v2 trace format report this kind; older kernels report the request
    /// as an ordinary write.
    pub fn is_write_zeroes(&self) -> bool {
        self.kind == BlkIoKind::WriteZeroes
    }

    /// Whether the event is a discard.
    pub fn is_discard(&self) -> bool {
        self.kind == BlkIoKind::Discard
    }
}

/// Maps a record's raw action to a kind.
///
/// The trace categories live at `action >> BLK_TC_SHIFT`. The specific
/// kinds are tested before the direction bits because a write-zeroes,
/// discard or flush category comes paired with `BLK_TC_WRITE` in the
/// action. A category the classifier does not name yields
/// [`BlkIoKind::Other`] carrying the raw action.
fn kind_from_action(action: u64) -> BlkIoKind {
    let category = action >> BLK_TC_SHIFT;
    if category & BLK_TC_WRITE_ZEROES != 0 {
        BlkIoKind::WriteZeroes
    } else if category & BLK_TC_DISCARD != 0 {
        BlkIoKind::Discard
    } else if category & BLK_TC_FLUSH != 0 {
        BlkIoKind::Flush
    } else if category & BLK_TC_READ != 0 {
        BlkIoKind::Read
    } else if category & BLK_TC_WRITE != 0 {
        BlkIoKind::Write
    } else {
        BlkIoKind::Other(action)
    }
}

/// Reads the rest of a trace record, after the magic has already been
/// consumed, into `T`.
///
/// # Safety
/// `T` must be a plain-old-data `#[repr(C)]` trace record whose first
/// field is the `u32` magic: every byte of the struct is a directly
/// readable integer field, so the kernel's bytes are a valid `T`.
unsafe fn read_record_tail<T>(reader: &mut impl Read, magic: [u8; 4]) -> Result<T, BlktraceError> {
    let mut raw = std::mem::MaybeUninit::<T>::uninit();
    // SAFETY: caller contract above; the four magic bytes go back at
    // offset 0 of the aligned storage and the remaining bytes are read
    // straight into the struct's own bytes.
    unsafe {
        *raw.as_mut_ptr().cast::<[u8; 4]>() = magic;
        let tail = std::slice::from_raw_parts_mut(
            raw.as_mut_ptr().cast::<u8>().add(4),
            std::mem::size_of::<T>() - 4,
        );
        reader.read_exact(tail)?;
        Ok(raw.assume_init())
    }
}

/// Reads one record (header plus pdu data) from `reader`.
///
/// The record's magic carries its format version in the low byte, so the
/// v1 and v2 layouts are dispatched on that.
///
/// At EOF this returns `BlktraceError::Io` with
/// [`std::io::ErrorKind::UnexpectedEof`]; callers retry while their trace
/// session is still live, because the kernel appends records to the relay
/// channel as they happen.
pub(crate) fn read_event(reader: &mut impl Read) -> Result<BlkTraceEvent, BlktraceError> {
    let mut magic_bytes = [0u8; 4];
    reader.read_exact(&mut magic_bytes)?;
    let magic = u32::from_ne_bytes(magic_bytes);

    // A record header is `BLK_IO_TRACE_MAGIC | version`; anything else is
    // not a blktrace record at all.
    if magic & !0xff != BLK_IO_TRACE_MAGIC & !0xff {
        return Err(BlktraceError::MagicMismatch);
    }
    let version = (magic & 0xff) as u8;

    if version == BLK_IO_TRACE_VERSION as u8 {
        // SAFETY: BlkIoTrace is a plain-old-data repr(C) record with
        // the u32 magic first.
        let rec: BlkIoTrace = unsafe { read_record_tail(reader, magic_bytes)? };
        Ok(BlkTraceEvent {
            kind: kind_from_action(rec.action as u64),
            sector: rec.sector,
            bytes: rec.bytes,
            pid: rec.pid,
            cpu: rec.cpu,
            device: rec.device,
            sequence: rec.sequence,
            time_ns: rec.time,
            error: rec.error,
            data: read_pdu(reader, rec.pdu_len)?,
        })
    } else if version == BLK_IO_TRACE2_VERSION as u8 {
        // SAFETY: BlkIoTrace2 is a plain-old-data repr(C) record with the
        // u32 magic first; the pad bytes are a valid [u8; 12] whatever
        // the kernel wrote there.
        let rec: BlkIoTrace2 = unsafe { read_record_tail(reader, magic_bytes)? };
        Ok(BlkTraceEvent {
            kind: kind_from_action(rec.action),
            sector: rec.sector,
            bytes: rec.bytes,
            pid: rec.pid,
            cpu: rec.cpu,
            device: rec.device,
            sequence: rec.sequence,
            time_ns: rec.time,
            error: rec.error,
            data: read_pdu(reader, rec.pdu_len)?,
        })
    } else {
        Err(BlktraceError::UnsupportedRecordVersion(version))
    }
}

fn read_pdu(reader: &mut impl Read, pdu_len: u16) -> Result<Vec<u8>, BlktraceError> {
    let mut data = vec![0; pdu_len as usize];
    reader.read_exact(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_magic() -> u32 {
        BLK_IO_TRACE_MAGIC | BLK_IO_TRACE_VERSION
    }

    fn v2_magic() -> u32 {
        BLK_IO_TRACE_MAGIC | BLK_IO_TRACE2_VERSION
    }

    fn v1_trace(action: u32, pdu_len: u16) -> BlkIoTrace {
        BlkIoTrace {
            magic: v1_magic(),
            action,
            pdu_len,
            ..Default::default()
        }
    }

    fn v2_trace(action: u64, pdu_len: u16) -> BlkIoTrace2 {
        BlkIoTrace2 {
            magic: v2_magic(),
            action,
            pdu_len,
            ..Default::default()
        }
    }

    fn cat(action: u64) -> u64 {
        action << BLK_TC_SHIFT
    }

    // The wire format is the raw C struct, so serialize records back the
    // same way the reader deserializes them: a plain byte view of the
    // repr(C) value, header then pdu.
    fn v1_bytes(event: &BlkIoTrace, pdu: &[u8]) -> Vec<u8> {
        let header = unsafe {
            std::slice::from_raw_parts(
                event as *const _ as *const u8,
                std::mem::size_of::<BlkIoTrace>(),
            )
        };
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(pdu);
        bytes
    }

    fn v2_bytes(event: &BlkIoTrace2, pdu: &[u8]) -> Vec<u8> {
        let header = unsafe {
            std::slice::from_raw_parts(
                event as *const _ as *const u8,
                std::mem::size_of::<BlkIoTrace2>(),
            )
        };
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(pdu);
        bytes
    }

    #[test]
    fn read_event_v1_parses_and_normalizes() {
        let event = BlkIoTrace {
            magic: v1_magic(),
            sequence: 42,
            time: 1234,
            sector: 8,
            bytes: 512,
            action: (BLK_TC_READ << BLK_TC_SHIFT) as u32,
            pid: 100,
            device: 2059,
            cpu: 3,
            error: 5,
            pdu_len: 3,
        };
        let bytes = v1_bytes(&event, b"xyz");
        let mut reader = &bytes[..];

        let parsed = read_event(&mut reader).unwrap();
        assert_eq!(parsed.kind, BlkIoKind::Read);
        assert_eq!(parsed.sector, 8);
        assert_eq!(parsed.bytes, 512);
        assert_eq!(parsed.pid, 100);
        assert_eq!(parsed.cpu, 3);
        assert_eq!(parsed.device, 2059);
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.time_ns, 1234);
        assert_eq!(parsed.error, 5);
        assert_eq!(parsed.data, b"xyz");
        assert!(parsed.is_read());
        assert!(!parsed.is_write());
        assert!(!parsed.is_write_zeroes());
        assert!(!parsed.is_discard());
        // The record is fully consumed.
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn read_event_v2_parses_and_normalizes() {
        // A write-zeroes record carries the write bit alongside its own
        // category; the normalized kind must be the more specific one.
        let event = BlkIoTrace2 {
            magic: v2_magic(),
            sequence: 7,
            time: 99,
            sector: 2048,
            bytes: 4096,
            action: cat(BLK_TC_WRITE | BLK_TC_WRITE_ZEROES),
            pid: 7,
            device: 2059,
            cpu: 1,
            error: 0,
            pdu_len: 2,
            pad: [0; 12],
        };
        let bytes = v2_bytes(&event, b"ab");
        let mut reader = &bytes[..];

        let parsed = read_event(&mut reader).unwrap();
        assert_eq!(parsed.kind, BlkIoKind::WriteZeroes);
        assert_eq!(parsed.sector, 2048);
        assert_eq!(parsed.bytes, 4096);
        assert_eq!(parsed.pid, 7);
        assert_eq!(parsed.cpu, 1);
        assert_eq!(parsed.device, 2059);
        assert_eq!(parsed.sequence, 7);
        assert_eq!(parsed.time_ns, 99);
        assert_eq!(parsed.error, 0);
        assert_eq!(parsed.data, b"ab");
        assert!(parsed.is_write_zeroes());
        assert!(!parsed.is_write());
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn read_event_v2_reads_plain_write() {
        let bytes = v2_bytes(&v2_trace(cat(BLK_TC_WRITE), 0), b"");
        let mut reader = &bytes[..];

        let parsed = read_event(&mut reader).unwrap();
        assert_eq!(parsed.kind, BlkIoKind::Write);
    }

    #[test]
    fn write_zeroes_on_pre_v2_kernels_arrives_as_write() {
        // Kernels before 6.19 trace write-zeroes bios with only the
        // BLK_TC_WRITE category; classifying them as Write is the
        // expected behavior, not a parser gap.
        let bytes = v1_bytes(&v1_trace((BLK_TC_WRITE << BLK_TC_SHIFT) as u32, 0), b"");
        let mut reader = &bytes[..];

        let parsed = read_event(&mut reader).unwrap();
        assert_eq!(parsed.kind, BlkIoKind::Write);
        assert!(!parsed.is_write_zeroes());
    }

    #[test]
    fn category_classification_prefers_the_specific_bit() {
        assert_eq!(kind_from_action(cat(BLK_TC_READ)), BlkIoKind::Read);
        assert_eq!(kind_from_action(cat(BLK_TC_WRITE)), BlkIoKind::Write);
        assert_eq!(
            kind_from_action(cat(BLK_TC_WRITE | BLK_TC_WRITE_ZEROES)),
            BlkIoKind::WriteZeroes
        );
        assert_eq!(
            kind_from_action(cat(BLK_TC_WRITE | BLK_TC_DISCARD)),
            BlkIoKind::Discard
        );
        assert_eq!(
            kind_from_action(cat(BLK_TC_WRITE | BLK_TC_FLUSH)),
            BlkIoKind::Flush
        );
        assert_eq!(kind_from_action(0), BlkIoKind::Other(0));
    }

    #[test]
    fn unknown_categories_carry_the_raw_action() {
        // A category the classifier does not name (zone append, bit 16)
        // yields Other with the record's raw action preserved.
        let action = cat(1 << 16) | 8;
        assert_eq!(kind_from_action(action), BlkIoKind::Other(action));

        let bytes = v2_bytes(&v2_trace(action, 0), b"");
        let mut reader = &bytes[..];
        assert_eq!(
            read_event(&mut reader).unwrap().kind,
            BlkIoKind::Other(action)
        );

        // A v1 record's 32-bit action reaches Other zero-extended.
        let bytes = v1_bytes(&v1_trace(2, 0), b"");
        let mut reader = &bytes[..];
        assert_eq!(read_event(&mut reader).unwrap().kind, BlkIoKind::Other(2));
    }

    #[test]
    fn read_event_rejects_bad_magic() {
        let mut event = v1_trace(0, 0);
        event.magic = 0;
        let bytes = v1_bytes(&event, b"");
        let mut reader = &bytes[..];

        assert!(matches!(
            read_event(&mut reader),
            Err(BlktraceError::MagicMismatch)
        ));
    }

    #[test]
    fn read_event_rejects_unknown_record_version() {
        let mut event = v1_trace(0, 0);
        // Right magic prefix, version byte the reader knows nothing about.
        event.magic = BLK_IO_TRACE_MAGIC | 0x09;
        let bytes = v1_bytes(&event, b"");
        let mut reader = &bytes[..];

        assert!(matches!(
            read_event(&mut reader),
            Err(BlktraceError::UnsupportedRecordVersion(9))
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
    fn read_event_reports_eof_on_truncated_header() {
        let mut bytes = v1_bytes(&v1_trace(0, 0), b"");
        bytes.truncate(10);
        let mut reader = &bytes[..];

        let err = read_event(&mut reader).unwrap_err();
        let kind = match &err {
            BlktraceError::Io(e) => e.kind(),
            _ => panic!("expected an io error, got {err:?}"),
        };
        assert_eq!(kind, std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_event_reports_eof_on_truncated_pdu() {
        let bytes = v1_bytes(&v1_trace(0, 4), b"xy");
        let mut reader = &bytes[..];

        let err = read_event(&mut reader).unwrap_err();
        let kind = match &err {
            BlktraceError::Io(e) => e.kind(),
            _ => panic!("expected an io error, got {err:?}"),
        };
        assert_eq!(kind, std::io::ErrorKind::UnexpectedEof);
    }
}
