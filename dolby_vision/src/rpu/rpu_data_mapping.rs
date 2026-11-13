//! Bounded, ordered reshaping pieces. Pivots retain their coded delta representation.

mod mapping;
mod piece;
mod types;
mod validation;

pub use types::{
    DoviCoefficient, DoviMappingPiece, DoviMmrPiece, DoviNlqMethod, DoviPolynomialPiece,
    DoviReshapingCurve, RpuDataMapping,
};

#[cfg(test)]
mod tests;
