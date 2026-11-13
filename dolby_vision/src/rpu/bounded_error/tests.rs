use super::*;
use crate::rpu::dovi_rpu::DoviRpu;
use crate::rpu::extension_metadata::blocks::ExtMetadataBlock;
use crate::rpu::extension_metadata::{CmV29DmData, DmData};
use crate::rpu::generate::GenerateConfig;
use crate::rpu::rpu_data_header::RpuDataHeader;
use crate::rpu::rpu_data_mapping::{DoviMappingPiece, DoviPolynomialPiece};
use bitvec_helpers::{
    bitstream_io_reader::BsIoSliceReader, bitstream_io_writer::BitstreamIoWriter,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

// Thread-local accounting leaves concurrent library tests unaffected. Record exact
// allocator requests, including reallocations, without allocating in the observer.
#[derive(Default, Copy, Clone, Debug)]
struct Requests {
    count: usize,
    bytes: usize,
    largest: usize,
    reallocations: usize,
}
thread_local! {
    static REQUESTS: Cell<Option<Requests>> = const { Cell::new(None) };
}
struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn record(size: usize, reallocation: bool) {
    let _ = REQUESTS.try_with(|slot| {
        if let Some(mut requests) = slot.get() {
            requests.count += 1;
            requests.bytes += size;
            requests.largest = requests.largest.max(size);
            requests.reallocations += usize::from(reallocation);
            slot.set(Some(requests));
        }
    });
}
// SAFETY: Every allocation/deallocation is forwarded unchanged to System. The
// observer only uses fixed thread-local cells and never allocates recursively.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size(), false);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size(), false);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size, true);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
fn measured<T>(operation: impl FnOnce() -> T) -> (T, Requests) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REQUESTS.with(|slot| slot.set(None));
        }
    }
    REQUESTS.with(|slot| {
        assert!(slot.get().is_none());
        slot.set(Some(Requests::default()));
    });
    let reset = Reset;
    let value = operation();
    let requests = REQUESTS.with(|slot| slot.get().unwrap());
    drop(reset);
    (value, requests)
}
fn fixture() -> DoviRpu {
    let mut rpu = DoviRpu::profile81_config(&GenerateConfig::default()).unwrap();
    rpu.header.vdr_dm_metadata_present_flag = false;
    rpu.vdr_dm_data = None;
    rpu
}
fn hevc(raw: &[u8]) -> Vec<u8> {
    let mut data = raw.to_vec();
    crate::utils::add_start_code_emulation_prevention_3_byte(&mut data);
    data
}
fn with_tail(mut raw: Vec<u8>, count: usize) -> Vec<u8> {
    raw.truncate(raw.len() - 5);
    raw.resize(raw.len() + count, 0xff);
    let crc = crate::rpu::compute_crc32(&raw[1..]);
    raw.extend(crc.to_be_bytes());
    raw.push(0x80);
    raw
}
fn framed(mut writer: BitstreamIoWriter) -> Vec<u8> {
    writer.byte_align().unwrap();
    let mut data = writer.into_inner();
    data.resize(data.len().max(20), 0x55);
    let crc = crate::rpu::compute_crc32(&data[1..]);
    data.extend(crc.to_be_bytes());
    data.push(0x80);
    hevc(&data)
}
fn mapping_prefix() -> BitstreamIoWriter {
    let mut writer = BitstreamIoWriter::with_capacity(128);
    writer.write::<8, u8>(25).unwrap();
    let mut header = RpuDataHeader::p8_default();
    header.vdr_dm_metadata_present_flag = false;
    header.write_header(&mut writer).unwrap();
    for _ in 0..3 {
        writer.write_ue(0).unwrap();
    }
    for _ in 0..3 {
        writer.write_ue(0).unwrap();
        writer.write::<10, u16>(0).unwrap();
        writer.write::<10, u16>(1023).unwrap();
    }
    writer.write_ue(0).unwrap();
    writer.write_ue(0).unwrap();
    writer
}

#[test]
fn bounded_diagnostic_truncates_utf8_and_context_without_allocating() {
    let ((error, text), requests) = measured(|| {
        let mut error = bounded_error!(
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxééé"
        );
        for context in [
            ParseContext::Header,
            ParseContext::Mapping,
            ParseContext::Dm,
            ParseContext::CmV29,
            ParseContext::Rpu,
        ] {
            error = error.context(context);
        }
        let mut text = [0_u8; 192];
        text[..error.message().len()].copy_from_slice(error.message().as_bytes());
        (error, text)
    });
    assert_eq!(requests.count, 0);
    assert_eq!(error.message().len(), 191);
    assert_eq!(text[191], 0);
    assert_eq!(error.contexts().count(), 4);
    assert_eq!(error.contexts().last(), Some(ParseContext::CmV29));
    assert!(std::mem::size_of::<BoundedParseError>() <= 256);
}

#[test]
fn bounded_valid_parity_and_legacy_large_tail_parse() {
    for tail in [0, 7000, 9000] {
        let mut raw = fixture().write_rpu().unwrap();
        if tail > 0 {
            raw = with_tail(raw, tail);
        }
        let bytes = hevc(&raw);
        let mut legacy = DoviRpu::parse_unspec62_nalu(&bytes).unwrap();
        assert_eq!(
            legacy.remaining.as_ref().map_or(0, |bits| bits.len()),
            tail * 8
        );
        let legacy_wire = legacy.write_rpu().map_err(|e| e.to_string());
        assert_eq!(
            DoviRpu::parse_rpu(&raw)
                .unwrap()
                .write_rpu()
                .map_err(|e| e.to_string()),
            legacy_wire
        );
        if bytes.len() <= 8192 {
            let mut bounded = DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap();
            assert_eq!(bounded.write_rpu().map_err(|e| e.to_string()), legacy_wire);
            assert_eq!(legacy.remaining.take(), bounded.remaining.take());
            assert_eq!(format!("{legacy:?}"), format!("{bounded:?}"));
        } else {
            let (result, requests) = measured(|| DoviRpu::parse_unspec62_nalu_bounded(&bytes));
            assert_eq!(result.unwrap_err().category(), ParseErrorCategory::Capacity);
            assert_eq!(requests.count, 0);
        }
    }
}

#[test]
fn bounded_corruption_orders_alignment_and_io_are_typed() {
    let raw = fixture().write_rpu().unwrap();
    let mut crc = raw.clone();
    let n = crc.len();
    crc[n - 3] ^= 1;
    let error = DoviRpu::parse_unspec62_nalu_bounded(&hevc(&crc)).unwrap_err();
    assert_eq!(error.category(), ParseErrorCategory::InvalidInput);
    assert!(error.message().contains("CRC32"));
    assert_eq!(error.contexts().last(), Some(ParseContext::Rpu));
    for (method, order, interpolation, category, text) in [
        (
            0,
            2,
            false,
            ParseErrorCategory::InvalidInput,
            "polynomial order",
        ),
        (1, 3, false, ParseErrorCategory::InvalidInput, "MMR order"),
        (0, 0, true, ParseErrorCategory::Unsupported, "interpolation"),
        (
            2,
            0,
            false,
            ParseErrorCategory::InvalidInput,
            "mapping method",
        ),
    ] {
        let mut writer = mapping_prefix();
        writer.write_ue(method).unwrap();
        if method == 1 {
            writer.write::<2, u8>(order).unwrap();
        } else if method == 0 {
            writer.write_ue(u64::from(order)).unwrap();
            if order == 0 {
                writer.write_bit(interpolation).unwrap();
            }
        }
        let error = DoviRpu::parse_unspec62_nalu_bounded(&framed(writer)).unwrap_err();
        assert_eq!(error.category(), category);
        assert!(error.message().contains(text), "{error}");
        assert_eq!(error.contexts().next(), Some(ParseContext::Mapping));
    }
    let mut writer = BitstreamIoWriter::with_capacity(64);
    writer.write::<8, u8>(25).unwrap();
    let rpu = fixture();
    rpu.header.write_header(&mut writer).unwrap();
    rpu.rpu_data_mapping
        .as_ref()
        .unwrap()
        .write(&mut writer, &rpu.header)
        .unwrap();
    writer.write_bit(true).unwrap(); // first required rpu_alignment_zero_bit
    let error = DoviRpu::parse_unspec62_nalu_bounded(&framed(writer)).unwrap_err();
    assert_eq!(error.category(), ParseErrorCategory::InvalidInput);
    assert_eq!(error.message(), "rpu_alignment_zero_bit != 0");
    let mut truncated = raw[..25].to_vec();
    truncated[24] = 0x80;
    let error = DoviRpu::parse_unspec62_nalu_bounded(&hevc(&truncated)).unwrap_err();
    assert_eq!(error.category(), ParseErrorCategory::Io);
    assert_eq!(error.io_kind(), Some(io::ErrorKind::UnexpectedEof));
    let legacy = DoviRpu::parse_unspec62_nalu(&hevc(&truncated)).unwrap_err();
    assert_eq!(
        legacy.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn bounded_cm_errors_retain_version_and_reader_kind() {
    let mut rpu = fixture();
    rpu.header.vdr_dm_metadata_present_flag = true;
    rpu.header.reserved_zero_3bits = 1; // compressed IDs, then CM v2.9
    for (version, disallowed) in [(ParseContext::CmV29, 3), (ParseContext::CmV40, 255)] {
        let mut writer = BitstreamIoWriter::with_capacity(128);
        writer.write::<8, u8>(25).unwrap();
        rpu.header.write_header(&mut writer).unwrap();
        rpu.rpu_data_mapping
            .as_ref()
            .unwrap()
            .write(&mut writer, &rpu.header)
            .unwrap();
        for _ in 0..3 {
            writer.write_ue(0).unwrap();
        }
        if version == ParseContext::CmV40 {
            writer.write_ue(0).unwrap(); // empty CM v2.9 preceding CM v4.0
            writer.byte_align().unwrap();
        }
        writer.write_ue(1).unwrap();
        writer.byte_align().unwrap();
        writer.write_ue(2).unwrap();
        writer.write::<8, u8>(disallowed).unwrap();
        // Keep enough bits after CM v2.9 to enter the CM v4.0 branch.
        for _ in 0..8 {
            writer.write::<8, u8>(0x55).unwrap();
        }
        let error = DoviRpu::parse_unspec62_nalu_bounded(&framed(writer)).unwrap_err();
        assert_eq!(error.category(), ParseErrorCategory::InvalidInput);
        assert_eq!(
            error.contexts().collect::<Vec<_>>(),
            [version, ParseContext::Dm, ParseContext::Rpu]
        );
        assert!(error.message().contains("Disallowed block level"));
    }
    let error =
        DmData::parse::<CmV29DmData>(&mut BsIoSliceReader::from_slice(&[0x81])).unwrap_err();
    assert_eq!(error.category(), ParseErrorCategory::InvalidInput);
    assert_eq!(error.message(), "dm_alignment_zero_bit != 0");
}

#[test]
fn bounded_maximum_declarations_and_tail_have_exact_requests() {
    // Largest syntactically admissible count for an 8192-byte count+payload slice.
    // The first payload is malformed, but the claimed count passes the input bound
    // and exercises the full reservation before its first block is rejected.
    let mut writer = BitstreamIoWriter::with_capacity(8192);
    let count = 7278;
    writer.write_ue(count).unwrap();
    writer.byte_align().unwrap();
    let mut bytes = writer.into_inner();
    bytes.resize(8192, 0);
    let (result, requests) =
        measured(|| DmData::parse::<CmV29DmData>(&mut BsIoSliceReader::from_slice(&bytes)));
    assert!(result.is_err());
    assert_eq!(
        requests.largest,
        count as usize * std::mem::size_of::<ExtMetadataBlock>()
    );
    assert_eq!(requests.reallocations, 0);
    assert!(requests.bytes <= requests.largest + 4096);
    let mut rpu = fixture();
    for curve in &mut rpu.rpu_data_mapping.as_mut().unwrap().curves {
        curve.num_pivots_minus2 = 7;
        curve.pivots = vec![0; 9];
        curve.pieces = vec![DoviMappingPiece::Polynomial(DoviPolynomialPiece::p81_default()); 8];
    }
    let raw = with_tail(rpu.write_rpu().unwrap(), 7000);
    let bytes = hevc(&raw);
    assert!(bytes.len() <= 8192);
    let (parsed, requests) = measured(|| DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap());
    assert_eq!(parsed.remaining.as_ref().unwrap().len(), 7000 * 8);
    let expected = bytes.len() + 54 + 24 * std::mem::size_of::<DoviMappingPiece>() + 7000;
    assert_eq!(requests.bytes, expected);
    assert_eq!(requests.count, 8); // EPB, three pivots, three pieces, one tail
    assert_eq!(requests.reallocations, 0);
    eprintln!(
        "count={count}, block_size={}, count_requests={}; mapping_size={}, tail_parse_requests={expected}",
        std::mem::size_of::<ExtMetadataBlock>(),
        count as usize * std::mem::size_of::<ExtMetadataBlock>(),
        std::mem::size_of::<DoviMappingPiece>()
    );
}

#[test]
fn bounded_errors_do_not_construct_anyhow() {
    if std::env::var_os("DOVI_BOUNDED_BACKTRACE_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "rpu::bounded_error::tests::bounded_errors_do_not_construct_anyhow",
                "--nocapture",
            ])
            .env("DOVI_BOUNDED_BACKTRACE_CHILD", "1")
            .env("RUST_BACKTRACE", "1")
            .env("RUST_LIB_BACKTRACE", "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let mut raw = fixture().write_rpu().unwrap();
    *raw.last_mut().unwrap() = 0x81;
    let bytes = hevc(&raw);
    let (bounded, requests) =
        measured(|| DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap_err());
    assert_eq!(bounded.category(), ParseErrorCategory::InvalidInput);
    assert_eq!(requests.count, 1); // only the EPB buffer; error and context add none
    assert_eq!(requests.bytes, bytes.len());
    let mut writer = mapping_prefix();
    writer.write_ue(0).unwrap();
    writer.write_ue(2).unwrap();
    let invalid_order = framed(writer);
    let (error, nested) =
        measured(|| DoviRpu::parse_unspec62_nalu_bounded(&invalid_order).unwrap_err());
    assert_eq!(error.contexts().next(), Some(ParseContext::Mapping));
    assert_eq!(nested.count, 5); // EPB, three pivots, first piece vector; no diagnostic allocation
    assert_eq!(
        nested.bytes,
        invalid_order.len() + 12 + std::mem::size_of::<DoviMappingPiece>()
    );
    let valid = fixture().write_rpu().unwrap();
    let mut truncated = valid[..25].to_vec();
    truncated[24] = 0x80;
    let truncated = hevc(&truncated);
    let (error, nested) =
        measured(|| DoviRpu::parse_unspec62_nalu_bounded(&truncated).unwrap_err());
    assert_eq!(error.io_kind(), Some(io::ErrorKind::UnexpectedEof));
    assert_eq!(nested.count, 6); // EPB, three pivots, two piece vectors; EOF is static
    let (legacy, requests) = measured(|| DoviRpu::parse_unspec62_nalu(&bytes).unwrap_err());
    assert_eq!(
        legacy.backtrace().status(),
        std::backtrace::BacktraceStatus::Captured
    );
    assert!(requests.count > 1); // positive control: Anyhow + enabled backtrace allocate
    let bytes = [0_u8; 8193];
    let (result, requests) = measured(|| DoviRpu::parse_unspec62_nalu_bounded(&bytes));
    assert_eq!(result.unwrap_err().category(), ParseErrorCategory::Capacity);
    assert_eq!(requests.count, 0);
}

#[test]
fn bounded_reader_error_requests_fit_the_fixed_allowance() {
    let mut overflow = [0_u8; 17];
    overflow[8] = 0x80;
    overflow[16] = 0x80;
    let mut signed_overflow = overflow;
    signed_overflow[16] = 0;
    for (bytes, signed) in [
        (&[0_u8; 9][..], false),
        (&overflow[..], false),
        (&signed_overflow[..], true),
    ] {
        let (error, requests) = measured(|| {
            let mut reader = BsIoSliceReader::from_slice(bytes);
            let error = if signed {
                reader.read_se().unwrap_err()
            } else {
                reader.read_ue().unwrap_err()
            };
            BoundedParseError::from(error)
        });
        assert_eq!(error.io_kind(), Some(io::ErrorKind::InvalidData));
        assert!(requests.bytes <= 4096);
        assert_eq!(requests.reallocations, 0);
        eprintln!("reader static error requests: {requests:?}");
    }
    let (error, requests) = measured(|| {
        BoundedParseError::from(
            BsIoSliceReader::from_slice(&[0])
                .read_var::<u8>(9)
                .unwrap_err(),
        )
    });
    assert_eq!(error.io_kind(), Some(io::ErrorKind::InvalidInput));
    assert!(requests.bytes <= 4096);
    assert_eq!(requests.reallocations, 0);
}

#[test]
fn bounded_profile_and_metadata_payloads_preserve_values_and_wire() {
    let config = GenerateConfig::default();
    let mut cmv40 = DoviRpu::profile81_config(&config).unwrap();
    cmv40.add_cmv40_safe_default_metadata().unwrap();
    for authored in [
        DoviRpu::profile5_config(&config).unwrap(),
        DoviRpu::profile81_config(&config).unwrap(),
        DoviRpu::profile84_config(&config).unwrap(),
        cmv40,
    ] {
        let bytes = authored.write_hevc_unspec62_nalu().unwrap();
        let legacy = DoviRpu::parse_unspec62_nalu(&bytes).unwrap();
        let bounded = DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap();
        assert_eq!(legacy.write_hevc_unspec62_nalu().unwrap(), bytes);
        assert_eq!(bounded.write_hevc_unspec62_nalu().unwrap(), bytes);
        assert_eq!(format!("{legacy:?}"), format!("{bounded:?}"));
    }
}

// Encode a CM v4.0 block with an independently authored declared length. Provide
// enough bounded payload for the level parser's existing conditional reads, so
// malformed lengths reach the shared length guard rather than an earlier EOF.
fn cmv40_declared_length(level: u8, length: u64) -> Vec<u8> {
    let mut rpu = fixture();
    rpu.header.vdr_dm_metadata_present_flag = true;
    rpu.header.reserved_zero_3bits = 1;
    let mut writer = BitstreamIoWriter::with_capacity(128);
    writer.write::<8, u8>(25).unwrap();
    rpu.header.write_header(&mut writer).unwrap();
    rpu.rpu_data_mapping
        .as_ref()
        .unwrap()
        .write(&mut writer, &rpu.header)
        .unwrap();
    for _ in 0..3 {
        writer.write_ue(0).unwrap();
    }
    writer.write_ue(0).unwrap(); // empty CM v2.9
    writer.byte_align().unwrap();
    writer.write_ue(2).unwrap(); // selected level plus required L254
    writer.byte_align().unwrap();
    writer.write_ue(length).unwrap();
    writer.write::<8, u8>(level).unwrap();
    let payload_bytes = match level {
        8 => match length {
            0..=10 => 10,
            11..=12 => 12,
            13 => 13,
            14..=19 => 19,
            _ => 25,
        },
        9 => {
            if length <= 1 {
                1
            } else {
                17
            }
        }
        10 => {
            if length <= 5 {
                5
            } else {
                21
            }
        }
        _ => unreachable!("test only authors variable-length levels"),
    };
    for _ in 0..payload_bytes {
        writer.write::<8, u8>(0).unwrap();
    }
    writer.write_ue(2).unwrap();
    writer.write::<8, u8>(254).unwrap();
    writer.write::<16, u16>(2).unwrap();
    framed(writer)
}

fn assert_declared_length_error(level: u8, length: u64) {
    let bytes = cmv40_declared_length(level, length);
    let error = DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap_err();
    assert_eq!(error.category(), ParseErrorCategory::InvalidInput);
    assert_eq!(
        error.message(),
        format!("Invalid metadata block length {length} for level {level}")
    );
    assert_eq!(
        error.contexts().collect::<Vec<_>>(),
        [ParseContext::CmV40, ParseContext::Dm, ParseContext::Rpu]
    );
    assert_eq!(error.io_kind(), None);
}

#[test]
fn bounded_invalid_declared_lengths_are_fixed_errors() {
    for (level, length) in [(8, 11), (9, 2), (10, 6)] {
        assert_declared_length_error(level, length);
    }
}

#[test]
fn bounded_zero_l9_length_is_fixed_error() {
    assert_declared_length_error(9, 0);
}

#[test]
fn bounded_huge_declared_length_is_fixed_error() {
    assert_declared_length_error(8, u64::MAX);
}

#[test]
fn bounded_supported_declared_lengths_preserve_values() {
    for (level, lengths) in [
        (8, &[10, 12, 13, 19, 25][..]),
        (9, &[1, 17][..]),
        (10, &[5, 21][..]),
    ] {
        for &length in lengths {
            let bytes = cmv40_declared_length(level, length);
            let legacy = DoviRpu::parse_unspec62_nalu(&bytes).unwrap();
            let bounded = DoviRpu::parse_unspec62_nalu_bounded(&bytes).unwrap();
            let block = bounded
                .vdr_dm_data
                .as_ref()
                .unwrap()
                .get_block(level)
                .unwrap();
            assert_eq!(block.length_bytes(), length);
            assert_eq!(format!("{legacy:?}"), format!("{bounded:?}"));
        }
    }
}
