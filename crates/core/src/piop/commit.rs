// Copyright 2024 Irreducible, Inc

use super::{error::Error, util::ResizeableIndex, verify::CommitMeta};
use crate::{
	oracle::{MultilinearOracleSet, MultilinearPolyOracle},
	witness::{MultilinearExtensionIndex, MultilinearWitness},
};
use binius_field::{
	as_packed_field::{PackScalar, PackedType},
	underlier::UnderlierType,
	TowerField,
};
use binius_utils::sparse_index::SparseIndex;

/// Indexes the committed oracles in a [`MultilinearOracleSet`] and returns:
///
/// 1. a [`CommitMeta`] struct that stores information about the committed polynomials
/// 2. a sparse index mapping oracle IDs to committed IDs in the commit metadata
pub fn make_oracle_commit_meta<F: TowerField>(
	oracles: &MultilinearOracleSet<F>,
) -> Result<(CommitMeta, SparseIndex<usize>), Error> {
	// We need to construct two structures:
	//
	// 1) the commit metadata structure, which depends on the counts of the number of multilinears
	//    per number of packed variables
	// 2) a sparse index mapping oracle IDs to IDs in the commit metadata
	//
	// We will construct the two indices in two passes. On the first pass, we count the number of
	// multilinears and assign for each oracle the index of the oracle in the bucket of oracles
	// with the same number of packed variables. On the second pass, the commit metadata is
	// finalized, so we can determine the absolute indices into the commit metadata structure by
	// adding offsets.

	#[derive(Clone)]
	struct CommitIDFirstPass {
		n_packed_vars: usize,
		idx_in_bucket: usize,
	}

	// First pass: count the number of multilinears and index within buckets
	let mut first_pass_index = SparseIndex::new(oracles.size());
	let mut n_multilins_by_vars = ResizeableIndex::<usize>::new();
	for oracle in oracles.iter() {
		if let MultilinearPolyOracle::Committed { oracle_id: id, .. } = &oracle {
			let n_packed_vars = n_packed_vars_for_committed_oracle(&oracle)?;
			let n_multilins_for_vars = n_multilins_by_vars.get_mut(n_packed_vars);

			first_pass_index.set(
				*id,
				CommitIDFirstPass {
					n_packed_vars,
					idx_in_bucket: *n_multilins_for_vars,
				},
			);
			*n_multilins_for_vars += 1;
		}
	}

	let commit_meta = CommitMeta::new(n_multilins_by_vars.into_vec());

	// Second pass: use commit_meta counts to finalized indices with offsets
	let mut index = SparseIndex::new(oracles.size());
	for id in 0..oracles.size() {
		if let Some(CommitIDFirstPass {
			n_packed_vars,
			idx_in_bucket,
		}) = first_pass_index.get(id)
		{
			let offset = commit_meta.range_by_vars(*n_packed_vars).start;
			index.set(id, offset + *idx_in_bucket);
		}
	}

	Ok((commit_meta, index))
}

/// Collects the committed multilinear witnesses from the witness index and returns them in order.
///
/// During the commitment phase of the protocol, the trace polynomials are committed in a specific
/// order recorded by the commit metadata. This collects the witnesses corresponding to committed
/// multilinears and returns a vector of them in the commitment order.
///
/// ## Preconditions
///
/// * `oracle_to_commit_index` must be correctly constructed. Specifically, it must be surjective,
///   mapping at exactly one oracle to every index up to the number of committed multilinears.
pub fn collect_committed_witnesses<'a, U, F>(
	commit_meta: &CommitMeta,
	oracle_to_commit_index: &SparseIndex<usize>,
	oracles: &MultilinearOracleSet<F>,
	witness_index: &MultilinearExtensionIndex<'a, U, F>,
) -> Result<Vec<MultilinearWitness<'a, PackedType<U, F>>>, Error>
where
	U: UnderlierType + PackScalar<F>,
	F: TowerField,
{
	let mut witnesses = vec![None; commit_meta.total_multilins()];
	for oracle_id in 0..oracles.size() {
		if let Some(commit_idx) = oracle_to_commit_index.get(oracle_id) {
			witnesses[*commit_idx] = Some(witness_index.get_multilin_poly(oracle_id)?);
		}
	}
	Ok(witnesses
		.into_iter()
		.map(|witness| witness.expect("pre-condition: oracle_to_commit index is surjective"))
		.collect())
}

fn n_packed_vars_for_committed_oracle<F: TowerField>(
	oracle: &MultilinearPolyOracle<F>,
) -> Result<usize, Error> {
	let n_vars = oracle.n_vars();
	let tower_level = oracle.binary_tower_level();
	n_vars
		.checked_sub(F::TOWER_LEVEL - tower_level)
		.ok_or_else(|| Error::OracleTooSmall {
			id: oracle.id(),
			min_vars: F::TOWER_LEVEL - tower_level,
		})
}

#[cfg(test)]
mod tests {
	use super::*;
	use binius_field::BinaryField128b;

	#[test]
	fn test_make_oracle_commit_meta() {
		let mut oracles = MultilinearOracleSet::<BinaryField128b>::new();

		let batch_0_0 = oracles.add_committed_batch(8, 0);
		let batch_0_0_ids = oracles.add_committed_multiple::<2>(batch_0_0);

		let batch_0_1 = oracles.add_committed_batch(10, 0);
		let batch_0_1_ids = oracles.add_committed_multiple::<2>(batch_0_1);

		let batch_0_2 = oracles.add_committed_batch(12, 0);
		let batch_0_2_ids = oracles.add_committed_multiple::<2>(batch_0_2);

		let repeat = oracles.add_repeating(batch_0_2_ids[0], 5).unwrap();

		let batch_2_0 = oracles.add_committed_batch(8, 2);
		let batch_2_0_ids = oracles.add_committed_multiple::<2>(batch_2_0);

		let batch_2_1 = oracles.add_committed_batch(10, 2);
		let batch_2_1_ids = oracles.add_committed_multiple::<2>(batch_2_1);

		let batch_2_2 = oracles.add_committed_batch(12, 2);
		let batch_2_2_ids = oracles.add_committed_multiple::<2>(batch_2_2);

		let (commit_meta, index) = make_oracle_commit_meta(&oracles).unwrap();
		assert_eq!(commit_meta.n_multilins_by_vars(), &[0, 2, 0, 4, 0, 4, 0, 2]);
		assert_eq!(index.get(batch_0_0_ids[0]).cloned(), Some(0));
		assert_eq!(index.get(batch_0_0_ids[1]).cloned(), Some(1));
		assert_eq!(index.get(batch_0_1_ids[0]).cloned(), Some(2));
		assert_eq!(index.get(batch_0_1_ids[1]).cloned(), Some(3));
		assert_eq!(index.get(batch_0_2_ids[0]).cloned(), Some(6));
		assert_eq!(index.get(batch_0_2_ids[1]).cloned(), Some(7));
		assert_eq!(index.get(batch_2_0_ids[0]).cloned(), Some(4));
		assert_eq!(index.get(batch_2_0_ids[1]).cloned(), Some(5));
		assert_eq!(index.get(batch_2_1_ids[0]).cloned(), Some(8));
		assert_eq!(index.get(batch_2_1_ids[1]).cloned(), Some(9));
		assert_eq!(index.get(batch_2_2_ids[0]).cloned(), Some(10));
		assert_eq!(index.get(batch_2_2_ids[1]).cloned(), Some(11));
		assert_eq!(index.get(repeat).cloned(), None);
	}
}