use crate::rpu::bounded_error::{BoundedParseError, BoundedResult, bensure, bounded_error};
use anyhow::{Context, Result, ensure};
use bitvec_helpers::{
    bitstream_io_reader::BsIoSliceReader, bitstream_io_writer::BitstreamIoWriter,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

pub mod blocks;
pub mod cmv29;
pub mod cmv40;

pub mod primaries;
pub use primaries::*;

pub use cmv29::CmV29DmData;
pub use cmv40::CmV40DmData;

use blocks::ExtMetadataBlock;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum DmData {
    V29(CmV29DmData),
    V40(CmV40DmData),
}

pub trait ExtMetadata {
    fn parse(&mut self, reader: &mut BsIoSliceReader) -> Result<()>;
    fn write(&self, writer: &mut BitstreamIoWriter);
}

pub trait WithExtMetadataBlocks {
    const VERSION: &'static str;
    const ALLOWED_BLOCK_LEVELS: &'static [u8];

    fn with_blocks_allocation(num_ext_blocks: u64) -> Self;

    fn set_num_ext_blocks(&mut self, num_ext_blocks: u64);
    fn num_ext_blocks(&self) -> u64;

    fn parse_block(&mut self, reader: &mut BsIoSliceReader) -> Result<()>;
    fn blocks_ref(&self) -> &Vec<ExtMetadataBlock>;
    fn blocks_mut(&mut self) -> &mut Vec<ExtMetadataBlock>;

    fn sort_blocks(&mut self) {
        let blocks = self.blocks_mut();
        blocks.sort_by_key(|ext| ext.sort_key());
    }

    fn update_extension_block_info(&mut self) {
        self.set_num_ext_blocks(self.blocks_ref().len() as u64);
        self.sort_blocks();
    }

    fn add_block(&mut self, meta: ExtMetadataBlock) -> Result<()> {
        let level = meta.level();

        ensure!(
            Self::ALLOWED_BLOCK_LEVELS.contains(&level),
            "Metadata block level {level} is not allowed"
        );

        let blocks = self.blocks_mut();
        blocks.push(meta);

        self.update_extension_block_info();

        Ok(())
    }

    fn remove_level(&mut self, level: u8) {
        let blocks = self.blocks_mut();
        blocks.retain(|b| b.level() != level);

        self.update_extension_block_info();
    }

    fn write(&self, writer: &mut BitstreamIoWriter) -> Result<()> {
        let num_ext_blocks = self.num_ext_blocks();

        writer.write_ue(num_ext_blocks)?;

        // dm_alignment_zero_bit
        writer.byte_align()?;

        let ext_metadata_blocks = self.blocks_ref();

        for ext_metadata_block in ext_metadata_blocks {
            let level = ext_metadata_block.level();
            let remaining_bits =
                (ext_metadata_block.length_bits() - ext_metadata_block.required_bits()) as u32;

            writer.write_ue(ext_metadata_block.length_bytes())?;
            writer.write::<8, u8>(level)?;

            ext_metadata_block
                .write(writer)
                .with_context(|| format!("Level {level}"))?;

            // ext_dm_alignment_zero_bit
            writer.pad(remaining_bits)?;
        }

        Ok(())
    }
}

pub(crate) trait BoundedExtMetadataBlocks: WithExtMetadataBlocks {
    fn parse_block_bounded(&mut self, reader: &mut BsIoSliceReader) -> BoundedResult<()>;
}

impl DmData {
    pub(crate) fn parse<T: BoundedExtMetadataBlocks + Default>(
        reader: &mut BsIoSliceReader,
    ) -> BoundedResult<Option<T>> {
        let num_ext_blocks = reader.read_ue()?;
        while !reader.byte_aligned() {
            bensure!(!reader.read_bit()?, "dm_alignment_zero_bit != 0");
        }

        // Every block needs at least one UE length bit and eight level bits.
        // Later payloads or trailing data can only make this bound more permissive.
        bensure!(
            num_ext_blocks <= reader.available()? / 9,
            "Extension metadata block count exceeds remaining input"
        );
        let block_count = usize::try_from(num_ext_blocks).map_err(|_| {
            bounded_error!(
                Capacity,
                "Extension metadata block count does not fit usize"
            )
        })?;

        let mut meta = T::default();
        reserve_blocks(meta.blocks_mut(), block_count)?;
        meta.set_num_ext_blocks(num_ext_blocks);

        for _ in 0..block_count {
            meta.parse_block_bounded(reader)?;
        }

        Ok(Some(meta))
    }

    pub fn write(&self, writer: &mut BitstreamIoWriter) -> Result<()> {
        match self {
            DmData::V29(m) => m.write(writer),
            DmData::V40(m) => m.write(writer),
        }
    }

    /// Validate field constraints. Errors use a legacy Anyhow adapter; exact text
    /// and chain identity are unspecified, while reader I/O kinds remain downcastable.
    pub fn validate(&self) -> Result<()> {
        self.validate_bounded()
            .map_err(BoundedParseError::into_anyhow)
    }

    pub(crate) fn validate_bounded(&self) -> BoundedResult<()> {
        match self {
            DmData::V29(m) => m.validate_bounded(),
            DmData::V40(m) => m.validate_bounded(),
        }
    }
}

fn reserve_blocks(blocks: &mut Vec<ExtMetadataBlock>, count: usize) -> BoundedResult<()> {
    blocks
        .try_reserve_exact(count)
        .map_err(|_| bounded_error!(Capacity, "Failed to reserve extension metadata blocks"))
}

#[cfg(test)]
mod tests;
