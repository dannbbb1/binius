// Copyright 2023 Ulvetanna Inc.

use p3_challenger::{CanObserve, CanSample, CanSampleBits};
use p3_matrix::{dense::RowMajorMatrix, MatrixRowSlices};
use p3_util::{log2_ceil_usize, log2_strict_usize};
use rayon::prelude::*;
use std::{iter::repeat_with, marker::PhantomData};

use super::error::{Error, VerificationError};
use crate::{
	field::{
		get_packed_slice, iter_packed_slice, square_transpose, transpose_scalars, unpack_scalars,
		unpack_scalars_mut, util::inner_product_unchecked, BinaryField8b, ExtensionField, Field,
		PackedExtensionField, PackedField,
	},
	hash::{hash, GroestlDigest, GroestlDigestCompression, GroestlHasher, Hasher},
	linear_code::LinearCode,
	merkle_tree::{MerkleTreeVCS, VectorCommitScheme},
	poly_commit::PolyCommitScheme,
	polynomial::{
		multilinear_query::MultilinearQuery, Error as PolynomialError, MultilinearExtension,
	},
};

/// Creates a new multilinear from a batch of multilinears and a mixing challenge
///
/// REQUIRES:
///     All inputted multilinear polynomials have $\mu := \text{n_vars}$ variables
///     t_primes.len() == mixing_coeffs.len()
/// ENSURES:
///     Given a batch of $m$ multilinear polynomials $t_i$'s, and $n$ mixing coeffs $c_i$,
///     this function computes the multilinear polynomial $t$ such that
///     $\forall v \in \{0, 1\}^{\mu}$, $t(v) = \sum_{i=0}^{n-1} c_i * t_i(v)$
fn mix_t_primes<F, P>(
	n_vars: usize,
	t_primes: &[MultilinearExtension<'_, P>],
	mixing_coeffs: &[F],
) -> Result<MultilinearExtension<'static, P>, Error>
where
	F: Field,
	P: PackedField<Scalar = F>,
{
	for t_prime_i in t_primes {
		if t_prime_i.n_vars() != n_vars {
			return Err(Error::IncorrectPolynomialSize { expected: n_vars });
		}
	}

	let mixed_evals = (0..(1 << n_vars) / P::WIDTH)
		.into_par_iter()
		.map(|i| {
			t_primes
				.iter()
				.map(|t_prime| t_prime.evals()[i])
				.zip(mixing_coeffs.iter().copied())
				.map(|(t_prime_i, coeff)| t_prime_i * coeff)
				.sum()
		})
		.collect::<Vec<_>>();

	let mixed_t_prime = MultilinearExtension::from_values(mixed_evals)?;
	Ok(mixed_t_prime)
}

/// Evaluation proof data for the `TensorPCS` polynomial commitment scheme.
///
/// # Type Parameters
///
/// * `PI`: The packed intermediate field type.
/// * `PE`: The packed extension field type.
/// * `VCSProof`: The vector commitment scheme proof type.
#[derive(Debug)]
pub struct Proof<'a, PI, PE, VCSProof>
where
	PE: PackedField,
{
	/// Number of distinct multilinear polynomials in the batch opening proof
	pub n_polys: usize,
	/// Represents a mixing of individual polynomial t_primes
	///
	/// Let $n$ denote n_polys. Define $l = \lceil\log_2(n)\rceil$.
	/// Let $\alpha_0, \ldots, \alpha_{l-1}$ be the sampled mixing challenges.
	/// Then $c := \otimes_{i=0}^{l-1} (1 - \alpha_i, \alpha_i)$ are the $2^l$ mixing coefficients,
	/// denoting the $i$-th coefficient by $c_i$.
	/// Let $t'_i$ denote the $t'$ for the $i$-th polynomial in the batch opening proof.
	/// This value represents the multilinear polynomial such that $\forall v \in \{0, 1\}^{\mu}$,
	/// $v \rightarrow \sum_{i=0}^{n-1} c_i * t'_i(v)$
	pub mixed_t_prime: MultilinearExtension<'a, PE>,
	/// Opening proofs for chosen columns of the encoded matrices
	///
	/// Let $j_1, \ldots, j_k$ be the indices of the columns that are opened.
	/// The ith element is a tuple of:
	/// * A vector (size=n_polys) of the $j_i$th columns (one from each polynomial's encoded matrix)
	/// * A proof that these columns are consistent with the vector commitment
	pub vcs_proofs: Vec<(Vec<Vec<PI>>, VCSProof)>,
}

/// The multilinear polynomial commitment scheme specified in [DP23].
///
/// # Type Parameters
///
/// * `P`: The base field type of committed elements.
/// * `PA`: The field type of the encoding alphabet.
/// * `PI`: The intermediate field type that base field elements are packed into.
/// * `PE`: The extension field type used for cryptographic challenges.
///
/// [DP23]: https://eprint.iacr.org/2023/630
#[derive(Debug, Copy, Clone)]
pub struct TensorPCS<P, PA, PI, PE, LC, H, VCS>
where
	P: PackedField,
	PA: PackedField,
	PI: PackedField,
	PE: PackedField,
	LC: LinearCode<P = PA>,
	H: Hasher<PI>,
	VCS: VectorCommitScheme<H::Digest>,
{
	log_rows: usize,
	code: LC,
	vcs: VCS,
	_p_marker: PhantomData<P>,
	_pi_marker: PhantomData<PI>,
	_h_marker: PhantomData<H>,
	_ext_marker: PhantomData<PE>,
}

impl<P, PA, PI, PE, LC>
	TensorPCS<
		P,
		PA,
		PI,
		PE,
		LC,
		GroestlHasher<PI>,
		MerkleTreeVCS<
			GroestlDigest,
			GroestlDigest,
			GroestlHasher<GroestlDigest>,
			GroestlDigestCompression,
		>,
	> where
	P: PackedField,
	PA: PackedField,
	PI: PackedField + PackedExtensionField<BinaryField8b> + Sync,
	PI::Scalar: ExtensionField<P::Scalar> + ExtensionField<BinaryField8b>,
	PE: PackedField,
	PE::Scalar: ExtensionField<P::Scalar>,
	LC: LinearCode<P = PA>,
{
	pub fn new_using_groestl_merkle_tree(log_rows: usize, code: LC) -> Result<Self, Error> {
		// Check power of two length because MerkleTreeVCS requires it
		if !code.len().is_power_of_two() {
			return Err(Error::CodeLengthPowerOfTwoRequired);
		}
		let log_len = log2_strict_usize(code.len());
		Self::new(log_rows, code, MerkleTreeVCS::new(log_len, GroestlDigestCompression))
	}
}

impl<F, P, FA, PA, FI, PI, FE, PE, LC, H, VCS> PolyCommitScheme<P, FE>
	for TensorPCS<P, PA, PI, PE, LC, H, VCS>
where
	F: Field,
	P: PackedField<Scalar = F> + Send,
	FA: Field,
	PA: PackedField<Scalar = FA>,
	FI: ExtensionField<F> + ExtensionField<FA>,
	PI: PackedField<Scalar = FI>
		+ PackedExtensionField<FI>
		+ PackedExtensionField<P>
		+ PackedExtensionField<PA>
		+ Sync,
	FE: ExtensionField<F> + ExtensionField<FI>,
	PE: PackedField<Scalar = FE> + PackedExtensionField<PI> + PackedExtensionField<FE>,
	LC: LinearCode<P = PA>,
	H: Hasher<PI>,
	H::Digest: Copy + Default + Send,
	VCS: VectorCommitScheme<H::Digest>,
{
	type Commitment = VCS::Commitment;
	type Committed = (Vec<RowMajorMatrix<PI>>, VCS::Committed);
	type Proof = Proof<'static, PI, PE, VCS::Proof>;
	type Error = Error;

	fn n_vars(&self) -> usize {
		self.log_rows() + self.log_cols()
	}

	fn commit(
		&self,
		polys: &[&MultilinearExtension<P>],
	) -> Result<(Self::Commitment, Self::Committed), Error> {
		for poly in polys {
			if poly.n_vars() != self.n_vars() {
				return Err(Error::IncorrectPolynomialSize {
					expected: self.n_vars(),
				});
			}
		}

		// These conditions are checked by the constructor, so are safe to assert defensively
		debug_assert_eq!(self.code.dim() % PI::WIDTH, 0);

		// Dimensions as an intermediate field matrix.
		let n_rows = 1 << self.log_rows;
		let n_cols_enc = self.code.len();

		let mut encoded_mats = Vec::with_capacity(polys.len());
		let mut all_digests = Vec::with_capacity(polys.len());
		for poly in polys {
			let mut encoded = vec![PI::default(); n_rows * n_cols_enc / PI::WIDTH];
			let poly_vals_packed =
				PI::try_cast_to_ext(poly.evals()).ok_or_else(|| Error::UnalignedMessage)?;

			transpose::transpose(
				unpack_scalars(poly_vals_packed),
				unpack_scalars_mut(&mut encoded[..n_rows * self.code.dim() / PI::WIDTH]),
				1 << self.code.dim_bits(),
				1 << self.log_rows,
			);

			// TODO: Parallelize
			self.code
				.encode_batch_inplace(
					<PI as PackedExtensionField<PA>>::cast_to_bases_mut(&mut encoded),
					self.log_rows + log2_strict_usize(<FI as ExtensionField<FA>>::DEGREE),
				)
				.map_err(|err| Error::EncodeError(Box::new(err)))?;

			let mut digests = vec![H::Digest::default(); n_cols_enc];
			encoded
				.par_chunks_exact(n_rows / PI::WIDTH)
				.map(hash::<_, H>)
				.collect_into_vec(&mut digests);
			all_digests.push(digests);

			let encoded_mat = RowMajorMatrix::new(encoded, n_rows / PI::WIDTH);
			encoded_mats.push(encoded_mat);
		}

		let (commitment, vcs_committed) = self
			.vcs
			.commit_batch(all_digests.into_iter())
			.map_err(|err| Error::VectorCommit(Box::new(err)))?;
		Ok((commitment, (encoded_mats, vcs_committed)))
	}

	/// Generate an evaluation proof at a *random* challenge point.
	///
	/// Follows the notation from Construction 4.6 in [DP23].
	///
	/// Precondition: The queried point must already be observed by the challenger.
	///
	/// [DP23]: https://eprint.iacr.org/2023/630
	fn prove_evaluation<CH>(
		&self,
		challenger: &mut CH,
		committed: &Self::Committed,
		polys: &[&MultilinearExtension<P>],
		query: &[FE],
	) -> Result<Self::Proof, Error>
	where
		CH: CanObserve<FE> + CanSample<FE> + CanSampleBits<usize>,
	{
		let n_polys = polys.len();
		let n_challenges = log2_ceil_usize(n_polys);
		let mixing_challenges = challenger.sample_vec(n_challenges);
		let mixing_coefficients =
			&MultilinearQuery::with_full_query(&mixing_challenges)?.into_expansion()[..n_polys];

		let (col_major_mats, ref vcs_committed) = committed;
		if col_major_mats.len() != n_polys {
			return Err(Error::NumBatchedMismatchError {
				err_str: format!("In prove_evaluation: number of polynomials {} must match number of committed matrices {}", n_polys, col_major_mats.len()),
			});
		}

		if query.len() != self.n_vars() {
			return Err(PolynomialError::IncorrectQuerySize {
				expected: self.n_vars(),
			}
			.into());
		}

		let code_len_bits = log2_strict_usize(self.code.len());
		let log_block_size = log2_strict_usize(<FI as ExtensionField<F>>::DEGREE);
		let log_n_cols = self.code.dim_bits() + log_block_size;

		let partial_query = &MultilinearQuery::with_full_query(&query[log_n_cols..])?;
		let ts = polys;
		let t_primes = ts
			.iter()
			.map(|t| t.evaluate_partial_high(partial_query))
			.collect::<Result<Vec<_>, _>>()?;
		let t_prime = mix_t_primes(log_n_cols, &t_primes, mixing_coefficients)?;

		challenger.observe_slice(unpack_scalars(t_prime.evals()));
		let merkle_proofs = repeat_with(|| challenger.sample_bits(code_len_bits))
			.take(self.code.n_test_queries())
			.map(|index| {
				let vcs_proof = self
					.vcs
					.prove_batch_opening(vcs_committed, index)
					.map_err(|err| Error::VectorCommit(Box::new(err)))?;

				let cols: Vec<_> = col_major_mats
					.iter()
					.map(|col_major_mat| col_major_mat.row_slice(index).to_vec())
					.collect();

				Ok((cols, vcs_proof))
			})
			.collect::<Result<_, Error>>()?;

		Ok(Proof {
			n_polys,
			mixed_t_prime: t_prime,
			vcs_proofs: merkle_proofs,
		})
	}

	/// Verify an evaluation proof at a *random* challenge point.
	///
	/// Follows the notation from Construction 4.6 in [DP23].
	///
	/// Precondition: The queried point must already be observed by the challenger.
	///
	/// [DP23]: https://eprint.iacr.org/2023/630
	fn verify_evaluation<CH>(
		&self,
		challenger: &mut CH,
		commitment: &Self::Commitment,
		query: &[FE],
		proof: Self::Proof,
		values: &[FE],
	) -> Result<(), Error>
	where
		CH: CanObserve<FE> + CanSample<FE> + CanSampleBits<usize>,
	{
		// These are all checked during construction, so it is safe to assert as a defensive
		// measure.
		debug_assert_eq!(self.code.dim() % PI::WIDTH, 0);
		debug_assert_eq!((1 << self.log_rows) % P::WIDTH, 0);
		debug_assert_eq!((1 << self.log_rows) % PI::WIDTH, 0);
		debug_assert_eq!(self.code.dim() % PI::WIDTH, 0);
		debug_assert_eq!(self.code.dim() % PE::WIDTH, 0);

		if values.len() != proof.n_polys {
			return Err(Error::NumBatchedMismatchError {
				err_str:
					format!("In verify_evaluation: proof number of polynomials {} must match number of opened values {}", proof.n_polys, values.len()),
			});
		}

		let n_challenges = log2_ceil_usize(proof.n_polys);
		let mixing_challenges = challenger.sample_vec(n_challenges);
		let mixing_coefficients = &MultilinearQuery::<PE>::with_full_query(&mixing_challenges)?
			.into_expansion()[..proof.n_polys];
		let value =
			inner_product_unchecked(values.iter().copied(), iter_packed_slice(mixing_coefficients));

		if query.len() != self.n_vars() {
			return Err(PolynomialError::IncorrectQuerySize {
				expected: self.n_vars(),
			}
			.into());
		}

		self.check_proof_shape(&proof)?;

		// Code length is checked to be a power of two in the constructor
		let code_len_bits = log2_strict_usize(self.code.len());
		let block_size = <FI as ExtensionField<F>>::DEGREE;
		let log_block_size = log2_strict_usize(block_size);
		let log_n_cols = self.code.dim_bits() + log_block_size;

		let n_rows = 1 << self.log_rows;

		challenger.observe_slice(unpack_scalars(proof.mixed_t_prime.evals()));

		// Check evaluation of t' matches the claimed value
		let multilin_query = MultilinearQuery::<PE>::with_full_query(&query[..log_n_cols])?;
		let computed_value = proof
			.mixed_t_prime
			.evaluate(&multilin_query)
			.expect("query is the correct size by check_proof_shape checks");
		if computed_value != value {
			return Err(VerificationError::IncorrectEvaluation.into());
		}

		// Encode t' into u'
		let mut u_prime = vec![PE::default(); (1 << (code_len_bits + log_block_size)) / PE::WIDTH];
		self.encode_ext(proof.mixed_t_prime.evals(), &mut u_prime)?;

		// Check vector commitment openings.
		let columns = proof
			.vcs_proofs
			.into_iter()
			.map(|(cols, vcs_proof)| {
				let index = challenger.sample_bits(code_len_bits);

				let leaf_digests = cols.iter().map(hash::<_, H>);

				self.vcs
					.verify_batch_opening(commitment, index, vcs_proof, leaf_digests)
					.map_err(|err| Error::VectorCommit(Box::new(err)))?;

				Ok((index, cols))
			})
			.collect::<Result<Vec<_>, Error>>()?;

		// Get the sequence of column tests.
		let column_tests = columns
			.into_iter()
			.flat_map(|(index, cols)| {
				let mut batched_column_test = (0..block_size)
					.map(|j| {
						let u_prime_i = get_packed_slice(&u_prime, index << log_block_size | j);
						let base_cols = Vec::with_capacity(proof.n_polys);
						(u_prime_i, base_cols)
					})
					.collect::<Vec<_>>();

				cols.iter().for_each(|col| {
					// Checked by check_proof_shape
					debug_assert_eq!(col.len(), n_rows / PI::WIDTH);

					// The columns are committed to and provided by the prover as packed vectors of
					// intermediate field elements. We need to transpose them into packed base field
					// elements to perform the consistency checks. Allocate col_transposed as packed
					// intermediate field elements to guarantee alignment.
					let mut col_transposed = vec![PI::default(); n_rows / PI::WIDTH];
					let base_cols =
						PackedExtensionField::<P>::cast_to_bases_mut(&mut col_transposed);
					transpose_scalars(col, base_cols).expect(
						"guaranteed safe because of parameter checks in constructor; \
							alignment is guaranteed the cast from a PI slice",
					);

					debug_assert_eq!(base_cols.len(), n_rows / P::WIDTH * block_size);

					(0..block_size)
						.zip(base_cols.chunks_exact(n_rows / P::WIDTH))
						.for_each(|(j, col)| {
							batched_column_test[j].1.push(col.to_vec());
						});
				});
				batched_column_test
			})
			.collect::<Vec<_>>();

		// Batch evaluate all opened columns
		let multilin_query = MultilinearQuery::<PE>::with_full_query(&query[log_n_cols..])?;
		let expected_and_actual_results = column_tests.iter().map(|(expected, leaves)| {
			let actual_evals = leaves
				.iter()
				.map(|leaf| {
					MultilinearExtension::from_values_slice(leaf)
						.expect("leaf is guaranteed power of two length due to check_proof_shape")
						.evaluate(&multilin_query)
						.expect("failed to evaluate")
				})
				.collect::<Vec<_>>();
			(expected, actual_evals)
		});

		// Check that opened column evaluations match u'
		for test in expected_and_actual_results {
			let (expected_result, unmixed_actual_results) = test;
			let actual_result = inner_product_unchecked(
				unmixed_actual_results.into_iter(),
				iter_packed_slice(mixing_coefficients),
			);
			if actual_result != *expected_result {
				return Err(VerificationError::IncorrectPartialEvaluation.into());
			}
		}

		Ok(())
	}
}

impl<F, P, FA, PA, FI, PI, FE, PE, LC, H, VCS> TensorPCS<P, PA, PI, PE, LC, H, VCS>
where
	F: Field,
	P: PackedField<Scalar = F>,
	FA: Field,
	PA: PackedField<Scalar = FA>,
	FI: ExtensionField<F>,
	PI: PackedField<Scalar = FI>,
	FE: ExtensionField<F>,
	PE: PackedField<Scalar = FE>,
	LC: LinearCode<P = PA>,
	H: Hasher<PI>,
	VCS: VectorCommitScheme<H::Digest>,
{
	/// Construct a [`TensorPCS`].
	///
	/// The constructor checks the validity of the type arguments and constructor arguments.
	///
	/// Throws if the linear code block length is not a power of 2.
	/// Throws if the packing width does not divide the code dimension.
	pub fn new(log_rows: usize, code: LC, vcs: VCS) -> Result<Self, Error> {
		if !code.len().is_power_of_two() {
			// This requirement is just to make sampling indices easier. With a little work it
			// could be relaxed, but power-of-two code lengths are more convenient to work with.
			return Err(Error::CodeLengthPowerOfTwoRequired);
		}

		if !<FI as ExtensionField<F>>::DEGREE.is_power_of_two() {
			return Err(Error::ExtensionDegreePowerOfTwoRequired);
		}
		if !FE::DEGREE.is_power_of_two() {
			return Err(Error::ExtensionDegreePowerOfTwoRequired);
		}

		if (1 << log_rows) % P::WIDTH != 0 {
			return Err(Error::PackingWidthMustDivideNumberOfRows);
		}
		if (1 << log_rows) % PI::WIDTH != 0 {
			return Err(Error::PackingWidthMustDivideNumberOfRows);
		}
		if code.dim() % PI::WIDTH != 0 {
			return Err(Error::PackingWidthMustDivideCodeDimension);
		}
		if code.dim() % PE::WIDTH != 0 {
			return Err(Error::PackingWidthMustDivideCodeDimension);
		}

		Ok(Self {
			log_rows,
			code,
			vcs,
			_p_marker: PhantomData,
			_pi_marker: PhantomData,
			_h_marker: PhantomData,
			_ext_marker: PhantomData,
		})
	}

	/// The base-2 logarithm of the number of rows in the committed matrix.
	pub fn log_rows(&self) -> usize {
		self.log_rows
	}

	/// The base-2 logarithm of the number of columns in the pre-encoded matrix.
	pub fn log_cols(&self) -> usize {
		self.code.dim_bits() + log2_strict_usize(FI::DEGREE)
	}
}

// Helper functions for PolyCommitScheme implementation.
impl<F, P, FA, PA, FI, PI, FE, PE, LC, H, VCS> TensorPCS<P, PA, PI, PE, LC, H, VCS>
where
	F: Field,
	P: PackedField<Scalar = F> + Send,
	FA: Field,
	PA: PackedField<Scalar = FA>,
	FI: ExtensionField<P::Scalar> + ExtensionField<PA::Scalar>,
	PI: PackedField<Scalar = FI>
		+ PackedExtensionField<FI>
		+ PackedExtensionField<P>
		+ PackedExtensionField<PA>
		+ Sync,
	FE: ExtensionField<F> + ExtensionField<FI>,
	PE: PackedField<Scalar = FE> + PackedExtensionField<PI> + PackedExtensionField<FE>,
	LC: LinearCode<P = PA>,
	H: Hasher<PI>,
	H::Digest: Copy + Default + Send,
	VCS: VectorCommitScheme<H::Digest>,
{
	fn check_proof_shape(&self, proof: &Proof<PI, PE, VCS::Proof>) -> Result<(), Error> {
		let n_rows = 1 << self.log_rows;
		let log_block_size = log2_strict_usize(<FI as ExtensionField<F>>::DEGREE);
		let log_n_cols = self.code.dim_bits() + log_block_size;
		let n_queries = self.code.n_test_queries();

		if proof.vcs_proofs.len() != n_queries {
			return Err(VerificationError::NumberOfOpeningProofs {
				expected: n_queries,
			}
			.into());
		}
		for (col_idx, (polys_col, _)) in proof.vcs_proofs.iter().enumerate() {
			if polys_col.len() != proof.n_polys {
				return Err(Error::NumBatchedMismatchError {
					err_str: format!(
						"Expected {} polynomials, but VCS proof at col_idx {} found {} polynomials instead",
						proof.n_polys,
						col_idx,
						polys_col.len()
					),
				});
			}

			for (poly_idx, poly_col) in polys_col.iter().enumerate() {
				if poly_col.len() * PI::WIDTH != n_rows {
					return Err(VerificationError::OpenedColumnSize {
						col_index: col_idx,
						poly_index: poly_idx,
						expected: n_rows,
						actual: poly_col.len() * PI::WIDTH,
					}
					.into());
				}
			}
		}

		if proof.mixed_t_prime.n_vars() != log_n_cols {
			return Err(VerificationError::PartialEvaluationSize.into());
		}

		Ok(())
	}

	fn encode_ext(&self, t_prime: &[PE], u_prime: &mut [PE]) -> Result<(), Error> {
		let code_len_bits = log2_strict_usize(self.code.len());
		let block_size = <FI as ExtensionField<F>>::DEGREE;
		let log_block_size = log2_strict_usize(block_size);
		let log_n_cols = self.code.dim_bits() + log_block_size;

		assert_eq!(t_prime.len(), (1 << log_n_cols) / PE::WIDTH);
		assert_eq!(u_prime.len(), (1 << (code_len_bits + log_block_size)) / PE::WIDTH);

		u_prime[..(1 << log_n_cols) / PE::WIDTH].copy_from_slice(t_prime);

		// View u' as a vector of packed base field elements and transpose into packed intermediate
		// field elements in order to apply the extension encoding.
		if log_block_size > 0 {
			// TODO: This requirement is necessary for how we perform the following transpose.
			// It should be relaxed by providing yet another PackedField type as a generic
			// parameter for which this is true.
			assert!(P::WIDTH <= <FE as ExtensionField<F>>::DEGREE);

			let f_view = PackedExtensionField::<P>::cast_to_bases_mut(
				PackedExtensionField::<PI>::cast_to_bases_mut(
					&mut u_prime[..(1 << log_n_cols) / PE::WIDTH],
				),
			);
			f_view
				.par_chunks_exact_mut(block_size)
				.try_for_each(|chunk| square_transpose(log_block_size, chunk))?;
		}

		// View u' as a vector of packed intermediate field elements and batch encode.
		{
			let fi_view = PackedExtensionField::<PI>::cast_to_bases_mut(u_prime);
			let log_batch_size = log2_strict_usize(<FE as ExtensionField<F>>::DEGREE);
			self.code
				.encode_batch_inplace(
					<PI as PackedExtensionField<PA>>::cast_to_bases_mut(fi_view),
					log_batch_size + log2_strict_usize(<FI as ExtensionField<FA>>::DEGREE),
				)
				.map_err(|err| Error::EncodeError(Box::new(err)))?;
		}

		if log_block_size > 0 {
			// TODO: This requirement is necessary for how we perform the following transpose.
			// It should be relaxed by providing yet another PackedField type as a generic
			// parameter for which this is true.
			assert!(P::WIDTH <= <FE as ExtensionField<F>>::DEGREE);

			let f_view = PackedExtensionField::<P>::cast_to_bases_mut(
				PackedExtensionField::<PI>::cast_to_bases_mut(u_prime),
			);
			f_view
				.par_chunks_exact_mut(block_size)
				.try_for_each(|chunk| square_transpose(log_block_size, chunk))?;
		}

		Ok(())
	}
}

/// The basic multilinear polynomial commitment scheme from [DP23].
///
/// The basic scheme follows Construction 3.7. In this case, the encoding alphabet is a subfield of
/// the polynomial's coefficient field.
pub type BasicTensorPCS<P, PA, PE, LC, H, VCS> = TensorPCS<P, PA, P, PE, LC, H, VCS>;

/// The multilinear polynomial commitment scheme from [DP23] with block-level encoding.
///
/// The basic scheme follows Construction 3.11. In this case, the encoding alphabet is an extension
/// field of the polynomial's coefficient field.
pub type BlockTensorPCS<P, PA, PE, LC, H, VCS> = TensorPCS<P, PA, PA, PE, LC, H, VCS>;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		challenger::HashChallenger,
		field::{
			PackedBinaryField128x1b, PackedBinaryField16x8b, PackedBinaryField1x128b,
			PackedBinaryField4x32b,
		},
		polynomial::multilinear_query::MultilinearQuery,
		reed_solomon::reed_solomon::ReedSolomonCode,
	};
	use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};
	use std::iter::repeat_with;

	#[test]
	fn test_simple_commit_prove_verify_without_error() {
		type Packed = PackedBinaryField16x8b;

		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs =
			<BasicTensorPCS<Packed, Packed, PackedBinaryField1x128b, _, _, _>>::new_using_groestl_merkle_tree(4, rs_code).unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let evals = repeat_with(|| Packed::random(&mut rng))
			.take((1 << pcs.n_vars()) / Packed::WIDTH)
			.collect::<Vec<_>>();
		let poly = MultilinearExtension::from_values(evals).unwrap();
		let polys = vec![&poly];

		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();

		let multilin_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();
		let value = poly.evaluate(&multilin_query).unwrap();
		let values = vec![value];

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}

	#[test]
	fn test_simple_commit_prove_verify_batch_without_error() {
		type Packed = PackedBinaryField16x8b;

		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs =
			<BasicTensorPCS<Packed, Packed, PackedBinaryField1x128b, _, _, _>>::new_using_groestl_merkle_tree(4, rs_code).unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let batch_size = thread_rng().gen_range(1..=10);
		let polys = repeat_with(|| {
			let evals = repeat_with(|| Packed::random(&mut rng))
				.take((1 << pcs.n_vars()) / Packed::WIDTH)
				.collect::<Vec<_>>();
			MultilinearExtension::from_values(evals).unwrap()
		})
		.take(batch_size)
		.collect::<Vec<_>>();
		let polys = polys.iter().collect::<Vec<_>>();

		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();
		let multilin_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();

		let values = polys
			.iter()
			.map(|poly| poly.evaluate(&multilin_query).unwrap())
			.collect::<Vec<_>>();

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}

	#[test]
	fn test_packed_1b_commit_prove_verify_without_error() {
		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs = <BlockTensorPCS<
			PackedBinaryField128x1b,
			PackedBinaryField16x8b,
			PackedBinaryField1x128b,
			_,
			_,
			_,
		>>::new_using_groestl_merkle_tree(8, rs_code)
		.unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let evals = repeat_with(|| PackedBinaryField128x1b::random(&mut rng))
			.take((1 << pcs.n_vars()) / PackedBinaryField128x1b::WIDTH)
			.collect::<Vec<_>>();
		let poly = MultilinearExtension::from_values(evals).unwrap();
		let polys = vec![&poly];

		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();

		let multilin_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();
		let value = poly.evaluate(&multilin_query).unwrap();
		let values = vec![value];

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}

	#[test]
	fn test_packed_1b_commit_prove_verify_batch_without_error() {
		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs = <BlockTensorPCS<
			PackedBinaryField128x1b,
			PackedBinaryField16x8b,
			PackedBinaryField1x128b,
			_,
			_,
			_,
		>>::new_using_groestl_merkle_tree(8, rs_code)
		.unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let batch_size = thread_rng().gen_range(1..=10);
		let polys = repeat_with(|| {
			let evals = repeat_with(|| PackedBinaryField128x1b::random(&mut rng))
				.take((1 << pcs.n_vars()) / PackedBinaryField128x1b::WIDTH)
				.collect::<Vec<_>>();
			MultilinearExtension::from_values(evals).unwrap()
		})
		.take(batch_size)
		.collect::<Vec<_>>();
		let polys = polys.iter().collect::<Vec<_>>();
		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();
		let multilinear_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();

		let values = polys
			.iter()
			.map(|poly| poly.evaluate(&multilinear_query).unwrap())
			.collect::<Vec<_>>();

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}

	#[test]
	fn test_packed_32b_commit_prove_verify_without_error() {
		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs = <BasicTensorPCS<
			PackedBinaryField4x32b,
			PackedBinaryField16x8b,
			PackedBinaryField1x128b,
			_,
			_,
			_,
		>>::new_using_groestl_merkle_tree(8, rs_code)
		.unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let evals = repeat_with(|| PackedBinaryField4x32b::random(&mut rng))
			.take((1 << pcs.n_vars()) / PackedBinaryField4x32b::WIDTH)
			.collect::<Vec<_>>();
		let poly = MultilinearExtension::from_values(evals).unwrap();
		let polys = vec![&poly];

		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();

		let multilin_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();
		let value = poly.evaluate(&multilin_query).unwrap();
		let values = vec![value];

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}

	#[test]
	fn test_packed_32b_commit_prove_verify_batch_without_error() {
		let rs_code = ReedSolomonCode::new(5, 2, 12).unwrap();
		let pcs = <BasicTensorPCS<
			PackedBinaryField4x32b,
			PackedBinaryField16x8b,
			PackedBinaryField1x128b,
			_,
			_,
			_,
		>>::new_using_groestl_merkle_tree(8, rs_code)
		.unwrap();

		let mut rng = StdRng::seed_from_u64(0);
		let batch_size = thread_rng().gen_range(1..=10);
		let polys = repeat_with(|| {
			let evals = repeat_with(|| PackedBinaryField4x32b::random(&mut rng))
				.take((1 << pcs.n_vars()) / PackedBinaryField4x32b::WIDTH)
				.collect::<Vec<_>>();
			MultilinearExtension::from_values(evals).unwrap()
		})
		.take(batch_size)
		.collect::<Vec<_>>();
		let polys = polys.iter().collect::<Vec<_>>();
		let (commitment, committed) = pcs.commit(&polys).unwrap();

		let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
		let query = repeat_with(|| challenger.sample())
			.take(pcs.n_vars())
			.collect::<Vec<_>>();
		let multilin_query =
			MultilinearQuery::<PackedBinaryField1x128b>::with_full_query(&query).unwrap();

		let values = polys
			.iter()
			.map(|poly| poly.evaluate(&multilin_query).unwrap())
			.collect::<Vec<_>>();

		let mut prove_challenger = challenger.clone();
		let proof = pcs
			.prove_evaluation(&mut prove_challenger, &committed, &polys, &query)
			.unwrap();

		let mut verify_challenger = challenger.clone();
		pcs.verify_evaluation(&mut verify_challenger, &commitment, &query, proof, &values)
			.unwrap();
	}
}
