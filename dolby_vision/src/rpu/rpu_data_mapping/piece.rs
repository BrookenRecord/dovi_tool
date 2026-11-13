use crate::rpu::bounded_error::{BoundedResult, bbail, bensure};
use anyhow::Result;
use bitvec_helpers::{
    bitstream_io_reader::BsIoSliceReader, bitstream_io_writer::BitstreamIoWriter,
};
use tinyvec::array_vec;

use super::{DoviCoefficient, DoviMappingPiece, DoviMmrPiece, DoviPolynomialPiece};
use crate::rpu::{MMR_MAX_COEFFS, rpu_data_header::RpuDataHeader};

impl DoviCoefficient {
    fn parse(reader: &mut BsIoSliceReader, header: &RpuDataHeader) -> BoundedResult<Self> {
        Ok(Self {
            integer: if header.coefficient_data_type == 0 {
                Some(reader.read_se()?)
            } else {
                None
            },
            value: reader.read_var(header.coefficient_log2_denom_length)?,
        })
    }

    pub(crate) fn validate(&self, header: &RpuDataHeader) -> BoundedResult<()> {
        bensure!(
            self.integer.is_some() == (header.coefficient_data_type == 0),
            "Invalid coefficient encoding"
        );
        bensure!(
            self.integer != Some(i64::MIN),
            "Coefficient integer is not writable"
        );
        bensure!(
            self.value < (1_u64 << header.coefficient_log2_denom_length),
            "Coefficient value exceeds width"
        );
        Ok(())
    }

    fn write(&self, writer: &mut BitstreamIoWriter, header: &RpuDataHeader) -> Result<()> {
        if let Some(integer) = self.integer {
            writer.write_se(integer)?;
        }
        writer.write_var(header.coefficient_log2_denom_length, self.value)?;
        Ok(())
    }
}

impl DoviMappingPiece {
    pub(crate) fn parse(
        reader: &mut BsIoSliceReader,
        header: &RpuDataHeader,
    ) -> BoundedResult<Self> {
        match reader.read_ue()? {
            0 => {
                let order_minus1 = reader.read_ue()?;
                bensure!(order_minus1 <= 1, "Invalid polynomial order");
                if order_minus1 == 0 {
                    bensure!(
                        !reader.read_bit()?,
                        Unsupported,
                        "Unsupported polynomial interpolation"
                    );
                }
                let mut coefficients = array_vec!();
                for _ in 0..order_minus1 + 2 {
                    coefficients.push(DoviCoefficient::parse(reader, header)?);
                }
                Ok(Self::Polynomial(DoviPolynomialPiece { coefficients }))
            }
            1 => {
                let order_minus1 = reader.read::<2, u8>()?;
                bensure!(order_minus1 <= 2, "Invalid MMR order");
                let constant = DoviCoefficient::parse(reader, header)?;
                let mut orders = array_vec!();
                for _ in 0..=order_minus1 {
                    let mut order = [DoviCoefficient::default(); MMR_MAX_COEFFS];
                    for coefficient in &mut order {
                        *coefficient = DoviCoefficient::parse(reader, header)?;
                    }
                    orders.push(order);
                }
                Ok(Self::MMR(DoviMmrPiece { constant, orders }))
            }
            _ => bbail!("Invalid mapping method"),
        }
    }

    pub(crate) fn validate_shape(&self) -> BoundedResult<()> {
        match self {
            Self::Polynomial(piece) => bensure!(
                (2..=3).contains(&piece.coefficients.len()),
                "Invalid polynomial coefficient count"
            ),
            Self::MMR(piece) => bensure!(
                (1..=3).contains(&piece.orders.len()),
                "Invalid MMR order count"
            ),
        }
        Ok(())
    }

    pub(crate) fn validate(&self, header: &RpuDataHeader) -> BoundedResult<()> {
        self.validate_shape()?;
        match self {
            Self::Polynomial(piece) => {
                for coefficient in &piece.coefficients {
                    coefficient.validate(header)?;
                }
            }
            Self::MMR(piece) => {
                piece.constant.validate(header)?;
                for order in &piece.orders {
                    for coefficient in order {
                        coefficient.validate(header)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn write(
        &self,
        writer: &mut BitstreamIoWriter,
        header: &RpuDataHeader,
    ) -> Result<()> {
        match self {
            Self::Polynomial(piece) => {
                writer.write_ue(0)?;
                let order_minus1 = piece.coefficients.len() - 2;
                writer.write_ue(order_minus1 as u64)?;
                if order_minus1 == 0 {
                    writer.write_bit(false)?;
                }
                for coefficient in &piece.coefficients {
                    coefficient.write(writer, header)?;
                }
            }
            Self::MMR(piece) => {
                writer.write_ue(1)?;
                writer.write::<2, u8>((piece.orders.len() - 1) as u8)?;
                piece.constant.write(writer, header)?;
                for order in &piece.orders {
                    for coefficient in order {
                        coefficient.write(writer, header)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl DoviPolynomialPiece {
    /// Identity degree-1 polynomial for the default fixed-point profile 8.1 header.
    pub fn p81_default() -> Self {
        Self {
            coefficients: array_vec!(
                DoviCoefficient {
                    integer: Some(0),
                    value: 0
                },
                DoviCoefficient {
                    integer: Some(1),
                    value: 0
                }
            ),
        }
    }
}
