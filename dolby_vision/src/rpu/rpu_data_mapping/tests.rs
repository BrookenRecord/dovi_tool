use super::*;
use crate::rpu::dovi_rpu::DoviRpu;
use crate::rpu::{MMR_MAX_COEFFS, NUM_COMPONENTS, rpu_data_header::RpuDataHeader};
use anyhow::Result;
use bitvec_helpers::{
    bitstream_io_reader::BsIoSliceReader, bitstream_io_writer::BitstreamIoWriter,
};

fn header() -> RpuDataHeader {
    RpuDataHeader::p8_default()
}

// Independent wire tags; tests do not derive selectors from the production enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WireMethod {
    Polynomial = 0,
    Mmr = 1,
}

fn count_only(count: u64) -> Result<Vec<u8>> {
    let mut writer = BitstreamIoWriter::with_capacity(32);
    for _ in 0..3 {
        writer.write_ue(0)?;
    }
    writer.write_ue(count)?;
    writer.byte_align()?;
    Ok(writer.into_inner())
}

fn mapping_bytes(pivots: u64, method: WireMethod, order: u8) -> Result<Vec<u8>> {
    let mut writer = BitstreamIoWriter::with_capacity(2048);
    for _ in 0..3 {
        writer.write_ue(0)?;
    }
    for _ in 0..NUM_COMPONENTS {
        writer.write_ue(pivots - 2)?;
        for i in 0..pivots {
            writer.write_var(10, i as u16)?;
        }
    }
    writer.write_ue(0)?;
    writer.write_ue(0)?;

    for _ in 0..NUM_COMPONENTS {
        for _ in 0..pivots - 1 {
            writer.write_ue(method as u64)?;
            match method {
                WireMethod::Polynomial => {
                    writer.write_ue(order as u64)?;
                    if order == 0 {
                        writer.write_bit(false)?;
                    }
                    if order <= 1 {
                        for i in 0..order + 2 {
                            writer.write_se(-(i as i64))?;
                            writer.write_var(23, i as u64)?;
                        }
                    }
                }
                WireMethod::Mmr => {
                    writer.write::<2, u8>(order)?;
                    if order <= 2 {
                        writer.write_se(-1)?;
                        writer.write_var(23, 1_u64)?;
                        for _ in 0..=order {
                            for i in 0..MMR_MAX_COEFFS {
                                writer.write_se(-(i as i64))?;
                                writer.write_var(23, i as u64)?;
                            }
                        }
                    }
                }
            }
        }
    }
    writer.byte_align()?;
    Ok(writer.into_inner())
}

#[test]
fn invalid_pivot_count_is_rejected_before_payload_read() -> Result<()> {
    // Only ten pivots would be allocated by the old parser, then it would hit EOF.
    let bytes = count_only(8)?;
    let error =
        RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).unwrap_err();
    assert_eq!(error.to_string(), "num_pivots_minus2 must be at most 7");
    Ok(())
}

#[test]
fn large_pivot_count_is_rejected_before_conversion_or_allocation() -> Result<()> {
    let bytes = count_only(1_u64 << 40)?;
    let error =
        RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).unwrap_err();
    assert_eq!(error.to_string(), "num_pivots_minus2 must be at most 7");
    Ok(())
}

#[test]
fn homogeneous_mapping_boundaries_roundtrip() -> Result<()> {
    for pivots in [2, 9] {
        for (method, max_order) in [(WireMethod::Polynomial, 1), (WireMethod::Mmr, 2)] {
            for order in 0..=max_order {
                let bytes = mapping_bytes(pivots, method, order)?;
                let mapping =
                    RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header())?;
                for curve in &mapping.curves {
                    assert_eq!(curve.pivots.len(), pivots as usize);
                    assert_eq!(curve.pieces.len(), pivots as usize - 1);
                    for piece in &curve.pieces {
                        match (method, piece) {
                            (WireMethod::Polynomial, DoviMappingPiece::Polynomial(poly)) => {
                                assert_eq!(poly.coefficients.len(), order as usize + 2);
                            }
                            (WireMethod::Mmr, DoviMappingPiece::MMR(mmr)) => {
                                assert_eq!(mmr.orders.len(), order as usize + 1);
                            }
                            _ => panic!("Wrong method or coefficient association"),
                        }
                    }
                }
                let mut writer = BitstreamIoWriter::with_capacity(bytes.len());
                mapping.write(&mut writer, &header())?;
                writer.byte_align()?;
                assert_eq!(writer.into_inner(), bytes);
            }
        }
    }
    Ok(())
}

#[test]
fn valid_count_with_truncated_pivots_returns_error() -> Result<()> {
    let bytes = count_only(7)?;
    let error =
        RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).unwrap_err();
    assert_ne!(error.to_string(), "num_pivots_minus2 must be at most 7");
    Ok(())
}

#[test]
fn truncated_coefficients_return_error() -> Result<()> {
    for method in [WireMethod::Polynomial, WireMethod::Mmr] {
        let mut bytes = mapping_bytes(2, method, 1)?;
        bytes.pop();
        assert!(
            RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).is_err()
        );
    }
    Ok(())
}

#[test]
fn invalid_orders_are_rejected_before_coefficients() -> Result<()> {
    for (method, order, expected) in [
        (WireMethod::Polynomial, 2, "Invalid polynomial order"),
        (WireMethod::Mmr, 3, "Invalid MMR order"),
    ] {
        let bytes = mapping_bytes(2, method, order)?;
        let error =
            RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    Ok(())
}

#[test]
fn invalid_widths_are_rejected_before_pivots() -> Result<()> {
    let bytes = count_only(0)?;
    for invalid in [
        RpuDataHeader {
            bl_bit_depth_minus8: u64::MAX,
            ..header()
        },
        RpuDataHeader {
            coefficient_log2_denom: 24,
            coefficient_log2_denom_length: 24,
            ..header()
        },
        RpuDataHeader {
            coefficient_log2_denom_length: 64,
            ..header()
        },
        RpuDataHeader {
            coefficient_data_type: 1,
            coefficient_log2_denom_length: 33,
            ..header()
        },
        RpuDataHeader {
            coefficient_data_type: 2,
            ..header()
        },
    ] {
        let error =
            RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &invalid).unwrap_err();
        assert!(error.to_string().starts_with("Invalid mapping"), "{error}");
    }
    Ok(())
}

#[test]
fn existing_homogeneous_fixtures_roundtrip() -> Result<()> {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("assets/tests");
    for file in [
        "profile5.bin",
        "profile8.bin",
        "fel_orig.bin",
        "poly_coef_int_logic.bin",
        "profile84.bin",
    ] {
        let bytes = std::fs::read(assets.join(file))?;
        let rpu = DoviRpu::parse_unspec62_nalu(&bytes)?;
        assert_eq!(rpu.write_hevc_unspec62_nalu()?[2..], bytes[4..]);
    }
    Ok(())
}

mod mixed;
mod preflight;
