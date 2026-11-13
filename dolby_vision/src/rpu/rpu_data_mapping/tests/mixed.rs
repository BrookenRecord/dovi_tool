use super::*;

// Authored wire selectors and distinct payloads independently check association.
fn mixed_bytes(methods: &[WireMethod], floating: bool) -> Result<Vec<u8>> {
    let mut writer = BitstreamIoWriter::with_capacity(4096);
    for _ in 0..3 {
        writer.write_ue(0)?;
    }
    for _ in 0..NUM_COMPONENTS {
        writer.write_ue(methods.len() as u64 - 1)?;
        for i in 0..=methods.len() {
            writer.write_var(10, i as u16)?;
        }
    }
    writer.write_ue(0)?;
    writer.write_ue(0)?;
    for cmp in 0..NUM_COMPONENTS {
        for (i, method) in methods.iter().enumerate() {
            let marker = (100 * cmp + 10 * i + 1) as u64;
            writer.write_ue(*method as u64)?;
            match method {
                WireMethod::Polynomial => {
                    writer.write_ue((i % 2) as u64)?;
                    if i % 2 == 0 {
                        writer.write_bit(false)?;
                    }
                }
                WireMethod::Mmr => {
                    writer.write::<2, u8>((i % 3) as u8)?;
                }
            }
            let mut coefficient = |value: u64| -> Result<()> {
                if !floating {
                    writer.write_se(-(value as i64))?;
                }
                writer.write_var(
                    if floating { 32 } else { 23 },
                    if floating { 0x8000_0000 | value } else { value },
                )?;
                Ok(())
            };
            match method {
                WireMethod::Polynomial => {
                    for j in 0..i % 2 + 2 {
                        coefficient(marker + j as u64)?;
                    }
                }
                WireMethod::Mmr => {
                    coefficient(marker)?;
                    for order in 0..=i % 3 {
                        for j in 0..MMR_MAX_COEFFS {
                            coefficient(marker + 1 + (order * 7 + j) as u64)?;
                        }
                    }
                }
            }
        }
    }
    writer.byte_align()?;
    Ok(writer.into_inner())
}

fn coefficient_matches(c: &DoviCoefficient, marker: u64, floating: bool) {
    assert_eq!(
        c.integer,
        if floating {
            None
        } else {
            Some(-(marker as i64))
        }
    );
    assert_eq!(
        c.value,
        if floating {
            0x8000_0000 | marker
        } else {
            marker
        }
    );
}

#[test]
fn mixed_selectors_own_their_distinct_coefficients() -> Result<()> {
    use WireMethod::{Mmr as MMR, Polynomial as P};
    for methods in [
        vec![P, MMR, P],
        vec![MMR, P, MMR],
        vec![P, MMR, P, MMR, P, MMR, P, MMR],
    ] {
        for floating in [false, true] {
            let mut header = header();
            if floating {
                header.coefficient_data_type = 1;
                header.coefficient_log2_denom = 0;
                header.coefficient_log2_denom_length = 32;
            }
            let bytes = mixed_bytes(&methods, floating)?;
            let mapping = RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header)?;
            for (cmp, curve) in mapping.curves.iter().enumerate() {
                for (i, (piece, method)) in curve.pieces.iter().zip(&methods).enumerate() {
                    let marker = (100 * cmp + 10 * i + 1) as u64;
                    match (method, piece) {
                        (P, DoviMappingPiece::Polynomial(poly)) => {
                            assert_eq!(poly.coefficients.len(), i % 2 + 2);
                            for (j, c) in poly.coefficients.iter().enumerate() {
                                coefficient_matches(c, marker + j as u64, floating);
                            }
                        }
                        (MMR, DoviMappingPiece::MMR(mmr)) => {
                            coefficient_matches(&mmr.constant, marker, floating);
                            assert_eq!(mmr.orders.len(), i % 3 + 1);
                            for (order, values) in mmr.orders.iter().enumerate() {
                                for (j, c) in values.iter().enumerate() {
                                    coefficient_matches(
                                        c,
                                        marker + 1 + (order * 7 + j) as u64,
                                        floating,
                                    );
                                }
                            }
                        }
                        _ => panic!("Method changed between wire and parsed piece"),
                    }
                }
            }
            let mut writer = BitstreamIoWriter::with_capacity(bytes.len());
            mapping.write(&mut writer, &header)?;
            writer.byte_align()?;
            assert_eq!(writer.into_inner(), bytes);
        }
    }
    Ok(())
}

#[test]
fn interpolation_returns_unsupported_error() -> Result<()> {
    // Build a prefix independently to set only the first interpolation flag.
    let mut writer = BitstreamIoWriter::with_capacity(32);
    for _ in 0..3 {
        writer.write_ue(0)?;
    }
    for _ in 0..3 {
        writer.write_ue(0)?;
        writer.write_var(10, 0_u16)?;
        writer.write_var(10, 1023_u16)?;
    }
    writer.write_ue(0)?;
    writer.write_ue(0)?;
    writer.write_ue(0)?;
    writer.write_ue(0)?;
    writer.write_bit(true)?;
    writer.byte_align()?;
    let bytes = writer.into_inner();
    let error =
        RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header()).unwrap_err();
    assert_eq!(error.to_string(), "Unsupported polynomial interpolation");
    Ok(())
}

#[cfg(feature = "serde")]
#[test]
fn serialized_mixed_pieces_have_explicit_tags_and_owned_payloads() -> Result<()> {
    let bytes = mixed_bytes(
        &[
            WireMethod::Polynomial,
            WireMethod::Mmr,
            WireMethod::Polynomial,
        ],
        false,
    )?;
    let mapping = RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header())?;
    let value = serde_json::to_value(&mapping)?;
    let curve = &value["curves"][0];
    assert!(curve.get("mapping_idc").is_none());
    assert!(curve.get("polynomial").is_none());
    assert!(curve.get("mmr").is_none());
    let pieces = &curve["pieces"];
    assert_eq!(pieces[0]["method"], "Polynomial");
    assert_eq!(pieces[1]["method"], "MMR");
    assert_eq!(pieces[2]["method"], "Polynomial");
    assert_eq!(pieces[0]["data"]["coefficients"][0]["integer"], -1);
    assert_eq!(pieces[1]["data"]["constant"]["integer"], -11);
    assert_eq!(pieces[2]["data"]["coefficients"][0]["value"], 21);
    assert!(value.get("nlq").is_none());

    let bytes = mixed_bytes(
        &[
            WireMethod::Polynomial,
            WireMethod::Mmr,
            WireMethod::Polynomial,
        ],
        true,
    )?;
    let floating_header = RpuDataHeader {
        coefficient_data_type: 1,
        coefficient_log2_denom: 0,
        coefficient_log2_denom_length: 32,
        ..header()
    };
    let floating =
        RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &floating_header)?;
    let value = serde_json::to_value(&floating)?;
    assert!(value["curves"][0]["pieces"][0]["data"]["coefficients"][0]["integer"].is_null());
    assert_eq!(
        value["curves"][0]["pieces"][0]["data"]["coefficients"][0]["value"],
        0x8000_0001_u64
    );
    Ok(())
}

#[cfg(feature = "capi")]
#[test]
fn actual_c_getter_retains_mixed_pieces_after_rpu_free() -> Result<()> {
    use crate::{c_structs::RpuOpaque, capi::*};
    let bytes = mixed_bytes(
        &[WireMethod::Mmr, WireMethod::Polynomial, WireMethod::Mmr],
        false,
    )?;
    let mut rpu = DoviRpu::profile81_config(&crate::rpu::generate::GenerateConfig::default())?;
    rpu.rpu_data_mapping = Some(RpuDataMapping::parse(
        &mut BsIoSliceReader::from_slice(&bytes),
        &header(),
    )?);
    let opaque = Box::into_raw(Box::new(RpuOpaque::from(Ok(rpu))));
    unsafe {
        let mapping = dovi_rpu_get_data_mapping_v2(opaque);
        dovi_rpu_free(opaque);
        assert!(!mapping.is_null());
        struct OwnedMapping(*const crate::c_structs::RpuDataMappingV2);
        impl Drop for OwnedMapping {
            fn drop(&mut self) {
                unsafe {
                    dovi_rpu_free_data_mapping_v2(self.0);
                }
            }
        }
        let mapping = OwnedMapping(mapping);
        let pieces = (&*mapping.0).curves[0].pieces();
        assert_eq!(
            pieces.iter().map(|p| p.method).collect::<Vec<_>>(),
            [1, 0, 1]
        );
        assert_eq!(pieces[0].mmr_constant.integer, -1);
        assert_eq!(pieces[1].polynomial[0].integer, -11);
        assert_eq!(pieces[2].mmr_orders[2][6].value, 42);
        drop(mapping);
        dovi_rpu_free_data_mapping_v2(std::ptr::null());
        assert!(dovi_rpu_get_data_mapping_v2(std::ptr::null()).is_null());
    }
    Ok(())
}

#[test]
fn constructors_and_transforms_preserve_owned_piece_contracts() -> Result<()> {
    use crate::rpu::{ConversionMode, generate::GenerateConfig};
    let rpu = DoviRpu::profile84_config(&GenerateConfig::default())?;
    let original = rpu.rpu_data_mapping.as_ref().unwrap();
    assert_eq!(original.curves[0].pieces.len(), 8);
    if let DoviMappingPiece::Polynomial(poly) = &original.curves[0].pieces[0] {
        assert_eq!(
            poly.coefficients[0],
            DoviCoefficient {
                integer: Some(-1),
                value: 7978928
            }
        );
    } else {
        panic!("Expected profile 8.4 polynomial");
    }
    for (cmp, integer, value) in [(1, 1, 1150183), (2, -2, 6266112)] {
        if let DoviMappingPiece::MMR(mmr) = &original.curves[cmp].pieces[0] {
            assert_eq!(mmr.orders.len(), 3);
            assert_eq!(
                mmr.constant,
                DoviCoefficient {
                    integer: Some(integer),
                    value
                }
            );
        } else {
            panic!("Expected profile 8.4 MMR");
        }
    }
    let encoded = rpu.write_rpu()?;
    assert_eq!(
        DoviRpu::parse_rpu(&encoded)?
            .rpu_data_mapping
            .unwrap()
            .curves,
        original.curves
    );

    let mut rpu = DoviRpu::profile81_config(&GenerateConfig::default())?;
    let bytes = mixed_bytes(
        &[
            WireMethod::Polynomial,
            WireMethod::Mmr,
            WireMethod::Polynomial,
        ],
        false,
    )?;
    let mapping = RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header())?;
    let curves = mapping.curves.clone();
    rpu.rpu_data_mapping = Some(mapping);
    rpu.convert_with_mode(ConversionMode::ToMel)?;
    assert_eq!(rpu.rpu_data_mapping.as_ref().unwrap().curves, curves);
    rpu.convert_with_mode(ConversionMode::To81MappingPreserved)?;
    assert_eq!(rpu.rpu_data_mapping.as_ref().unwrap().curves, curves);
    rpu.remove_mapping();
    for curve in &rpu.rpu_data_mapping.as_ref().unwrap().curves {
        assert_eq!(curve.pivots, [0, 1023]);
        assert_eq!(
            curve.pieces,
            [DoviMappingPiece::Polynomial(
                DoviPolynomialPiece::p81_default()
            )]
        );
    }
    Ok(())
}

#[test]
fn invalid_last_mmr_coefficient_refuses_before_output() -> Result<()> {
    let bytes = mixed_bytes(
        &[WireMethod::Mmr, WireMethod::Polynomial, WireMethod::Mmr],
        false,
    )?;
    let good = RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header())?;
    for coefficient in [
        DoviCoefficient {
            integer: Some(i64::MIN),
            value: 0,
        },
        DoviCoefficient {
            integer: Some(0),
            value: 1 << 23,
        },
        DoviCoefficient {
            integer: None,
            value: 0,
        },
    ] {
        let mut invalid = good.clone();
        if let DoviMappingPiece::MMR(mmr) = &mut invalid.curves[2].pieces[2] {
            mmr.orders[2][6] = coefficient;
        }
        let mut writer = BitstreamIoWriter::with_capacity(256);
        assert!(invalid.write(&mut writer, &header()).is_err());
        assert!(writer.into_inner().is_empty());
    }
    Ok(())
}
