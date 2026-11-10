# Wallet privacy issue 28 — classification and scaffold

Working notes for [bitcoindevkit/bdk_wallet#28](https://github.com/bitcoindevkit/bdk_wallet/issues/28)
("Wallet Privacy Improvements", opened by notmandatory). This branch scaffolds
the items that fit the current wallet API. It does not open a pull request.

Base: `bitcoindevkit/bdk_wallet` master `91d3f8e67f78713366a9e52a8a5d09e562c7cbd5`
(merge of [#423](https://github.com/bitcoindevkit/bdk_wallet/pull/423), 2026-11-06).

The stable path is `TxBuilder` / `Wallet::create_tx` with the in-tree
`CoinSelectionAlgorithm`. The 4.0 path is `PsbtParams` / `Wallet::create_psbt`,
gated by `--cfg bdk_wallet_unstable` and the `bdk-tx` feature. Tracking issue
[#632](https://github.com/bitcoindevkit/bdk_wallet/issues/632) (reez) plans to
stabilize `create_psbt`, deprecate `TxBuilder` in a 3.x, and remove it in 4.0.
notmandatory's comment on [#493](https://github.com/bitcoindevkit/bdk_wallet/pull/493)
asks for 4.0 work to land on `create_psbt` + `bdk_tx` unless 3.x users need the
feature now. New knobs in this branch exist on both builders. Defaults stay
`PartialSpendPolicy::Off` and `OutputTypeMixing::Allow`, so existing callers
keep today's selection.

## Classification

| Core | What it does | Class | Where it stands |
| --- | --- | --- | --- |
| [bitcoin#2340](https://github.com/bitcoin/bitcoin/pull/2340) | Anti-fee-sniping locktime | Already done | [bdk#611](https://github.com/bitcoindevkit/bdk/pull/611). `create_tx` sets `nLockTime` to the current height when the caller leaves it unset. `create_psbt` exposes opt-in `PsbtParams::anti_fee_sniping_height`. Leave that default alone: some protocols need `nLockTime` 0. |
| [bitcoin#12119](https://github.com/bitcoin/bitcoin/pull/12119) | P2WPKH change when a destination is P2WPKH or P2WSH | Blocked | Superseded in Core by [#23789](https://github.com/bitcoin/bitcoin/pull/23789). martinsaposnic on [#28](https://github.com/bitcoindevkit/bdk_wallet/issues/28#issuecomment-2784700022): BDK change comes from the internal descriptor, and there is no `-changetype`. Needs a change keychain chosen from the recipient script type. |
| [bitcoin#12257](https://github.com/bitcoin/bitcoin/pull/12257) | Avoid partial spends (destination groups) | Viable today — grouping implemented, selection scaffolded | [bdk#1851](https://github.com/bitcoindevkit/bdk/pull/1851) (martinsaposnic) closed unmerged on 2025-03-14 after nymius said to wait for `bdk-tx`. No follow-up PR. `group_by_destination` is real. `PartialSpendPolicy::AvoidIfFeeNeutral` and `Always` panic in `finish` / `create_psbt` until a selector spends a group as one candidate. `Off` is historical BDK and does not attempt a grouped selection. Core with the flag unset still tries the grouped selection when the fee does not rise; that is `AvoidIfFeeNeutral`. `Always` matches `-avoidpartialspends`. |
| [bitcoin#13756](https://github.com/bitcoin/bitcoin/pull/13756) | `avoid_reuse` | In progress — do not duplicate | tvpeter on [#28](https://github.com/bitcoindevkit/bdk_wallet/issues/28#issuecomment-5090268634) (2026-07-27) and open PR [#541](https://github.com/bitcoindevkit/bdk_wallet/pull/541) `feat(wallet): add TxBuilder::avoid_reuse` (branch `feat/add-avoid-reuse`, updated 2026-09-29, reviewers oleonardolima and ValuedMammal). Opt-in: unspent outputs of a `(keychain, derivation_index)` that already has a spent output become unspendable. Explicit `add_utxo` can still spend them. |
| [bitcoin#13808](https://github.com/bitcoin/bitcoin/pull/13808) | Shuffle coins before grouping | Implemented | `group_by_destination` shuffles with `shuffle_slice` before it groups, so group order and member order follow the shuffle. |
| [bitcoin#16512](https://github.com/bitcoin/bitcoin/pull/16512) | `joinpsbts` shuffles inputs and outputs | Out of wallet scope | The wallet has no join-PSBT API. Transactions this wallet builds already shuffle inputs and outputs when `TxOrdering::Shuffle` is set, and that is the default. |
| [bitcoin#16528](https://github.com/bitcoin/bitcoin/pull/16528) | Native descriptor wallets; the useful part is up to six descriptors | Blocked | [#318](https://github.com/bitcoindevkit/bdk_wallet/pull/318) (thunderbiscuit / 110CodingP) closed unmerged on 2026-10-27. Successor: open [#524](https://github.com/bitcoindevkit/bdk_wallet/pull/524) "Multi-Keychain Wallets Take 3" (base `pre-multi-keychain`, milestone Wallet 4.0.0-alpha.1). Related: [#523](https://github.com/bitcoindevkit/bdk_wallet/pull/523) "Pre multi-keychain work (do not merge)" and tracking issue [#227](https://github.com/bitcoindevkit/bdk_wallet/issues/227). ValuedMammal on #318: pick the change keychain from the output script type, and keep an internal flag on descriptors. |
| [bitcoin#17621](https://github.com/bitcoin/bitcoin/pull/17621) | A destination is used if any single-key script of the same pubkey is known (P2PKH, P2WPKH, P2SH-P2WPKH) | Blocked | [#541](https://github.com/bitcoindevkit/bdk_wallet/pull/541) keys reuse by one descriptor's derivation index. The cross-type case needs several descriptors of the same key. Follow-up once `avoid_reuse` lands and multi-keychain exists. |
| [bitcoin#18418](https://github.com/bitcoin/bitcoin/pull/18418) | `OUTPUT_GROUP_MAX_ENTRIES = 100` | Implemented | `group_by_destination` splits a destination into chunks of 100. Core raised the cap from 10 so a full group stays inside the branch-and-bound attempt budget. |
| [bitcoin#23789](https://github.com/bitcoin/bitcoin/pull/23789) | Change script type matches a payment output | Blocked | One internal descriptor today (`map_keychain(Internal)`). Same dependency as #12119 and #16528: [#524](https://github.com/bitcoindevkit/bdk_wallet/pull/524) / [#227](https://github.com/bitcoindevkit/bdk_wallet/issues/227). A refuse-to-build guard would be a different feature from Core's behavior. |
| [bitcoin#24494](https://github.com/bitcoin/bitcoin/pull/24494) | Random knapsack change target | Out of wallet scope | Payment above 25 ksat: uniform `50 ksat..=min(2*payment, 1_000_000)`; otherwise 50 ksat. SRD in Core stays at a fixed 50 ksat target. [bitcoin#25825](https://github.com/bitcoin/bitcoin/pull/25825) is the follow-up that actually wires the random value (the #24494 note says the draw was unused). BDK has no knapsack selector. Branch-and-bound avoids change through `cost_of_change`. Single random draw uses `decide_change` and the dust limit. `bdk_coin_select`'s `LowestFee` uses long-term feerate waste. Add a change target only if a knapsack selector is added. |
| [bitcoin#24584](https://github.com/bitcoin/bitcoin/pull/24584) | Avoid mixing `OutputType`s during coin selection | Viable today — classifier implemented, selection scaffolded | mapaba79 commented on [#28](https://github.com/bitcoindevkit/bdk_wallet/issues/28#issuecomment-3697496663) (2025-12-29) that they were studying it. No issue or PR from that account. `output_type` matches Core: P2PKH and P2PK are `Legacy`, P2SH is `P2sh`, P2WPKH and P2WSH are one `Bech32` class, P2TR is `Bech32m`, everything else is `Unknown`. `OutputTypeMixing::Avoid` panics until selection runs once per type, compares waste, prefers a non-default type that can pay alone, and mixes only when no single type can fund the payment. A two-descriptor wallet can already hold two script types (external and internal). |

## What this branch contains

| Commit | Item | Real vs skeleton |
| --- | --- | --- |
| `feat(wallet): scaffold destination groups for partial spends` | [#12257](https://github.com/bitcoin/bitcoin/pull/12257) | `PartialSpendPolicy`, `DestinationGroup`, and `group_by_destination` are real. Selection is `unimplemented!` for any policy other than `Off`. Ignored test states the Core 1.0 + 0.5 BTC example. |
| `feat(wallet): shuffle coins before destination grouping` | [#13808](https://github.com/bitcoin/bitcoin/pull/13808) | Implemented. The shuffle uses the existing Fisher–Yates `shuffle_slice`. |
| `feat(wallet): cap destination groups at 100 outputs` | [#18418](https://github.com/bitcoin/bitcoin/pull/18418) | Implemented. `OUTPUT_GROUP_MAX_ENTRIES = 100`. |
| `feat(wallet): scaffold avoiding mixed output types` | [#24584](https://github.com/bitcoin/bitcoin/pull/24584) | `OutputType` / `output_type` are real and tested. `OutputTypeMixing::Avoid` panics. Ignored test states the bech32-then-P2SH-change scenario. |

Public surface, always compiled (no unstable cfg):

- `bdk_wallet::privacy::{PartialSpendPolicy, DestinationGroup, OUTPUT_GROUP_MAX_ENTRIES, group_by_destination, OutputType, output_type, OutputTypeMixing}`
- `TxBuilder::partial_spend_policy` and `TxBuilder::output_type_mixing`

`PsbtParams` has the same two setters. That module compiles only with
`--cfg bdk_wallet_unstable` and `--features bdk-tx`. `create_tx`,
`create_psbt_with_rng`, and `replace_by_fee_with_rng` call the guards before
selection, so a non-default policy fails before a PSBT is built.

## What blocks the rest

1. **Grouped selection (#12257).** Neither `CoinSelectionAlgorithm` nor
   `bdk_tx::Selector` accepts a destination group as one candidate.
   `AvoidIfFeeNeutral` needs two selections (grouped and ungrouped) and a fee
   comparison. One way is to pre-bundle each group into a single candidate
   before the current selector. The other is to teach `bdk_tx` / `bdk_coin_select`
   about groups. Open [bdk-tx#82](https://github.com/bitcoindevkit/bdk-tx/pull/82)
   (evanlinjin) replaces `Selector` with one `into_selection` pass, so the
   integration point is moving.
2. **Per-type selection (#24584).** Needs waste from the selector so the
   lowest-waste single-type result wins. In-tree branch-and-bound does not
   report waste. `bdk_tx`'s `LowestFee` does, via long-term feerate. The
   fallback (mix types only when no single type can pay) is a second pass over
   the full set. Full change-type matching still waits on multi-keychain; the
   selection policy itself can run on the script types the wallet already holds.
3. **Change type and extra descriptors (#12119, #23789, #16528).** Wait for
   [#524](https://github.com/bitcoindevkit/bdk_wallet/pull/524) /
   [#227](https://github.com/bitcoindevkit/bdk_wallet/issues/227). The privacy
   behavior ValuedMammal described is: choose the change keychain from the
   recipient output type.
4. **`avoid_reuse` across script types (#17621).** Wait for
   [#541](https://github.com/bitcoindevkit/bdk_wallet/pull/541), then extend it
   once several single-key script types of the same pubkey exist.

Searches of open `bdk-tx` and `bdk_coin_select` (`bitcoindevkit/coin-select`)
pull requests found no destination groups, `avoid_reuse`, or output-type
mixing. Nearby and different:

- [coin-select#47](https://github.com/bitcoindevkit/coin-select/pull/47)
  `ChangelessWaste` (evanlinjin) — a privacy metric for changeless selection.
  [coin-select#84](https://github.com/bitcoindevkit/coin-select/pull/84) removes
  the existing changeless metric.
- [bdk-tx#47](https://github.com/bitcoindevkit/bdk-tx/pull/47) `max_weight`
  (aagbotemi, draft) — mempool weight, related to the TRUC discussion on
  [#493](https://github.com/bitcoindevkit/bdk_wallet/pull/493), not to issue 28.

## Suggested PR order

1. Leave #2340 as it is. Mention the `create_psbt` opt-in gap in the tracking
   issue when #632 moves the default path.
2. Leave #13756 to [#541](https://github.com/bitcoindevkit/bdk_wallet/pull/541).
   File #17621 as a follow-up on that PR once it merges, tied to multi-keychain.
3. Land the grouping primitive (this branch: #12257 + #13808 + #18418) as its
   own PR, still with selection unimplemented if review wants the type first.
   The next PR wires selection: pre-bundle groups or extend `bdk_tx`. Include
   the fee-neutral double selection in that second PR.
4. Land the output-type classifier (this branch: #24584) after or beside the
   grouping PR. The PR that applies `Avoid` needs a waste-reporting selector.
5. After [#524](https://github.com/bitcoindevkit/bdk_wallet/pull/524), match
   the change keychain to the recipient output type (#23789, which covers the
   intent of #12119 and the descriptor count from #16528).
6. Add a knapsack change target (#24494) only together with a knapsack
   selector.
7. Shuffle-on-join (#16512) only if a join-PSBT API appears.

## Who is working upstream

| Person | Work |
| --- | --- |
| tvpeter | [#541](https://github.com/bitcoindevkit/bdk_wallet/pull/541) `avoid_reuse` (#13756). Reviewers: oleonardolima, ValuedMammal. |
| martinsaposnic | [bdk#1851](https://github.com/bitcoindevkit/bdk/pull/1851) closed unmerged. Comments on #28 for #12119, #2340, #12257, #18418. No open follow-up. |
| nymius | Asked #1851 to wait for `bdk-tx`. |
| mapaba79 | Comment only on #24584 (2025-12-29). No PR. |
| thunderbiscuit, 110CodingP, ValuedMammal | Multi-keychain [#524](https://github.com/bitcoindevkit/bdk_wallet/pull/524), [#523](https://github.com/bitcoindevkit/bdk_wallet/pull/523), [#227](https://github.com/bitcoindevkit/bdk_wallet/issues/227). #318 is closed. |
| reez | [#632](https://github.com/bitcoindevkit/bdk_wallet/issues/632) stabilize `create_psbt`. |
| notmandatory | Filed #28. On #493, wants 4.0 features on `create_psbt` + `bdk_tx`. |
| yanfsb | [#493](https://github.com/bitcoindevkit/bdk_wallet/pull/493) TRUC vsize caps on `TxBuilder` (`feat/truc-vsize-caps`). Port to `create_psbt` is a follow-up. This scaffold does not touch that branch. |
| evanlinjin | [bdk-tx#82](https://github.com/bitcoindevkit/bdk-tx/pull/82) (selector API), [coin-select#47](https://github.com/bitcoindevkit/coin-select/pull/47) and [#84](https://github.com/bitcoindevkit/coin-select/pull/84) (changeless metric). |
