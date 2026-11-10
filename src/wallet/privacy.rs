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
use rand_core::RngCore;

use super::utils::shuffle_slice;
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

/// Maximum outputs that share a destination inside one [`DestinationGroup`].
///
/// Bitcoin Core grouped at most 10 outputs, then raised the cap to 100 in
/// <https://github.com/bitcoin/bitcoin/pull/18418>. A larger group spends a
/// reused address in fewer transactions. It also raises the fee when
/// `-avoidpartialspends` forces the whole group into the transaction.
pub const OUTPUT_GROUP_MAX_ENTRIES: usize = 100;

/// UTXOs that pay the same `scriptPubKey` and should be spent together.
///
/// Built by [`group_by_destination`]. A single script is split into further
/// groups once it reaches [`OUTPUT_GROUP_MAX_ENTRIES`]. Coin selection does not
/// consume this type yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationGroup {
    /// Script every output in [`utxos`](Self::utxos) pays.
    pub script_pubkey: ScriptBuf,
    /// Outputs in the order they were visited.
    pub utxos: Vec<WeightedUtxo>,
}

/// Group `utxos` by `scriptPubKey`, preserving first-seen order after a shuffle.
///
/// Coins are shuffled before they are grouped. Bitcoin Core does this so a large
/// set of outputs paying one destination is not chunked in wallet order, which
/// could leak that order. See <https://github.com/bitcoin/bitcoin/pull/13808>,
/// which followed the grouping added in
/// <https://github.com/bitcoin/bitcoin/pull/12257>.
///
/// Outputs that pay different scripts stay in different groups. Outputs that pay
/// the same script share a group until it reaches [`OUTPUT_GROUP_MAX_ENTRIES`];
/// further outputs of that script start another group. Within a group, outputs
/// stay in the order they are visited after the shuffle.
pub fn group_by_destination<R: RngCore>(
    utxos: impl IntoIterator<Item = WeightedUtxo>,
    rng: &mut R,
) -> Vec<DestinationGroup> {
    let mut utxos: Vec<WeightedUtxo> = utxos.into_iter().collect();
    shuffle_slice(&mut utxos, rng);
    let mut groups: Vec<DestinationGroup> = Vec::new();
    // Index of the open (not yet full) group for each script.
    let mut open: BTreeMap<ScriptBuf, usize> = BTreeMap::new();
    for utxo in utxos {
        let script_pubkey = utxo.utxo.txout().script_pubkey.clone();
        if let Some(&index) = open.get(&script_pubkey) {
            if groups[index].utxos.len() < OUTPUT_GROUP_MAX_ENTRIES {
                groups[index].utxos.push(utxo);
                continue;
            }
        }
        open.insert(script_pubkey.clone(), groups.len());
        groups.push(DestinationGroup {
            script_pubkey,
            utxos: alloc::vec![utxo],
        });
    }
    groups
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
    use rand_core::{Error as RngError, RngCore};

    use crate::{KeychainKind, LocalOutput, Utxo};

    use super::*;

    /// Yields scripted `next_u32` values so shuffle positions are fixed.
    struct SeqRng {
        values: Vec<u32>,
        index: usize,
    }

    impl RngCore for SeqRng {
        fn next_u32(&mut self) -> u32 {
            let value = self.values[self.index];
            self.index += 1;
            value
        }

        fn next_u64(&mut self) -> u64 {
            u64::from(self.next_u32())
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for byte in dest.iter_mut() {
                *byte = self.next_u32() as u8;
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    /// Draws that make [`shuffle_slice`] a no-op for `len` elements.
    fn identity_draws(len: usize) -> Vec<u32> {
        (1..len)
            .rev()
            .map(|current_index| current_index as u32)
            .collect()
    }

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
        let mut rng = SeqRng {
            values: identity_draws(3),
            index: 0,
        };
        let groups = group_by_destination(
            [
                weighted_utxo(1, script_a.clone(), Amount::from_sat(100_000_000)),
                weighted_utxo(2, script_b.clone(), Amount::from_sat(50_000_000)),
                weighted_utxo(3, script_a.clone(), Amount::from_sat(50_000_000)),
            ],
            &mut rng,
        );

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
    fn shuffles_coins_before_grouping_them() {
        let script_a = p2wpkh_script(0x11);
        let script_b = p2wpkh_script(0x22);
        // len 3: draw 0 swaps the last coin to the front, draw 1 leaves it there.
        let mut rng = SeqRng {
            values: vec![0, 1],
            index: 0,
        };
        let groups = group_by_destination(
            [
                weighted_utxo(1, script_a.clone(), Amount::from_sat(1)),
                weighted_utxo(2, script_a.clone(), Amount::from_sat(2)),
                weighted_utxo(3, script_b.clone(), Amount::from_sat(3)),
            ],
            &mut rng,
        );

        assert_eq!(groups[0].script_pubkey, script_b);
        assert_eq!(groups[1].script_pubkey, script_a);
        assert_eq!(
            groups[1]
                .utxos
                .iter()
                .map(|utxo| utxo.utxo.outpoint().txid.to_byte_array()[0])
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn caps_a_destination_at_100_outputs() {
        let script = p2wpkh_script(0x44);
        let one_hundred: Vec<_> = (0u8..100)
            .map(|index| weighted_utxo(index, script.clone(), Amount::from_sat(1_000)))
            .collect();
        let mut rng = SeqRng {
            values: identity_draws(one_hundred.len()),
            index: 0,
        };
        let groups = group_by_destination(one_hundred, &mut rng);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].utxos.len(), OUTPUT_GROUP_MAX_ENTRIES);

        let one_hundred_one: Vec<_> = (0u8..101)
            .map(|index| weighted_utxo(index, script.clone(), Amount::from_sat(1_000)))
            .collect();
        let mut rng = SeqRng {
            values: identity_draws(one_hundred_one.len()),
            index: 0,
        };
        let groups = group_by_destination(one_hundred_one, &mut rng);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].script_pubkey, script);
        assert_eq!(groups[1].script_pubkey, script);
        assert_eq!(groups[0].utxos.len(), OUTPUT_GROUP_MAX_ENTRIES);
        assert_eq!(groups[1].utxos.len(), 1);
        assert_eq!(
            groups[0].utxos[0].utxo.outpoint().txid.to_byte_array()[0],
            0
        );
        assert_eq!(
            groups[0].utxos[99].utxo.outpoint().txid.to_byte_array()[0],
            99
        );
        assert_eq!(
            groups[1].utxos[0].utxo.outpoint().txid.to_byte_array()[0],
            100
        );
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
        let mut rng = SeqRng {
            values: identity_draws(4),
            index: 0,
        };
        let groups = group_by_destination(
            [
                weighted_utxo(1, script_a.clone(), Amount::from_btc(1.0).unwrap()),
                weighted_utxo(2, script_a.clone(), Amount::from_btc(0.5).unwrap()),
                weighted_utxo(3, script_b.clone(), Amount::from_btc(1.0).unwrap()),
                weighted_utxo(4, script_b, Amount::from_btc(0.5).unwrap()),
            ],
            &mut rng,
        );
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.utxos.len() == 2));
        ensure_partial_spend_policy(PartialSpendPolicy::Always);
    }
}
