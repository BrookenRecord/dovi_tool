use super::*;
use blocks::{ExtMetadataBlockLevel3, ExtMetadataBlockLevel5};

fn count_bytes(count: u64, remaining_bytes: usize) -> Result<Vec<u8>> {
    let mut writer = BitstreamIoWriter::with_capacity(16);
    writer.write_ue(count)?;
    writer.byte_align()?;

    let mut bytes = writer.into_inner();
    bytes.resize(bytes.len() + remaining_bytes, 0);
    Ok(bytes)
}

fn check_roundtrip<T: BoundedExtMetadataBlocks + Default>(meta: T) -> Result<()> {
    let mut writer = BitstreamIoWriter::with_capacity(64);
    meta.write(&mut writer)?;
    writer.byte_align()?;
    let bytes = writer.into_inner();

    let mut reader = BsIoSliceReader::from_slice(&bytes);
    let parsed = DmData::parse::<T>(&mut reader)?.expect("metadata is present");
    assert_eq!(parsed.num_ext_blocks(), meta.num_ext_blocks());
    assert_eq!(parsed.blocks_ref().len(), meta.blocks_ref().len());
    assert!(reader.available()? < 8);

    let mut writer = BitstreamIoWriter::with_capacity(64);
    parsed.write(&mut writer)?;
    writer.byte_align()?;
    assert_eq!(writer.into_inner(), bytes);
    Ok(())
}

#[test]
fn empty_metadata_roundtrips_both_versions() -> Result<()> {
    check_roundtrip(CmV29DmData::default())?;
    check_roundtrip(CmV40DmData::default())
}

#[test]
fn small_valid_counts_roundtrip_both_versions() -> Result<()> {
    let mut v29 = CmV29DmData::default();
    v29.add_block(ExtMetadataBlock::Level5(ExtMetadataBlockLevel5::default()))?;
    check_roundtrip(v29)?;

    let mut v40 = CmV40DmData::new_with_l254_402();
    v40.add_block(ExtMetadataBlock::Level3(ExtMetadataBlockLevel3::default()))?;
    check_roundtrip(v40)
}

#[derive(Debug)]
struct RefusalProbe;

impl Default for RefusalProbe {
    fn default() -> Self {
        panic!("impossible count must be refused before metadata construction");
    }
}

impl BoundedExtMetadataBlocks for RefusalProbe {
    fn parse_block_bounded(&mut self, _: &mut BsIoSliceReader) -> BoundedResult<()> {
        panic!("impossible count must be refused before block parsing");
    }
}

impl WithExtMetadataBlocks for RefusalProbe {
    const VERSION: &'static str = "test";
    const ALLOWED_BLOCK_LEVELS: &'static [u8] = &[];

    fn with_blocks_allocation(_: u64) -> Self {
        panic!("untrusted allocation constructor must not be used");
    }

    fn set_num_ext_blocks(&mut self, _: u64) {
        unreachable!();
    }

    fn num_ext_blocks(&self) -> u64 {
        unreachable!();
    }

    fn parse_block(&mut self, _: &mut BsIoSliceReader) -> Result<()> {
        panic!("impossible count must be refused before block parsing");
    }

    fn blocks_ref(&self) -> &Vec<ExtMetadataBlock> {
        unreachable!();
    }

    fn blocks_mut(&mut self) -> &mut Vec<ExtMetadataBlock> {
        unreachable!();
    }
}

#[test]
fn first_impossible_count_is_refused_before_construction() -> Result<()> {
    for remaining_bytes in 0..=4 {
        let count = (remaining_bytes * 8 / 9 + 1) as u64;
        let bytes = count_bytes(count, remaining_bytes)?;
        let mut reader = BsIoSliceReader::from_slice(&bytes);
        let error = DmData::parse::<RefusalProbe>(&mut reader).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Extension metadata block count exceeds remaining input"
        );
        assert_eq!(reader.available()?, (remaining_bytes * 8) as u64);
    }
    Ok(())
}

#[test]
fn impossible_count_is_refused_by_both_versions() -> Result<()> {
    let bytes = count_bytes(2, 2)?;
    let error = DmData::parse::<CmV29DmData>(&mut BsIoSliceReader::from_slice(&bytes)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Extension metadata block count exceeds remaining input"
    );
    let error = DmData::parse::<CmV40DmData>(&mut BsIoSliceReader::from_slice(&bytes)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Extension metadata block count exceeds remaining input"
    );
    Ok(())
}

#[test]
fn nonzero_alignment_is_refused_before_construction() {
    let error =
        DmData::parse::<RefusalProbe>(&mut BsIoSliceReader::from_slice(&[0x81])).unwrap_err();
    assert_eq!(error.to_string(), "dm_alignment_zero_bit != 0");
}

#[test]
fn truncated_count_is_refused_before_construction() {
    for bytes in [&[][..], &[0][..]] {
        let error =
            DmData::parse::<RefusalProbe>(&mut BsIoSliceReader::from_slice(bytes)).unwrap_err();
        assert_eq!(error.io_kind(), Some(std::io::ErrorKind::UnexpectedEof));
    }
}

#[test]
fn truncated_block_payload_returns_error() -> Result<()> {
    let mut writer = BitstreamIoWriter::with_capacity(16);
    CmV40DmData::new_with_l254_402().write(&mut writer)?;
    writer.byte_align()?;
    let mut bytes = writer.into_inner();
    bytes.pop();

    let error = DmData::parse::<CmV40DmData>(&mut BsIoSliceReader::from_slice(&bytes)).unwrap_err();
    assert_eq!(error.io_kind(), Some(std::io::ErrorKind::UnexpectedEof));
    Ok(())
}

#[test]
fn capacity_overflow_returns_fixed_reservation_error() {
    let mut blocks = Vec::new();
    let error = reserve_blocks(&mut blocks, usize::MAX).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Failed to reserve extension metadata blocks"
    );
    assert_eq!(blocks.capacity(), 0);
    assert!(blocks.is_empty());
}
