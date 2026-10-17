// Mede apenas a restauração do indexer: estado sparse conhecido versus materialização dense; não inclui descoberta nem recuperação da wallet.

use std::hint::black_box;

use bdk_chain::{
    bitcoin::ScriptBuf,
    keychain_txout::{ChangeSet, KeychainTxOutIndex},
    miniscript::{Descriptor, DescriptorPublicKey},
};
use bdk_wallet::KeychainKind;
use criterion::{
    measurement::WallTime, AxisScale, BatchSize, BenchmarkGroup, BenchmarkId, Criterion,
    PlotConfiguration,
};

use crate::fixture::{descriptor, new_index, restore_cases, BenchCase};

type Index = KeychainTxOutIndex<KeychainKind>;

type SparseOutput = (Index, ChangeSet);

type DenseOutput = (Index, Vec<(u32, ScriptBuf)>, ChangeSet);

fn do_sparse_restore(mut index: Index, case: &BenchCase) -> SparseOutput {
    let changeset = index
        .restore_keychain_state(
            case.metadata.keychain(),
            case.metadata.last_revealed(),
            case.metadata.used_indexes().iter().copied(),
        )
        .expect("sparse restore must succeed");

    (index, changeset)
}

fn do_dense_restore(mut index: Index, case: &BenchCase) -> DenseOutput {
    let (revealed_spks, changeset) = index
        .reveal_to_target(case.metadata.keychain(), case.last_revealed())
        .expect("dense restore must succeed");

    for &used_index in case.metadata.used_indexes() {
        let _ = index.mark_used(case.metadata.keychain(), used_index);
    }

    (index, revealed_spks, changeset)
}

fn validate_last_revealed(index: &Index, case: &BenchCase) {
    assert_eq!(
        index.last_revealed_index(case.metadata.keychain()),
        case.metadata.last_revealed(),
    );
}

fn validate_sparse_state(index: &Index, case: &BenchCase) {
    validate_last_revealed(index, case);

    let known_used = index
        .known_used_keychain_indexes(case.metadata.keychain())
        .collect::<Vec<_>>();

    assert_eq!(known_used.as_slice(), case.metadata.used_indexes(),);
}

fn validate_case(case: &BenchCase, descriptor: &Descriptor<DescriptorPublicKey>) {
    assert!(
        case.stop_gap > 0,
        "fixture stop-gap must be greater than zero",
    );

    let (sparse_index, _changeset) = do_sparse_restore(new_index(case, descriptor), case);

    validate_sparse_state(&sparse_index, case);

    assert_eq!(
        sparse_index.inner().all_spks().len(),
        case.expected_sparse_materialized(),
    );

    let (dense_index, revealed_spks, _changeset) =
        do_dense_restore(new_index(case, descriptor), case);

    validate_last_revealed(&dense_index, case);

    assert_eq!(revealed_spks.len(), case.revealed_count(),);

    assert_eq!(
        dense_index.inner().all_spks().len(),
        case.expected_dense_materialized(),
    );

    for &used_index in case.metadata.used_indexes() {
        assert!(dense_index.is_used(case.metadata.keychain(), used_index,),);
    }
}

fn benchmark_sparse(
    group: &mut BenchmarkGroup<'_, WallTime>,
    case: &BenchCase,
    descriptor: &Descriptor<DescriptorPublicKey>,
) {
    let benchmark_id = BenchmarkId::new("sparse", case.last_revealed());

    let case = case.clone();
    let descriptor = descriptor.clone();

    group.bench_function(benchmark_id, move |bencher| {
        bencher.iter_batched(
            // Setup: excluded from the measurement.
            || new_index(&case, &descriptor),
            // Operation under measurement.
            |index| black_box(do_sparse_restore(index, &case)),
            BatchSize::SmallInput,
        );
    });
}

fn benchmark_dense(
    group: &mut BenchmarkGroup<'_, WallTime>,
    case: &BenchCase,
    descriptor: &Descriptor<DescriptorPublicKey>,
) {
    let benchmark_id = BenchmarkId::new("dense", case.last_revealed());

    let case = case.clone();
    let descriptor = descriptor.clone();

    group.bench_function(benchmark_id, move |bencher| {
        bencher.iter_batched(
            // Setup: excluded from the measurement.
            || new_index(&case, &descriptor),
            // Operation under measurement.
            |index| black_box(do_dense_restore(index, &case)),
            BatchSize::LargeInput,
        );
    });
}

pub fn benchmarks(c: &mut Criterion) {
    let descriptor = descriptor();

    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);

    let mut group = c.benchmark_group("restore/tail");
    group.plot_config(plot_config);

    for case in restore_cases() {
        // Structural validation is performed once, outside Criterion timing.
        validate_case(&case, &descriptor);

        benchmark_sparse(&mut group, &case, &descriptor);

        benchmark_dense(&mut group, &case, &descriptor);
    }

    group.finish();
}
