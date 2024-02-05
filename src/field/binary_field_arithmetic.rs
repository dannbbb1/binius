// Copyright 2023-2024 Ulvetanna Inc.

use super::binary_field::*;
use cfg_if::cfg_if;
use subtle::{ConstantTimeEq, CtOption};

pub(super) trait TowerFieldArithmetic: TowerField {
	fn multiply(self, rhs: Self) -> Self;

	fn multiply_alpha(self) -> Self;

	fn square(self) -> Self;

	fn invert(self) -> CtOption<Self>;
}

macro_rules! binary_tower_unary_arithmetic_recursive {
	($name:ident) => {
		impl TowerFieldArithmetic for $name {
			fn multiply(self, rhs: Self) -> Self {
				multiply(self, rhs)
			}

			fn multiply_alpha(self) -> Self {
				multiply_alpha(self)
			}

			fn square(self) -> Self {
				square(self)
			}

			fn invert(self) -> CtOption<Self> {
				invert(self)
			}
		}
	};
}

impl TowerField for BinaryField1b {}

impl TowerFieldArithmetic for BinaryField1b {
	fn multiply(self, rhs: Self) -> Self {
		Self(self.0 & rhs.0)
	}

	fn multiply_alpha(self) -> Self {
		self
	}

	fn square(self) -> Self {
		self
	}

	fn invert(self) -> CtOption<Self> {
		CtOption::new(self, self.into())
	}
}

fn mul_bin_4b(a: u8, b: u8) -> u8 {
	#[rustfmt::skip]
	const MUL_4B_LOOKUP: [u8; 128] = [
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
		0x20, 0x13, 0xa8, 0x9b, 0xec, 0xdf, 0x64, 0x57,
		0x30, 0x21, 0xfc, 0xed, 0x74, 0x65, 0xb8, 0xa9,
		0x40, 0xc8, 0xd9, 0x51, 0xae, 0x26, 0x37, 0xbf,
		0x50, 0xfa, 0x8d, 0x27, 0x36, 0x9c, 0xeb, 0x41,
		0x60, 0xdb, 0x71, 0xca, 0x42, 0xf9, 0x53, 0xe8,
		0x70, 0xe9, 0x25, 0xbc, 0xda, 0x43, 0x8f, 0x16,
		0x80, 0x4c, 0x6e, 0xa2, 0xf7, 0x3b, 0x19, 0xd5,
		0x90, 0x7e, 0x3a, 0xd4, 0x6f, 0x81, 0xc5, 0x2b,
		0xa0, 0x5f, 0xc6, 0x39, 0x1b, 0xe4, 0x7d, 0x82,
		0xb0, 0x6d, 0x92, 0x4f, 0x83, 0x5e, 0xa1, 0x7c,
		0xc0, 0x84, 0xb7, 0xf3, 0x59, 0x1d, 0x2e, 0x6a,
		0xd0, 0xb6, 0xe3, 0x85, 0xc1, 0xa7, 0xf2, 0x94,
		0xe0, 0x97, 0x1f, 0x68, 0xb5, 0xc2, 0x4a, 0x3d,
		0xf0, 0xa5, 0x4b, 0x1e, 0x2d, 0x78, 0x96, 0xc3,
	];
	let idx = a << 4 | b;
	(MUL_4B_LOOKUP[idx as usize >> 1] >> ((idx & 1) * 4)) & 0x0f
}

#[rustfmt::skip]
const INVERSE_8B: [u8; 256] = [
	0x00, 0x01, 0x03, 0x02, 0x06, 0x0e, 0x04, 0x0f,
	0x0d, 0x0a, 0x09, 0x0c, 0x0b, 0x08, 0x05, 0x07,
	0x14, 0x67, 0x94, 0x7b, 0x10, 0x66, 0x9e, 0x7e,
	0xd2, 0x81, 0x27, 0x4b, 0xd1, 0x8f, 0x2f, 0x42,
	0x3c, 0xe6, 0xde, 0x7c, 0xb3, 0xc1, 0x4a, 0x1a,
	0x30, 0xe9, 0xdd, 0x79, 0xb1, 0xc6, 0x43, 0x1e,
	0x28, 0xe8, 0x9d, 0xb9, 0x63, 0x39, 0x8d, 0xc2,
	0x62, 0x35, 0x83, 0xc5, 0x20, 0xe7, 0x97, 0xbb,
	0x61, 0x48, 0x1f, 0x2e, 0xac, 0xc8, 0xbc, 0x56,
	0x41, 0x60, 0x26, 0x1b, 0xcf, 0xaa, 0x5b, 0xbe,
	0xef, 0x73, 0x6d, 0x5e, 0xf7, 0x86, 0x47, 0xbd,
	0x88, 0xfc, 0xbf, 0x4e, 0x76, 0xe0, 0x53, 0x6c,
	0x49, 0x40, 0x38, 0x34, 0xe4, 0xeb, 0x15, 0x11,
	0x8b, 0x85, 0xaf, 0xa9, 0x5f, 0x52, 0x98, 0x92,
	0xfb, 0xb5, 0xee, 0x51, 0xb7, 0xf0, 0x5c, 0xe1,
	0xdc, 0x2b, 0x95, 0x13, 0x23, 0xdf, 0x17, 0x9f,
	0xd3, 0x19, 0xc4, 0x3a, 0x8a, 0x69, 0x55, 0xf6,
	0x58, 0xfd, 0x84, 0x68, 0xc3, 0x36, 0xd0, 0x1d,
	0xa6, 0xf3, 0x6f, 0x99, 0x12, 0x7a, 0xba, 0x3e,
	0x6e, 0x93, 0xa0, 0xf8, 0xb8, 0x32, 0x16, 0x7f,
	0x9a, 0xf9, 0xe2, 0xdb, 0xed, 0xd8, 0x90, 0xf2,
	0xae, 0x6b, 0x4d, 0xce, 0x44, 0xc9, 0xa8, 0x6a,
	0xc7, 0x2c, 0xc0, 0x24, 0xfa, 0x71, 0xf1, 0x74,
	0x9c, 0x33, 0x96, 0x3f, 0x46, 0x57, 0x4f, 0x5a,
	0xb2, 0x25, 0x37, 0x8c, 0x82, 0x3b, 0x2d, 0xb0,
	0x45, 0xad, 0xd7, 0xff, 0xf4, 0xd4, 0xab, 0x4c,
	0x8e, 0x1c, 0x18, 0x80, 0xcd, 0xf5, 0xfe, 0xca,
	0xa5, 0xec, 0xe3, 0xa3, 0x78, 0x2a, 0x22, 0x7d,
	0x5d, 0x77, 0xa2, 0xda, 0x64, 0xea, 0x21, 0x3d,
	0x31, 0x29, 0xe5, 0x65, 0xd9, 0xa4, 0x72, 0x50,
	0x75, 0xb6, 0xa7, 0x91, 0xcc, 0xd5, 0x87, 0x54,
	0x9b, 0xa1, 0xb4, 0x70, 0x59, 0x89, 0xd6, 0xcb,
];

impl TowerFieldArithmetic for BinaryField2b {
	fn multiply(self, rhs: Self) -> Self {
		Self(mul_bin_4b(self.0, rhs.0))
	}

	fn multiply_alpha(self) -> Self {
		self * Self(0x02)
	}

	fn square(self) -> Self {
		self * self
	}

	fn invert(self) -> CtOption<Self> {
		CtOption::new(Self(INVERSE_8B[self.0 as usize]), self.0.ct_ne(&0))
	}
}

impl TowerFieldArithmetic for BinaryField4b {
	fn multiply(self, rhs: Self) -> Self {
		Self(mul_bin_4b(self.0, rhs.0))
	}

	fn multiply_alpha(self) -> Self {
		self * Self(0x04)
	}

	fn square(self) -> Self {
		self * self
	}

	fn invert(self) -> CtOption<Self> {
		CtOption::new(Self(INVERSE_8B[self.0 as usize]), self.0.ct_ne(&0))
	}
}

impl TowerFieldArithmetic for BinaryField8b {
	fn multiply(self, rhs: Self) -> Self {
		multiply(self, rhs)
	}

	fn multiply_alpha(self) -> Self {
		#[rustfmt::skip]
		const ALPHA_MAP: [u8; 256] = [
			0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70,
			0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0,
			0x41, 0x51, 0x61, 0x71, 0x01, 0x11, 0x21, 0x31,
			0xc1, 0xd1, 0xe1, 0xf1, 0x81, 0x91, 0xa1, 0xb1,
			0x82, 0x92, 0xa2, 0xb2, 0xc2, 0xd2, 0xe2, 0xf2,
			0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72,
			0xc3, 0xd3, 0xe3, 0xf3, 0x83, 0x93, 0xa3, 0xb3,
			0x43, 0x53, 0x63, 0x73, 0x03, 0x13, 0x23, 0x33,
			0x94, 0x84, 0xb4, 0xa4, 0xd4, 0xc4, 0xf4, 0xe4,
			0x14, 0x04, 0x34, 0x24, 0x54, 0x44, 0x74, 0x64,
			0xd5, 0xc5, 0xf5, 0xe5, 0x95, 0x85, 0xb5, 0xa5,
			0x55, 0x45, 0x75, 0x65, 0x15, 0x05, 0x35, 0x25,
			0x16, 0x06, 0x36, 0x26, 0x56, 0x46, 0x76, 0x66,
			0x96, 0x86, 0xb6, 0xa6, 0xd6, 0xc6, 0xf6, 0xe6,
			0x57, 0x47, 0x77, 0x67, 0x17, 0x07, 0x37, 0x27,
			0xd7, 0xc7, 0xf7, 0xe7, 0x97, 0x87, 0xb7, 0xa7,
			0xe8, 0xf8, 0xc8, 0xd8, 0xa8, 0xb8, 0x88, 0x98,
			0x68, 0x78, 0x48, 0x58, 0x28, 0x38, 0x08, 0x18,
			0xa9, 0xb9, 0x89, 0x99, 0xe9, 0xf9, 0xc9, 0xd9,
			0x29, 0x39, 0x09, 0x19, 0x69, 0x79, 0x49, 0x59,
			0x6a, 0x7a, 0x4a, 0x5a, 0x2a, 0x3a, 0x0a, 0x1a,
			0xea, 0xfa, 0xca, 0xda, 0xaa, 0xba, 0x8a, 0x9a,
			0x2b, 0x3b, 0x0b, 0x1b, 0x6b, 0x7b, 0x4b, 0x5b,
			0xab, 0xbb, 0x8b, 0x9b, 0xeb, 0xfb, 0xcb, 0xdb,
			0x7c, 0x6c, 0x5c, 0x4c, 0x3c, 0x2c, 0x1c, 0x0c,
			0xfc, 0xec, 0xdc, 0xcc, 0xbc, 0xac, 0x9c, 0x8c,
			0x3d, 0x2d, 0x1d, 0x0d, 0x7d, 0x6d, 0x5d, 0x4d,
			0xbd, 0xad, 0x9d, 0x8d, 0xfd, 0xed, 0xdd, 0xcd,
			0xfe, 0xee, 0xde, 0xce, 0xbe, 0xae, 0x9e, 0x8e,
			0x7e, 0x6e, 0x5e, 0x4e, 0x3e, 0x2e, 0x1e, 0x0e,
			0xbf, 0xaf, 0x9f, 0x8f, 0xff, 0xef, 0xdf, 0xcf,
			0x3f, 0x2f, 0x1f, 0x0f, 0x7f, 0x6f, 0x5f, 0x4f,
		];
		Self(ALPHA_MAP[self.0 as usize])
	}

	fn square(self) -> Self {
		#[rustfmt::skip]
		const SQUARE_MAP: [u8; 256] = [
			0x00, 0x01, 0x03, 0x02, 0x09, 0x08, 0x0a, 0x0b,
			0x07, 0x06, 0x04, 0x05, 0x0e, 0x0f, 0x0d, 0x0c,
			0x41, 0x40, 0x42, 0x43, 0x48, 0x49, 0x4b, 0x4a,
			0x46, 0x47, 0x45, 0x44, 0x4f, 0x4e, 0x4c, 0x4d,
			0xc3, 0xc2, 0xc0, 0xc1, 0xca, 0xcb, 0xc9, 0xc8,
			0xc4, 0xc5, 0xc7, 0xc6, 0xcd, 0xcc, 0xce, 0xcf,
			0x82, 0x83, 0x81, 0x80, 0x8b, 0x8a, 0x88, 0x89,
			0x85, 0x84, 0x86, 0x87, 0x8c, 0x8d, 0x8f, 0x8e,
			0xa9, 0xa8, 0xaa, 0xab, 0xa0, 0xa1, 0xa3, 0xa2,
			0xae, 0xaf, 0xad, 0xac, 0xa7, 0xa6, 0xa4, 0xa5,
			0xe8, 0xe9, 0xeb, 0xea, 0xe1, 0xe0, 0xe2, 0xe3,
			0xef, 0xee, 0xec, 0xed, 0xe6, 0xe7, 0xe5, 0xe4,
			0x6a, 0x6b, 0x69, 0x68, 0x63, 0x62, 0x60, 0x61,
			0x6d, 0x6c, 0x6e, 0x6f, 0x64, 0x65, 0x67, 0x66,
			0x2b, 0x2a, 0x28, 0x29, 0x22, 0x23, 0x21, 0x20,
			0x2c, 0x2d, 0x2f, 0x2e, 0x25, 0x24, 0x26, 0x27,
			0x57, 0x56, 0x54, 0x55, 0x5e, 0x5f, 0x5d, 0x5c,
			0x50, 0x51, 0x53, 0x52, 0x59, 0x58, 0x5a, 0x5b,
			0x16, 0x17, 0x15, 0x14, 0x1f, 0x1e, 0x1c, 0x1d,
			0x11, 0x10, 0x12, 0x13, 0x18, 0x19, 0x1b, 0x1a,
			0x94, 0x95, 0x97, 0x96, 0x9d, 0x9c, 0x9e, 0x9f,
			0x93, 0x92, 0x90, 0x91, 0x9a, 0x9b, 0x99, 0x98,
			0xd5, 0xd4, 0xd6, 0xd7, 0xdc, 0xdd, 0xdf, 0xde,
			0xd2, 0xd3, 0xd1, 0xd0, 0xdb, 0xda, 0xd8, 0xd9,
			0xfe, 0xff, 0xfd, 0xfc, 0xf7, 0xf6, 0xf4, 0xf5,
			0xf9, 0xf8, 0xfa, 0xfb, 0xf0, 0xf1, 0xf3, 0xf2,
			0xbf, 0xbe, 0xbc, 0xbd, 0xb6, 0xb7, 0xb5, 0xb4,
			0xb8, 0xb9, 0xbb, 0xba, 0xb1, 0xb0, 0xb2, 0xb3,
			0x3d, 0x3c, 0x3e, 0x3f, 0x34, 0x35, 0x37, 0x36,
			0x3a, 0x3b, 0x39, 0x38, 0x33, 0x32, 0x30, 0x31,
			0x7c, 0x7d, 0x7f, 0x7e, 0x75, 0x74, 0x76, 0x77,
			0x7b, 0x7a, 0x78, 0x79, 0x72, 0x73, 0x71, 0x70,
		];
		Self(SQUARE_MAP[self.0 as usize])
	}

	fn invert(self) -> CtOption<Self> {
		CtOption::new(Self(INVERSE_8B[self.0 as usize]), self.0.ct_ne(&0))
	}
}

binary_tower_unary_arithmetic_recursive!(BinaryField16b);
binary_tower_unary_arithmetic_recursive!(BinaryField32b);
binary_tower_unary_arithmetic_recursive!(BinaryField64b);

impl TowerFieldArithmetic for BinaryField128b {
	cfg_if! {
		// HACK: Carve-out for accelerated packed field arithmetic. This is temporary until the
		// portable packed128b implementation is refactored to not rely on BinaryField mul.
		if #[cfg(all(target_arch = "x86_64", target_feature = "gfni", target_feature = "sse2"))] {
			fn multiply(self, rhs: Self) -> Self {
				use bytemuck::must_cast;
				use crate::field::PackedBinaryField1x128b;

				let a = must_cast::<_, PackedBinaryField1x128b>(self);
				let b = must_cast::<_, PackedBinaryField1x128b>(rhs);
				must_cast(a * b)
			}
		} else {
			fn multiply(self, rhs: Self) -> Self {
				multiply(self, rhs)
			}
		}
	}

	fn multiply_alpha(self) -> Self {
		multiply_alpha(self)
	}

	fn square(self) -> Self {
		square(self)
	}

	fn invert(self) -> CtOption<Self> {
		invert(self)
	}
}

fn multiply<F>(a: F, b: F) -> F
where
	F: TowerExtensionField,
	F::DirectSubfield: TowerFieldArithmetic,
{
	let (a0, a1) = a.into();
	let (b0, b1) = b.into();
	let z0 = a0 * b0;
	let z2 = a1 * b1;
	let z0z2 = z0 + z2;
	let z1 = (a0 + a1) * (b0 + b1) - z0z2;
	let z2a = z2.multiply_alpha();
	(z0z2, z1 + z2a).into()
}

fn multiply_alpha<F>(a: F) -> F
where
	F: TowerExtensionField,
	F::DirectSubfield: TowerFieldArithmetic,
{
	let (a0, a1) = a.into();
	let z1 = a1.multiply_alpha();
	(a1, a0 + z1).into()
}

fn square<F>(a: F) -> F
where
	F: TowerExtensionField,
	F::DirectSubfield: TowerFieldArithmetic,
{
	let (a0, a1) = a.into();
	let z0 = a0.square();
	let z2 = a1.square();
	let z2a = z2.multiply_alpha();
	(z0 + z2, z2a).into()
}

fn invert<F>(a: F) -> CtOption<F>
where
	F: TowerExtensionField,
	F::DirectSubfield: TowerFieldArithmetic,
{
	let (a0, a1) = a.into();
	let a0z1 = a0 + a1.multiply_alpha();
	let delta = a0 * a0z1 + a1.square();
	delta.invert().map(|delta_inv| {
		let inv0 = delta_inv * a0z1;
		let inv1 = delta_inv * a1;
		(inv0, inv1).into()
	})
}
