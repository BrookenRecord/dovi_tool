use super::*;
use crate::rpu::{profiles::profile81::Profile81, rpu_data_nlq::RpuDataNlq};
use tinyvec::ArrayVec;

fn refuses_without_writing(mapping: &RpuDataMapping, header: &RpuDataHeader) -> Result<()> {
    let mut writer = BitstreamIoWriter::with_capacity(256);
    writer.write::<8, u8>(0xa5)?;
    assert!(mapping.write(&mut writer, header).is_err());
    assert_eq!(writer.into_inner(), [0xa5]);
    Ok(())
}

#[test]
fn malformed_caller_values_refuse_before_first_mapping_bit() -> Result<()> {
    let good = Profile81::rpu_data_mapping();
    let mut invalid = Vec::new();
    let mut m = good.clone();
    m.vdr_rpu_id = 16;
    invalid.push(m);
    let mut m = good.clone();
    m.mapping_color_space = 1;
    invalid.push(m);
    let mut m = good.clone();
    m.mapping_chroma_format_idc = 1;
    invalid.push(m);
    let mut m = good.clone();
    m.curves[2].num_pivots_minus2 = u64::MAX;
    invalid.push(m);
    let mut m = good.clone();
    m.curves[2].pivots.pop();
    invalid.push(m);
    let mut m = good.clone();
    m.curves[2].pivots[1] = 1024;
    invalid.push(m);
    let mut m = good.clone();
    m.curves[2].pieces.clear();
    invalid.push(m);
    let mut m = good.clone();
    let piece = m.curves[0].pieces[0].clone();
    m.curves[2].pieces.push(piece);
    invalid.push(m);
    let single: ArrayVec<[DoviCoefficient; 3]> =
        std::iter::once(DoviCoefficient::default()).collect();
    for coefficients in [ArrayVec::default(), single] {
        let mut m = good.clone();
        m.curves[2].pieces[0] = DoviMappingPiece::Polynomial(DoviPolynomialPiece { coefficients });
        invalid.push(m);
    }
    let mut m = good.clone();
    m.curves[2].pieces[0] = DoviMappingPiece::MMR(DoviMmrPiece::default());
    invalid.push(m);
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
        let mut m = good.clone();
        if let DoviMappingPiece::Polynomial(poly) = &mut m.curves[2].pieces[0] {
            poly.coefficients[0] = coefficient;
        }
        invalid.push(m);
    }
    let mut m = good.clone();
    m.nlq = Some(RpuDataNlq::default());
    invalid.push(m);
    let mut m = good.clone();
    m.nlq_method_idc = Some(DoviNlqMethod::LinearDeadzone);
    invalid.push(m);
    for mapping in invalid {
        refuses_without_writing(&mapping, &header())?;
    }
    Ok(())
}

#[test]
fn invalid_headers_refuse_before_mapping_output() -> Result<()> {
    let mapping = Profile81::rpu_data_mapping();
    for header in [
        RpuDataHeader {
            rpu_type: 0,
            ..header()
        },
        RpuDataHeader {
            rpu_format: 0x800,
            ..header()
        },
        RpuDataHeader {
            rpu_format: 0x100,
            ..header()
        },
        RpuDataHeader {
            vdr_rpu_profile: 16,
            ..header()
        },
        RpuDataHeader {
            vdr_rpu_normalized_idc: 4,
            ..header()
        },
        RpuDataHeader {
            reserved_zero_3bits: 8,
            ..header()
        },
        RpuDataHeader {
            ext_mapping_idc_0_4: 32,
            ..header()
        },
        RpuDataHeader {
            ext_mapping_idc_5_7: 8,
            ..header()
        },
        RpuDataHeader {
            use_prev_vdr_rpu_flag: true,
            ..header()
        },
        RpuDataHeader {
            vdr_seq_info_present_flag: false,
            ..header()
        },
        RpuDataHeader {
            coefficient_log2_denom_length: 64,
            ..header()
        },
        RpuDataHeader {
            coefficient_data_type: 1,
            coefficient_log2_denom_length: 32,
            ..header()
        },
        RpuDataHeader {
            el_bit_depth_minus8: u64::MAX,
            ..header()
        },
    ] {
        refuses_without_writing(&mapping, &header)?;
    }
    Ok(())
}

#[test]
fn nlq_presence_and_widths_are_preflighted() -> Result<()> {
    assert_eq!(DoviNlqMethod::from(0), DoviNlqMethod::LinearDeadzone);
    let mut rpu = DoviRpu::profile81_config(&crate::rpu::generate::GenerateConfig::default())?;
    rpu.convert_with_mode(1)?;
    let good = rpu.rpu_data_mapping.as_ref().unwrap();
    good.validate_with_header(&rpu.header)?;
    let mut invalid = Vec::new();
    let mut m = good.clone();
    m.nlq = None;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq_method_idc = None;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq_pred_pivot_value = None;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq_num_pivots_minus2 = Some(1);
    invalid.push(m);
    let mut m = good.clone();
    m.nlq_pred_pivot_value = Some([0, 1024]);
    invalid.push(m);
    let mut m = good.clone();
    m.nlq.as_mut().unwrap().nlq_offset[2] = 1024;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq.as_mut().unwrap().vdr_in_max[2] = 1 << 23;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq.as_mut().unwrap().linear_deadzone_slope[2] = 1 << 23;
    invalid.push(m);
    let mut m = good.clone();
    m.nlq.as_mut().unwrap().linear_deadzone_threshold[2] = 1 << 23;
    invalid.push(m);
    for mapping in invalid {
        refuses_without_writing(&mapping, &rpu.header)?;
    }
    Ok(())
}

#[test]
fn signed_endpoints_and_full_unsigned_fields_roundtrip() -> Result<()> {
    // Run only with the reviewed bit-reader/writer repair; the unfixed helper overflows here.
    let mut mapping = Profile81::rpu_data_mapping();
    mapping.num_x_partitions_minus1 = u64::MAX;
    mapping.num_y_partitions_minus1 = u64::MAX;
    if let DoviMappingPiece::Polynomial(poly) = &mut mapping.curves[0].pieces[0] {
        poly.coefficients[0].integer = Some(-i64::MAX);
        poly.coefficients[1].integer = Some(i64::MAX);
    }
    let mut writer = BitstreamIoWriter::with_capacity(256);
    mapping.write(&mut writer, &header())?;
    writer.byte_align()?;
    let bytes = writer.into_inner();
    let parsed = RpuDataMapping::parse(&mut BsIoSliceReader::from_slice(&bytes), &header())?;
    assert_eq!(parsed.num_x_partitions_minus1, u64::MAX);
    assert_eq!(parsed.num_y_partitions_minus1, u64::MAX);
    assert_eq!(parsed.curves, mapping.curves);
    Ok(())
}
