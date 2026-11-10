// Bitcoin Dev Kit
// Written in 2026 by Bitcoin Dev Kit Developers
//
// Copyright (c) 2020-2026 Bitcoin Dev Kit Developers
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! Wallet privacy helpers tracked by [bdk_wallet#28](https://github.com/bitcoindevkit/bdk_wallet/issues/28).
//!
//! The types here mirror Bitcoin Core behavior that can be expressed on the
//! current two-descriptor wallet. Coin selection still spends individual UTXOs;
//! policies other than the historical default panic until that selection is wired
//! through [`TxBuilder`] and [`PsbtParams`].
//!
//! [`TxBuilder`]: super::tx_builder::TxBuilder
//! [`PsbtParams`]: crate::psbt::PsbtParams

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use bitcoin::ScriptBuf;

use crate::WeightedUtxo;

/// How coin selection treats several UTXOs that pay the same script.
///
/// Bitcoin Core groups outputs that share a destination and spends that group
/// together, so a later payment cannot link a leftover output to the same
/// address. See <https://github.com/bitcoin/bitcoin/pull/12257>.
///
/// With the flag off, Core still *tries* a grouped selection when it does not
/// raise the fee. That attempt is [`PartialSpendPolicy::AvoidIfFeeNeutral`].
/// BDK keeps [`PartialSpendPolicy::Off`] as the default so existing wallets are
/// unchanged until grouped selection exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PartialSpendPolicy {
    /// Select individual UTXOs. This is the historical BDK behavior.
    #[default]
    Off,
    /// Prefer a grouped selection when its fee is not greater than the ungrouped
    /// selection. This is Bitcoin Core's default after
    /// <https://github.com/bitcoin/bitcoin/pull/12257> (`-avoidpartialspends` unset).
    AvoidIfFeeNeutral,
    /// Always spend every UTXO of a chosen destination together, in groups of at
    /// most the Core output-group cap. This is `-avoidpartialspends`.
    Always,
}

/// UTXOs that pay the same `scriptPubKey` and should be spent together.
///
/// Built by [`group_by_destination`]. Coin selection does not consume this type
/// yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationGroup {
    /// Script every output in [`utxos`](Self::utxos) pays.
    pub script_pubkey: ScriptBuf,
    /// Outputs in the order they were visited.
    pub utxos: Vec<WeightedUtxo>,
}

/// Group `utxos` by `scriptPubKey`, preserving first-seen order.
///
/// Outputs that pay different scripts stay in different groups. Outputs that pay
/// the same script stay in one group, in the order they appear in `utxos`.
///
/// This is the grouping step of <https://github.com/bitcoin/bitcoin/pull/12257>.
/// Shuffling before the group, and the cap of 100 outputs per group, are applied
/// by later changes in this module.
pub fn group_by_destination(
    utxos: impl IntoIterator<Item = WeightedUtxo>,
) -> Vec<DestinationGroup> {
    let mut order: Vec<ScriptBuf> = Vec::new();
    let mut grouped: BTreeMap<ScriptBuf, Vec<WeightedUtxo>> = BTreeMap::new();
    for utxo in utxos {
        let script_pubkey = utxo.utxo.txout().script_pubkey.clone();
        if let Some(group) = grouped.get_mut(&script_pubkey) {
            group.push(utxo);
        } else {
            order.push(script_pubkey.clone());
            grouped.insert(script_pubkey, alloc::vec![utxo]);
        }
    }
    order
        .into_iter()
        .map(|script_pubkey| {
            let utxos = grouped
                .remove(&script_pubkey)
                .expect("script was inserted when it was first seen");
            DestinationGroup {
                script_pubkey,
                utxos,
            }
        })
        .collect()
}

/// Panic unless `policy` is the historical [`PartialSpendPolicy::Off`].
///
/// Grouped selection needs either the in-tree [`CoinSelectionAlgorithm`] or the
/// `bdk_tx` [`Selector`] to spend a [`DestinationGroup`] as one candidate,
/// including the fee-neutral comparison Core runs when the flag is unset.
/// Neither selector accepts groups today.
///
/// [`CoinSelectionAlgorithm`]: super::coin_selection::CoinSelectionAlgorithm
/// [`Selector`]: https://docs.rs/bdk_tx/latest/bdk_tx/struct.Selector.html
pub(crate) fn ensure_partial_spend_policy(policy: PartialSpendPolicy) {
    match policy {
        PartialSpendPolicy::Off => {}
        PartialSpendPolicy::AvoidIfFeeNeutral | PartialSpendPolicy::Always => {
            unimplemented!(
                "partial-spend policy {policy:?} is not applied during coin selection yet; see https://github.com/bitcoin/bitcoin/pull/12257"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, OutPoint, TxOut, Txid, Weight};
    use chain::ChainPosition;

    use crate::{KeychainKind, LocalOutput, Utxo};

    use super::*;

    fn p2wpkh_script(byte: u8) -> ScriptBuf {
        let mut raw = alloc::vec![0x00, 0x14];
        raw.extend(core::iter::repeat(byte).take(20));
        ScriptBuf::from_bytes(raw)
    }

    fn weighted_utxo(index: u8, script_pubkey: ScriptBuf, value: Amount) -> WeightedUtxo {
        WeightedUtxo {
            satisfaction_weight: Weight::ZERO,
            utxo: Utxo::Local(LocalOutput {
                outpoint: OutPoint::new(Txid::from_byte_array([index; 32]), 0),
                txout: TxOut {
                    value,
                    script_pubkey,
                },
                keychain: KeychainKind::External,
                is_spent: false,
                derivation_index: u32::from(index),
                chain_position: ChainPosition::Unconfirmed {
                    first_seen: None,
                    last_seen: None,
                },
            }),
        }
    }

    #[test]
    fn groups_utxos_that_share_a_script_pubkey() {
        let script_a = p2wpkh_script(0x11);
        let script_b = p2wpkh_script(0x22);
        let groups = group_by_destination([
            weighted_utxo(1, script_a.clone(), Amount::from_sat(100_000_000)),
            weighted_utxo(2, script_b.clone(), Amount::from_sat(50_000_000)),
            weighted_utxo(3, script_a.clone(), Amount::from_sat(50_000_000)),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].script_pubkey, script_a);
        assert_eq!(
            groups[0]
                .utxos
                .iter()
                .map(|utxo| utxo.utxo.outpoint().txid.to_byte_array()[0])
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(groups[1].script_pubkey, script_b);
        assert_eq!(groups[1].utxos.len(), 1);
    }

    #[test]
    fn off_policy_does_not_change_selection() {
        ensure_partial_spend_policy(PartialSpendPolicy::Off);
    }

    /// Core example from <https://github.com/bitcoin/bitcoin/pull/12257>:
    /// 1.0 and 0.5 BTC to A, 1.0 and 0.5 BTC to B, paying 0.2 BTC.
    ///
    /// Without the policy, selection may spend only the 0.5 BTC output.
    /// [`PartialSpendPolicy::Always`] must spend both outputs of A or both of B.
    /// [`PartialSpendPolicy::AvoidIfFeeNeutral`] must do the same when the extra
    /// input does not raise the fee above the ungrouped selection.
    #[test]
    #[ignore = "coin selection does not spend a DestinationGroup atomically yet (bitcoin/bitcoin#12257)"]
    fn avoid_partial_spends_spends_the_whole_destination() {
        let script_a = p2wpkh_script(0xaa);
        let script_b = p2wpkh_script(0xbb);
        let groups = group_by_destination([
            weighted_utxo(1, script_a.clone(), Amount::from_btc(1.0).unwrap()),
            weighted_utxo(2, script_a.clone(), Amount::from_btc(0.5).unwrap()),
            weighted_utxo(3, script_b.clone(), Amount::from_btc(1.0).unwrap()),
            weighted_utxo(4, script_b, Amount::from_btc(0.5).unwrap()),
        ]);
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.utxos.len() == 2));
        ensure_partial_spend_policy(PartialSpendPolicy::Always);
    }
}
