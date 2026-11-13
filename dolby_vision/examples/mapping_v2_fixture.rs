//! Write an authored mixed-method RPU for the mapping_v2_test.c consumer.
use anyhow::{Context, Result};
use dolby_vision::rpu::{
    dovi_rpu::DoviRpu,
    generate::GenerateConfig,
    rpu_data_mapping::{DoviCoefficient, DoviMappingPiece, DoviMmrPiece, DoviPolynomialPiece},
};
use tinyvec::array_vec;

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("Usage: mapping_v2_fixture OUTPUT")?;
    let mut rpu = DoviRpu::profile81_config(&GenerateConfig::default())?;
    let mapping = rpu
        .rpu_data_mapping
        .as_mut()
        .context("Missing generated mapping")?;
    for (cmp, curve) in mapping.curves.iter_mut().enumerate() {
        curve.num_pivots_minus2 = 2;
        curve.pivots = vec![0, 200, 300, 523];
        curve.pieces = (0..3)
            .map(|i| {
                let marker = (100 * cmp + 10 * i + 1) as u64;
                let coefficient = |value| DoviCoefficient {
                    integer: Some(-(value as i64)),
                    value,
                };
                if (cmp + i) % 2 == 0 {
                    DoviMappingPiece::Polynomial(DoviPolynomialPiece {
                        coefficients: array_vec!(coefficient(marker), coefficient(marker + 1)),
                    })
                } else {
                    DoviMappingPiece::MMR(DoviMmrPiece {
                        constant: coefficient(marker),
                        orders: (0..=i % 3)
                            .map(|order| {
                                std::array::from_fn(|j| {
                                    coefficient(marker + 1 + (order * 7 + j) as u64)
                                })
                            })
                            .collect(),
                    })
                }
            })
            .collect();
    }
    std::fs::write(path, rpu.write_rpu()?)?;
    Ok(())
}
