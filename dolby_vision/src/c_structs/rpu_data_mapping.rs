//! Version 2 mapping ABI: each curve owns one ordered piece buffer; pivots are inline.

use anyhow::Result;
use libc::size_t;
use std::ptr::null;

use super::RpuDataNlq;
use crate::rpu::{
    NUM_COMPONENTS,
    rpu_data_header::RpuDataHeader,
    rpu_data_mapping::{DoviCoefficient, DoviMappingPiece, DoviReshapingCurve, RpuDataMapping},
};

/// Encoded coefficient in the version 2 mapping ABI.
#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct MappingCoefficientV2 {
    /// 1 for a signed integer plus fractional bits; 0 for raw 32-bit encoding.
    pub has_integer: u8,
    /// Signed integer when has_integer=1, otherwise zero.
    pub integer: i64,
    /// Fractional bits or raw 32-bit encoding, matching the RPU header.
    pub value: u64,
}

/// One ordered piece. Unused inline fields are zeroed, never separately allocated.
#[repr(C)]
#[derive(Default, Copy, Clone)]
pub struct MappingPieceV2 {
    /// 0 for polynomial, 1 for MMR.
    pub method: u8,
    /// 2 or 3 for polynomial; zero for MMR.
    pub polynomial_count: u8,
    /// Polynomial coefficients in wire order; only polynomial_count entries are used.
    pub polynomial: [MappingCoefficientV2; 3],
    /// 1..=3 for MMR; zero for polynomial.
    pub mmr_order_count: u8,
    /// MMR constant, or zeroed for polynomial.
    pub mmr_constant: MappingCoefficientV2,
    /// MMR orders in wire order, seven coefficients each; unused orders are zeroed.
    pub mmr_orders: [[MappingCoefficientV2; 7]; 3],
}

/// One component. The owning mapping must be freed exactly once with the v2 free function.
#[repr(C)]
pub struct ReshapingCurveV2 {
    /// Pivot count minus two, in 0..=7.
    pub num_pivots_minus2: u64,
    /// Two to nine coded pivot deltas; unused entries are zeroed.
    pub pivots: [u16; 9],
    /// Owned read-only piece buffer, valid until the parent mapping is freed.
    pieces: *const MappingPieceV2,
    /// 1..=8 entries in pieces, in interval order.
    piece_count: size_t,
}

/// Breaking version 2 C mapping representation; no legacy homogeneous projection exists.
/// Borrow all pointers read-only and release only through dovi_rpu_free_data_mapping_v2.
#[repr(C)]
pub struct RpuDataMappingV2 {
    /// Mapping identifier in 0..=15.
    pub vdr_rpu_id: u64,
    /// Mapping color space, currently zero.
    pub mapping_color_space: u64,
    /// Mapping chroma format, currently zero.
    pub mapping_chroma_format_idc: u64,
    /// Horizontal partition count minus one.
    pub num_x_partitions_minus1: u64,
    /// Vertical partition count minus one.
    pub num_y_partitions_minus1: u64,
    /// Three component curves; each owns exactly one piece buffer.
    pub curves: [ReshapingCurveV2; NUM_COMPONENTS],
    /// -1 if absent; otherwise linear deadzone method 0.
    pub nlq_method_idc: i32,
    /// -1 if absent; otherwise the supported value 0.
    pub nlq_num_pivots_minus2: i32,
    /// Coded NLQ pivots when NLQ is present; zeroed otherwise.
    pub nlq_pred_pivot_value: [u16; 2],
    /// Owned read-only NLQ data, null when absent; freed with the parent mapping.
    nlq: *const RpuDataNlq,
}

impl From<DoviCoefficient> for MappingCoefficientV2 {
    fn from(value: DoviCoefficient) -> Self {
        Self {
            has_integer: u8::from(value.integer.is_some()),
            integer: value.integer.unwrap_or(0),
            value: value.value,
        }
    }
}

impl From<&DoviMappingPiece> for MappingPieceV2 {
    fn from(piece: &DoviMappingPiece) -> Self {
        let mut out = Self::default();
        match piece {
            DoviMappingPiece::Polynomial(poly) => {
                out.polynomial_count = poly.coefficients.len() as u8;
                for (dest, source) in out.polynomial.iter_mut().zip(&poly.coefficients) {
                    *dest = (*source).into();
                }
            }
            DoviMappingPiece::MMR(mmr) => {
                out.method = 1;
                out.mmr_order_count = mmr.orders.len() as u8;
                out.mmr_constant = mmr.constant.into();
                for (dest, source) in out.mmr_orders.iter_mut().zip(&mmr.orders) {
                    for (dest, source) in dest.iter_mut().zip(source) {
                        *dest = (*source).into();
                    }
                }
            }
        }
        out
    }
}

impl ReshapingCurveV2 {
    /// Borrow the ordered piece buffer for this curve; valid for this owner's lifetime.
    #[cfg(test)]
    pub(crate) fn pieces(&self) -> &[MappingPieceV2] {
        // Ownership fields are private and only initialized by try_new.
        unsafe { std::slice::from_raw_parts(self.pieces, self.piece_count) }
    }

    fn try_new(curve: &DoviReshapingCurve) -> Result<Self> {
        let mut pieces = Vec::new();
        pieces
            .try_reserve_exact(curve.pieces.len())
            .map_err(|_| anyhow::anyhow!("Unable to reserve C mapping pieces"))?;
        pieces.extend(curve.pieces.iter().map(MappingPieceV2::from));
        let mut pivots = [0; 9];
        pivots[..curve.pivots.len()].copy_from_slice(&curve.pivots);
        let piece_count = pieces.len();
        Ok(Self {
            num_pivots_minus2: curve.num_pivots_minus2,
            pivots,
            pieces: Box::into_raw(pieces.into_boxed_slice()) as *const MappingPieceV2,
            piece_count,
        })
    }
}

impl RpuDataMappingV2 {
    pub(crate) fn try_new(mapping: &RpuDataMapping, header: &RpuDataHeader) -> Result<Self> {
        mapping.validate_with_header(header)?;
        let curves = [
            ReshapingCurveV2::try_new(&mapping.curves[0])?,
            ReshapingCurveV2::try_new(&mapping.curves[1])?,
            ReshapingCurveV2::try_new(&mapping.curves[2])?,
        ];
        Ok(Self {
            vdr_rpu_id: mapping.vdr_rpu_id,
            mapping_color_space: mapping.mapping_color_space,
            mapping_chroma_format_idc: mapping.mapping_chroma_format_idc,
            num_x_partitions_minus1: mapping.num_x_partitions_minus1,
            num_y_partitions_minus1: mapping.num_y_partitions_minus1,
            curves,
            nlq_method_idc: mapping.nlq_method_idc.map_or(-1, |m| m as i32),
            nlq_num_pivots_minus2: mapping.nlq_num_pivots_minus2.map_or(-1, i32::from),
            nlq_pred_pivot_value: mapping.nlq_pred_pivot_value.unwrap_or([0; 2]),
            nlq: mapping
                .nlq
                .as_ref()
                .map_or(null(), |n| Box::into_raw(Box::new(RpuDataNlq::from(n)))),
        })
    }
}

impl Drop for ReshapingCurveV2 {
    fn drop(&mut self) {
        // Only try_new constructs this owning pointer/count pair from a boxed slice.
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.pieces as *mut MappingPieceV2,
                self.piece_count,
            )));
        }
    }
}

impl Drop for RpuDataMappingV2 {
    fn drop(&mut self) {
        if !self.nlq.is_null() {
            // NLQ is separately boxed and has no nested allocations.
            unsafe {
                drop(Box::from_raw(self.nlq as *mut RpuDataNlq));
            }
        }
    }
}
