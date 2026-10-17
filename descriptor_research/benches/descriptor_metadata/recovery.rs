// Mede a recuperação offline da wallet: metadata versus descoberta stop-gap, incluindo criação da wallet e aplicação dos outputs.

use std::{fs, hint::black_box, path::PathBuf};

use bdk_chain::miniscript::{Descriptor, DescriptorPublicKey};
use bdk_wallet::{bitcoin::Network, KeychainKind, Update, Wallet};
use criterion::{AxisScale, BatchSize, BenchmarkId, Criterion, PlotConfiguration};

use crate::{
    fixture::{recovery_cases, scan_stop_gap, GapProfile, RecoveryCase, RECOVERY_DENSITIES},
    history::{SyntheticActivity, SyntheticHistory},
};

const EXTERNAL: KeychainKind = KeychainKind::External;

pub struct RecoveryRun {
    pub wallet: Wallet,
    pub scripts_examined: usize,
}

#[derive(Debug)]
pub struct RecoveryMetrics {
    pub scripts_examined: usize,
    pub recovered_indexes: Vec<u32>,
    pub missing_indexes: Vec<u32>,
    pub last_revealed: Option<u32>,
    pub materialized_spks: usize,
    pub complete: bool,
}

fn new_wallet(case: &RecoveryCase, descriptor: &Descriptor<DescriptorPublicKey>) -> Wallet {
    Wallet::create_single(descriptor.clone())
        .network(Network::Regtest)
        .lookahead(case.lookahead)
        .use_spk_cache(false)
        .create_wallet_no_persist()
        .expect("benchmark wallet must build")
}

fn add_activity(update: &mut Update, activity: &SyntheticActivity) {
    update
        .tx_update
        .txouts
        .insert(activity.outpoint, activity.txout.clone());
}

pub fn recover_with_metadata(
    case: &RecoveryCase,
    descriptor: &Descriptor<DescriptorPublicKey>,
    history: &SyntheticHistory,
) -> RecoveryRun {
    let mut wallet = new_wallet(case, descriptor);

    wallet
        .apply_spk_metadata_sparse(&case.metadata)
        .expect("metadata must apply");

    let mut request = wallet.start_sync_with_spk_metadata_at(0).build();
    let mut update = Update::default();
    let mut scripts_examined = 0;

    for item in request.iter_spks_with_expected_txids() {
        scripts_examined += 1;

        if let Some(activity) = history.activity(&item.spk) {
            add_activity(&mut update, activity);
        }
    }

    wallet
        .apply_update(update)
        .expect("synthetic update must apply");

    RecoveryRun {
        wallet,
        scripts_examined,
    }
}

pub fn recover_with_stop_gap(
    case: &RecoveryCase,
    descriptor: &Descriptor<DescriptorPublicKey>,
    history: &SyntheticHistory,
) -> RecoveryRun {
    let mut wallet = new_wallet(case, descriptor);
    let mut request = wallet.start_full_scan_at(0).build();
    let mut update = Update::default();
    let mut scripts_examined = 0;
    let mut consecutive_empty = 0;

    // No fixture frontier is used to control this scan.
    while let Some((index, script)) = request.next_spk(EXTERNAL) {
        scripts_examined += 1;

        if let Some(activity) = history.activity(&script) {
            debug_assert_eq!(activity.index, index);
            consecutive_empty = 0;
            update.last_active_indices.insert(EXTERNAL, index);
            add_activity(&mut update, activity);
        } else {
            consecutive_empty += 1;
            if consecutive_empty == case.stop_gap {
                break;
            }
        }
    }

    wallet
        .apply_update(update)
        .expect("synthetic update must apply");

    RecoveryRun {
        wallet,
        scripts_examined,
    }
}

pub fn inspect_run(case: &RecoveryCase, run: RecoveryRun) -> RecoveryMetrics {
    let mut recovered_indexes = run
        .wallet
        .spk_index()
        .txouts()
        .filter_map(|((keychain, index), _)| (keychain == EXTERNAL).then_some(index))
        .collect::<Vec<_>>();
    recovered_indexes.sort_unstable();

    let missing_indexes = case
        .metadata
        .used_indexes()
        .iter()
        .copied()
        .filter(|index| recovered_indexes.binary_search(index).is_err())
        .collect::<Vec<_>>();

    let complete = recovered_indexes.as_slice() == case.metadata.used_indexes();

    RecoveryMetrics {
        scripts_examined: run.scripts_examined,
        last_revealed: run.wallet.derivation_index(EXTERNAL),
        materialized_spks: run.wallet.spk_index().inner().all_spks().len(),
        recovered_indexes,
        missing_indexes,
        complete,
    }
}

pub fn validate_case(
    case: &RecoveryCase,
    descriptor: &Descriptor<DescriptorPublicKey>,
    history: &SyntheticHistory,
) -> (RecoveryMetrics, RecoveryMetrics) {
    assert_eq!(history.used_count(), case.used_count());

    let metadata = inspect_run(case, recover_with_metadata(case, descriptor, history));
    let stop_gap = inspect_run(case, recover_with_stop_gap(case, descriptor, history));

    let expected = scan_stop_gap(
        case.last_revealed(),
        case.metadata.used_indexes(),
        case.stop_gap,
    );

    assert!(metadata.complete);
    assert_eq!(metadata.recovered_indexes, case.metadata.used_indexes());
    assert_eq!(metadata.scripts_examined, case.used_count());
    assert_eq!(metadata.last_revealed, case.metadata.last_revealed());
    assert_eq!(
        metadata.materialized_spks,
        case.expected_sparse_materialized(),
    );

    assert_eq!(stop_gap.scripts_examined, expected.scripts_examined);
    assert_eq!(stop_gap.recovered_indexes, expected.discovered_indexes);
    assert_eq!(stop_gap.last_revealed, expected.last_active_index);
    assert_eq!(stop_gap.complete, expected.complete);

    let expected_dense_materialized = expected
        .last_active_index
        .map_or(case.lookahead as usize, |index| {
            index as usize + 1 + case.lookahead as usize
        });
    assert_eq!(stop_gap.materialized_spks, expected_dense_materialized,);

    if case.profile == GapProfile::Bounded {
        assert!(stop_gap.complete);
    }

    if stop_gap.complete {
        assert_eq!(stop_gap.recovered_indexes, metadata.recovered_indexes);
    }

    (metadata, stop_gap)
}

fn metrics_row(case: &RecoveryCase, strategy: &str, metrics: &RecoveryMetrics) -> String {
    [
        case.profile.name().to_owned(),
        case.density_percent.to_string(),
        strategy.to_owned(),
        "applicable".to_owned(),
        case.last_revealed().to_string(),
        case.revealed_count().to_string(),
        case.used_count().to_string(),
        format!("{:.6}", case.effective_density_percent()),
        case.lookahead.to_string(),
        case.stop_gap.to_string(),
        case.maximum_internal_empty_gap().to_string(),
        metrics.scripts_examined.to_string(),
        metrics.recovered_indexes.len().to_string(),
        metrics.missing_indexes.len().to_string(),
        metrics
            .last_revealed
            .map(|index| index.to_string())
            .unwrap_or_default(),
        metrics.materialized_spks.to_string(),
        metrics.complete.to_string(),
    ]
    .join(",")
        + "\n"
}

fn not_applicable_row(profile: GapProfile, density: u32, reference: &RecoveryCase) -> String {
    // All cases in this matrix share LR, L and SG.
    let revealed = reference.revealed_count() as u64;
    let used = (revealed * u64::from(density)).div_ceil(100);

    [
        profile.name().to_owned(),
        density.to_string(),
        "both".to_owned(),
        "not_applicable".to_owned(),
        reference.last_revealed().to_string(),
        revealed.to_string(),
        used.to_string(),
        format!("{:.6}", 100.0 * used as f64 / revealed as f64),
        reference.lookahead.to_string(),
        reference.stop_gap.to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ]
    .join(",")
        + "\n"
}

pub fn benchmarks(c: &mut Criterion) {
    // Immutable preparation and validation are outside the measured closures.
    let descriptor = crate::fixture::descriptor();
    let mut csv = String::from(concat!(
        "profile,density_percent,strategy,status,LR,R,U,",
        "effective_density_percent,L,SG,max_internal_empty_gap,",
        "scripts_examined,recovered_count,missing_count,",
        "frontier,materialized_spks,complete\n",
    ));

    for profile in [GapProfile::Bounded, GapProfile::Unconstrained] {
        let cases = recovery_cases(profile);
        let reference = cases.first().expect("matrix has applicable cases");

        for density in RECOVERY_DENSITIES {
            if !cases.iter().any(|case| case.density_percent == density) {
                csv.push_str(&not_applicable_row(profile, density, reference));
            }
        }

        let mut group = c.benchmark_group(format!("recovery/density/{}", profile.name()));
        group.plot_config(PlotConfiguration::default().summary_scale(AxisScale::Linear));

        for case in cases {
            let history = SyntheticHistory::new(&case, &descriptor);
            let (metadata, stop_gap) = validate_case(&case, &descriptor, &history);

            csv.push_str(&metrics_row(&case, "metadata", &metadata));
            csv.push_str(&metrics_row(&case, "stop_gap", &stop_gap));

            group.bench_function(
                BenchmarkId::new("metadata", case.density_percent),
                |bencher| {
                    bencher.iter_batched(
                        || (),
                        |_| black_box(recover_with_metadata(&case, &descriptor, &history)),
                        BatchSize::PerIteration,
                    );
                },
            );

            group.bench_function(
                BenchmarkId::new("stop_gap", case.density_percent),
                |bencher| {
                    bencher.iter_batched(
                        || (),
                        |_| black_box(recover_with_stop_gap(&case, &descriptor, &history)),
                        BatchSize::PerIteration,
                    );
                },
            );
        }

        group.finish();
    }

    // Filesystem writes are outside all measured closures.
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&target).expect("artifact directory must create");
    fs::write(target.join("recovery-density-structure.csv"), csv)
        .expect("structural report must write");
}
