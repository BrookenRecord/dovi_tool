use tinyvec::ArrayVec;

#[cfg(feature = "serde")]
use serde::Serialize;

use crate::rpu::{MMR_MAX_COEFFS, NLQ_NUM_PIVOTS, NUM_COMPONENTS, rpu_data_nlq::RpuDataNlq};

/// Encoded coefficient; no floating-point conversion is performed.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct DoviCoefficient {
    /// Signed integer for coefficient_data_type 0; absent for type 1.
    /// The writable signed domain is [-i64::MAX, i64::MAX].
    pub integer: Option<i64>,
    /// Fractional bits for type 0, or the unchanged 32-bit encoding for type 1.
    pub value: u64,
}

/// One supported polynomial piece. Interpolation syntax is explicitly unsupported.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct DoviPolynomialPiece {
    /// Two coefficients for degree 1, or three for degree 2, in wire order.
    pub coefficients: ArrayVec<[DoviCoefficient; 3]>,
}

/// One MMR piece with inline, method-associated coefficients.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct DoviMmrPiece {
    /// Constant term, before all order-dependent coefficients.
    pub constant: DoviCoefficient,
    /// One to three orders, each containing exactly seven coefficients in wire order.
    pub orders: ArrayVec<[[DoviCoefficient; MMR_MAX_COEFFS]; 3]>,
}

/// Method and coefficients for one interval between consecutive coded pivots.
/// JSON uses explicit `method` and `data` fields; no legacy shape inference occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "method", content = "data"))]
// Inline payloads keep parsing to one fallibly reserved buffer per curve;
// boxing the MMR variant would add an allocation for each MMR piece.
#[allow(clippy::large_enum_variant)]
pub enum DoviMappingPiece {
    /// Degree 1 or 2 polynomial with its own coefficients.
    Polynomial(DoviPolynomialPiece),
    /// Order 1, 2 or 3 MMR with its own constant and coefficients.
    MMR(DoviMmrPiece),
}

/// Supported nonlinear quantization method.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum DoviNlqMethod {
    /// Linear deadzone syntax, encoded as method 0.
    LinearDeadzone = 0,
}

/// A component's ordered reshaping intervals, preserving encoded pivot deltas.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct DoviReshapingCurve {
    /// Wire pivot count minus two, in 0..=7; must agree with pivots.len().
    pub num_pivots_minus2: u64,
    /// Two to nine coded pivot values, not cumulative absolute coordinates.
    pub pivots: Vec<u16>,
    /// Exactly pivots.len()-1 pieces, in wire order; methods may differ per piece.
    pub pieces: Vec<DoviMappingPiece>,
}

/// Mapping update for three components, with optional dual-layer NLQ syntax.
/// Validate with the header before writing or exporting caller-constructed values.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct RpuDataMapping {
    /// Mapping identifier in 0..=15.
    pub vdr_rpu_id: u64,
    /// Supported mapping color space, currently 0.
    pub mapping_color_space: u64,
    /// Supported mapping chroma format, currently 0.
    pub mapping_chroma_format_idc: u64,
    /// Horizontal partition count minus one, encoded as UE.
    pub num_x_partitions_minus1: u64,
    /// Vertical partition count minus one, encoded as UE.
    pub num_y_partitions_minus1: u64,
    /// Luma and two chroma curves, each with independently ordered pieces.
    pub curves: [DoviReshapingCurve; NUM_COMPONENTS],
    /// Present exactly when the header enables dual-layer NLQ.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub nlq_method_idc: Option<DoviNlqMethod>,
    /// Present with NLQ; the supported value is 0.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub nlq_num_pivots_minus2: Option<u8>,
    /// Two coded NLQ pivots, present with NLQ.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub nlq_pred_pivot_value: Option<[u16; NLQ_NUM_PIVOTS]>,
    /// NLQ coefficients, present with NLQ.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub nlq: Option<RpuDataNlq>,
}

impl From<u8> for DoviNlqMethod {
    /// Convert encoded method 0. As in the prior API, other values panic.
    /// Bitstream parsing checks the encoded value before constructing this method.
    fn from(value: u8) -> Self {
        match value {
            0 => Self::LinearDeadzone,
            _ => unreachable!(),
        }
    }
}
