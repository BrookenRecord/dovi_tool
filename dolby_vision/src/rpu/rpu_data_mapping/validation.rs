use anyhow::{Result, ensure};

use super::RpuDataMapping;
use crate::rpu::rpu_data_header::RpuDataHeader;

pub(super) fn validate_widths(header: &RpuDataHeader) -> Result<()> {
    ensure!(
        header.bl_bit_depth_minus8 <= 8 && header.el_bit_depth_minus8 <= 8,
        "Invalid mapping bit depth"
    );
    ensure!(
        match header.coefficient_data_type {
            0 =>
                header.coefficient_log2_denom <= 23
                    && header.coefficient_log2_denom_length == header.coefficient_log2_denom as u32,
            1 => header.coefficient_log2_denom_length == 32,
            _ => false,
        },
        "Invalid mapping coefficient width"
    );
    Ok(())
}

impl RpuDataMapping {
    /// Check profile restrictions and mapping shapes without coefficient/header widths.
    /// Use validate_with_header for complete write/export validation.
    pub fn validate(&self, profile: u8) -> Result<()> {
        ensure!(self.vdr_rpu_id <= 15, "Invalid mapping identifier");
        ensure!(
            self.mapping_color_space == 0,
            "mapping_color_space should be 0"
        );
        ensure!(
            self.mapping_chroma_format_idc == 0,
            "mapping_chroma_format_idc should be 0"
        );
        for curve in &self.curves {
            ensure!(
                curve.num_pivots_minus2 <= 7,
                "num_pivots_minus2 must be at most 7"
            );
            ensure!(
                curve.pivots.len() == curve.num_pivots_minus2 as usize + 2,
                "Invalid pivot count"
            );
            ensure!(
                curve.pieces.len() == curve.pivots.len() - 1,
                "Invalid mapping piece count"
            );
            for piece in &curve.pieces {
                piece.validate_shape()?;
            }
        }
        if matches!(profile, 5 | 8) {
            ensure!(
                self.nlq_method_idc.is_none()
                    && self.nlq_num_pivots_minus2.is_none()
                    && self.nlq_pred_pivot_value.is_none()
                    && self.nlq.is_none(),
                "NLQ is not allowed for this profile"
            );
        }
        if profile == 7 {
            ensure!(
                self.nlq_pred_pivot_value.is_some_and(|p| p
                    .iter()
                    .map(|v| u32::from(*v))
                    .sum::<u32>()
                    == 1023),
                "Invalid profile 7 NLQ pivots"
            );
        }
        Ok(())
    }

    /// Check every mapping field for this header, including NLQ presence and encoding.
    /// Integer coefficients must be writable SE values (i64::MIN is excluded).
    /// Pivot values remain coded deltas; no prefix sum or normalization is performed.
    pub fn validate_with_header(&self, header: &RpuDataHeader) -> Result<()> {
        validate_widths(header)?;
        header.validate(header.get_dovi_profile())?;
        ensure!(
            header.rpu_type == 2
                && header.rpu_format <= 0x7ff
                && header.rpu_format & 0x700 == 0
                && header.vdr_rpu_profile <= 15
                && header.vdr_rpu_normalized_idc <= 3
                && header.reserved_zero_3bits <= 7
                && header.ext_mapping_idc_0_4 <= 31
                && header.ext_mapping_idc_5_7 <= 7,
            "Invalid mapping header fields"
        );
        ensure!(
            header.vdr_seq_info_present_flag && !header.use_prev_vdr_rpu_flag,
            "Mapping requires a self-contained header"
        );
        self.validate(header.get_dovi_profile())?;
        let pivot_limit = 1_u32 << (header.bl_bit_depth_minus8 as u32 + 8);
        for curve in &self.curves {
            ensure!(
                curve.pivots.iter().all(|p| u32::from(*p) < pivot_limit),
                "Pivot exceeds bit depth"
            );
            for piece in &curve.pieces {
                piece.validate(header)?;
            }
        }

        let needs_nlq = header.rpu_format & 0x700 == 0 && !header.disable_residual_flag;
        if needs_nlq {
            ensure!(
                self.nlq_method_idc.is_some()
                    && self.nlq_num_pivots_minus2 == Some(0)
                    && self.nlq_pred_pivot_value.is_some()
                    && self.nlq.is_some(),
                "Incomplete NLQ mapping"
            );
            if let Some(pivots) = self.nlq_pred_pivot_value {
                ensure!(
                    pivots.iter().all(|p| u32::from(*p) < pivot_limit),
                    "NLQ pivot exceeds bit depth"
                );
            }
            if let Some(nlq) = &self.nlq {
                let offset_limit = 1_u32 << (header.el_bit_depth_minus8 as u32 + 8);
                ensure!(
                    nlq.nlq_offset.iter().all(|v| u32::from(*v) < offset_limit),
                    "NLQ offset exceeds bit depth"
                );
                let limit = 1_u64 << header.coefficient_log2_denom_length;
                for values in [
                    &nlq.vdr_in_max,
                    &nlq.linear_deadzone_slope,
                    &nlq.linear_deadzone_threshold,
                ] {
                    ensure!(
                        values.iter().all(|v| *v < limit),
                        "NLQ value exceeds coefficient width"
                    );
                }
                if header.coefficient_data_type == 1 {
                    for values in [
                        &nlq.vdr_in_max_int,
                        &nlq.linear_deadzone_slope_int,
                        &nlq.linear_deadzone_threshold_int,
                    ] {
                        ensure!(
                            values.iter().all(|v| *v == 0),
                            "Unexpected NLQ integer for type 1"
                        );
                    }
                }
            }
        } else {
            ensure!(
                self.nlq_method_idc.is_none()
                    && self.nlq_num_pivots_minus2.is_none()
                    && self.nlq_pred_pivot_value.is_none()
                    && self.nlq.is_none(),
                "Unexpected NLQ mapping"
            );
        }
        Ok(())
    }
}
