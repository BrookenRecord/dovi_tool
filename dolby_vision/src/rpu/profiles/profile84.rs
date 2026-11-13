use tinyvec::{ArrayVec, array_vec};

use crate::rpu::{
    NUM_COMPONENTS,
    rpu_data_mapping::{
        DoviCoefficient, DoviMappingPiece, DoviMmrPiece, DoviPolynomialPiece, DoviReshapingCurve,
        RpuDataMapping,
    },
};

use super::{DoviProfile, VdrDmData, profile81::Profile81};

pub struct Profile84 {}

impl DoviProfile for Profile84 {
    fn dm_data() -> VdrDmData {
        VdrDmData {
            source_min_pq: 62,
            source_max_pq: 3079,
            ..Profile81::dm_data()
        }
    }
}

// Based on iPhone 13 polynomials and MMR
impl Profile84 {
    pub fn rpu_data_mapping() -> RpuDataMapping {
        // Luma component
        let poly_coef_int: Vec<ArrayVec<[i64; 3]>> = vec![
            array_vec!(-1, 1, -3),
            array_vec!(-1, 1, -2),
            array_vec!(0, 0, -1),
            array_vec!(0, 0, 0),
            array_vec!(0, -2, 1),
            array_vec!(6, -14, 8),
            array_vec!(13, -30, 16),
            array_vec!(28, -62, 34),
        ];

        let poly_coef: Vec<ArrayVec<[u64; 3]>> = vec![
            array_vec!(7978928, 8332855, 4889184),
            array_vec!(8269552, 5186604, 3909327),
            array_vec!(1317527, 5338528, 7440486),
            array_vec!(2119979, 2065496, 2288524),
            array_vec!(7982780, 5409990, 1585336),
            array_vec!(3460436, 3197328, 615464),
            array_vec!(3921968, 6820672, 5546752),
            array_vec!(1947392, 1244640, 6094272),
        ];

        let pieces = poly_coef_int
            .into_iter()
            .zip(poly_coef)
            .map(|(ints, values)| {
                DoviMappingPiece::Polynomial(DoviPolynomialPiece {
                    coefficients: ints
                        .into_iter()
                        .zip(values)
                        .map(|(integer, value)| DoviCoefficient {
                            integer: Some(integer),
                            value,
                        })
                        .collect(),
                })
            })
            .collect();
        let luma_reshaping_curve = DoviReshapingCurve {
            num_pivots_minus2: 7,
            pivots: vec![63, 69, 230, 256, 256, 37, 16, 8, 7],
            pieces,
        };

        // Chroma component 1
        let mmr_coef_int_cmp1: Vec<ArrayVec<[ArrayVec<[i64; 7]>; 3]>> = vec![array_vec!(
            array_vec!(-1, -2, -5, 2, 5, 9, -12),
            array_vec!(-1, -1, 3, -1, -5, -12, 18),
            array_vec!(-1, 0, -2, 0, 2, 7, -19)
        )];
        let mmr_coef_cmp1: Vec<ArrayVec<[ArrayVec<[u64; 7]>; 3]>> = vec![array_vec!(
            array_vec!(87355, 6228986, 642500, 1023296, 6569512, 5128216, 4317296),
            array_vec!(
                8299905, 5819931, 2324124, 7273546, 1562484, 3679480, 6357360
            ),
            array_vec!(8172981, 3261951, 5970055, 927142, 3525840, 5110348, 6236848)
        )];
        let piece = DoviMmrPiece {
            constant: DoviCoefficient {
                integer: Some(1),
                value: 1150183,
            },
            orders: mmr_coef_int_cmp1[0]
                .iter()
                .zip(mmr_coef_cmp1[0].iter())
                .map(|(ints, values)| {
                    std::array::from_fn(|i| DoviCoefficient {
                        integer: Some(ints[i]),
                        value: values[i],
                    })
                })
                .collect(),
        };
        let chroma_reshaping_curve1 = DoviReshapingCurve {
            num_pivots_minus2: 0,
            pivots: vec![0, 1023],
            pieces: vec![DoviMappingPiece::MMR(piece)],
        };

        // Chroma component 2
        let mmr_coef_int_cmp2: Vec<ArrayVec<[ArrayVec<[i64; 7]>; 3]>> = vec![array_vec!(
            array_vec!(4, 0, 5, -2, -8, -1, 1),
            array_vec!(-4, -1, -6, 1, 12, 0, -4),
            array_vec!(1, 0, 2, -1, -8, -1, 4)
        )];
        let mmr_coef_cmp2: Vec<ArrayVec<[ArrayVec<[u64; 7]>; 3]>> = vec![array_vec!(
            array_vec!(193104, 5369128, 2553116, 8009648, 2772020, 3122453, 2961581),
            array_vec!(6769788, 2565605, 7864496, 4777288, 649616, 7036536, 1666406),
            array_vec!(406265, 2901521, 2680224, 146340, 1008052, 4366810, 5080852)
        )];
        let piece = DoviMmrPiece {
            constant: DoviCoefficient {
                integer: Some(-2),
                value: 6266112,
            },
            orders: mmr_coef_int_cmp2[0]
                .iter()
                .zip(mmr_coef_cmp2[0].iter())
                .map(|(ints, values)| {
                    std::array::from_fn(|i| DoviCoefficient {
                        integer: Some(ints[i]),
                        value: values[i],
                    })
                })
                .collect(),
        };
        let chroma_reshaping_curve2 = DoviReshapingCurve {
            num_pivots_minus2: 0,
            pivots: vec![0, 1023],
            pieces: vec![DoviMappingPiece::MMR(piece)],
        };

        let curves: [DoviReshapingCurve; NUM_COMPONENTS] = [
            luma_reshaping_curve,
            chroma_reshaping_curve1,
            chroma_reshaping_curve2,
        ];

        RpuDataMapping {
            vdr_rpu_id: 0,
            mapping_color_space: 0,
            mapping_chroma_format_idc: 0,
            nlq_method_idc: None,
            nlq_num_pivots_minus2: None,
            nlq_pred_pivot_value: None,
            num_x_partitions_minus1: 0,
            num_y_partitions_minus1: 0,
            curves,
            nlq: None,
        }
    }
}
