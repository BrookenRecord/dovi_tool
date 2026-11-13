use anyhow::{Result, ensure};
use bitvec_helpers::{
    bitstream_io_reader::BsIoSliceReader, bitstream_io_writer::BitstreamIoWriter,
};

use super::validation::validate_widths;
use super::{DoviMappingPiece, DoviNlqMethod, DoviPolynomialPiece, RpuDataMapping};
use crate::rpu::{
    NLQ_NUM_PIVOTS,
    rpu_data_header::RpuDataHeader,
    rpu_data_nlq::{DoviELType, RpuDataNlq},
};

impl RpuDataMapping {
    pub(crate) fn parse(reader: &mut BsIoSliceReader, header: &RpuDataHeader) -> Result<Self> {
        let mut mapping = Self {
            vdr_rpu_id: reader.read_ue()?,
            mapping_color_space: reader.read_ue()?,
            mapping_chroma_format_idc: reader.read_ue()?,
            ..Default::default()
        };
        validate_widths(header)?;
        let bl_bit_depth = header.bl_bit_depth_minus8 as u32 + 8;

        for curve in &mut mapping.curves {
            curve.num_pivots_minus2 = reader.read_ue()?;
            ensure!(
                curve.num_pivots_minus2 <= 7,
                "num_pivots_minus2 must be at most 7"
            );
            let num_pivots = curve.num_pivots_minus2 as usize + 2;
            curve
                .pivots
                .try_reserve_exact(num_pivots)
                .map_err(|_| anyhow::anyhow!("Unable to reserve mapping pivots"))?;
            for _ in 0..num_pivots {
                curve.pivots.push(reader.read_var(bl_bit_depth)?);
            }
        }

        if header.rpu_format & 0x700 == 0 && !header.disable_residual_flag {
            ensure!(reader.read::<3, u8>()? == 0, "Unsupported NLQ method");
            mapping.nlq_method_idc = Some(DoviNlqMethod::LinearDeadzone);
            mapping.nlq_num_pivots_minus2 = Some(0);
            let mut pivots = [0; NLQ_NUM_PIVOTS];
            for pivot in &mut pivots {
                *pivot = reader.read_var(bl_bit_depth)?;
            }
            mapping.nlq_pred_pivot_value = Some(pivots);
        }
        mapping.num_x_partitions_minus1 = reader.read_ue()?;
        mapping.num_y_partitions_minus1 = reader.read_ue()?;

        for curve in &mut mapping.curves {
            let num_pieces = curve.num_pivots_minus2 as usize + 1;
            curve
                .pieces
                .try_reserve_exact(num_pieces)
                .map_err(|_| anyhow::anyhow!("Unable to reserve mapping pieces"))?;
            for _ in 0..num_pieces {
                curve.pieces.push(DoviMappingPiece::parse(reader, header)?);
            }
        }
        if mapping.nlq_method_idc.is_some() {
            mapping.nlq = Some(RpuDataNlq::parse(reader, header, &mapping)?);
        }
        mapping.validate_with_header(header)?;
        Ok(mapping)
    }

    /// Validate the complete mapping against its header before emitting any mapping bits.
    /// Invalid objects return an error without changing writer contents. An I/O error after
    /// writing starts may leave partial output; no I/O rollback is provided.
    pub fn write(&self, writer: &mut BitstreamIoWriter, header: &RpuDataHeader) -> Result<()> {
        self.validate_with_header(header)?;
        let bl_bit_depth = header.bl_bit_depth_minus8 as u32 + 8;
        writer.write_ue(self.vdr_rpu_id)?;
        writer.write_ue(self.mapping_color_space)?;
        writer.write_ue(self.mapping_chroma_format_idc)?;
        for curve in &self.curves {
            writer.write_ue(curve.num_pivots_minus2)?;
            for &pivot in &curve.pivots {
                writer.write_var(bl_bit_depth, pivot)?;
            }
        }
        if let Some(method) = self.nlq_method_idc {
            writer.write::<3, u8>(method as u8)?;
            if let Some(pivots) = self.nlq_pred_pivot_value {
                for pivot in pivots {
                    writer.write_var(bl_bit_depth, pivot)?;
                }
            }
        }
        writer.write_ue(self.num_x_partitions_minus1)?;
        writer.write_ue(self.num_y_partitions_minus1)?;
        for curve in &self.curves {
            for piece in &curve.pieces {
                piece.write(writer, header)?;
            }
        }
        if let Some(nlq) = &self.nlq {
            nlq.write(writer, header, self)?;
        }
        Ok(())
    }

    /// Replace all component curves with the profile 8.1 identity polynomial.
    /// Existing NLQ metadata is retained; profile conversion owns its removal.
    pub fn set_empty_p81_mapping(&mut self) {
        for curve in &mut self.curves {
            curve.num_pivots_minus2 = 0;
            curve.pivots.clear();
            curve.pivots.extend([0, 1023]);
            curve.pieces.clear();
            curve.pieces.push(DoviMappingPiece::Polynomial(
                DoviPolynomialPiece::p81_default(),
            ));
        }
    }

    /// Return the enhancement-layer type when this mapping carries NLQ data.
    pub fn get_enhancement_layer_type(&self) -> Option<DoviELType> {
        self.nlq.as_ref().map(|nlq| nlq.el_type())
    }
}
