// Copyright (c) 2022, Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, path::Path};

use rocksdb::Options;
use sui_types::{
    base_types::TransactionDigest,
    batch::TxSequenceNumber,
    error::SuiError,
    waypoint::{Waypoint, WaypointDiff},
};
use typed_store::{
    reopen,
    rocks::{open_cf_opts, DBMap, TypedStoreError},
    Map,
};

pub type CheckpointSequenceNumber = u64;

#[derive(Clone)]
pub struct CheckpointProposal<K> {
    /// Name of the authority
    name: K,

    /// The sequence number of this proposal
    sequence_number: CheckpointSequenceNumber,

    /// A way point is a commitment to the set of transactions
    /// included in this proposal.
    waypoint: Waypoint,
    /// The transactions included in the proposal.
    /// TODO: only include a commitment by default.
    transactions: Vec<TransactionDigest>,
}

impl<K> CheckpointProposal<K>
where
    K: Clone,
{
    /// Create a proposal for a checkpoint at a partiular height
    /// This contains a sequence number, waypoint and a list of
    /// proposed trasnactions.
    /// TOOD: Add an identifier for the proposer, probably
    ///       an AuthorityName.
    pub fn new(
        name: K,
        sequence_number: CheckpointSequenceNumber,
        transactions: Vec<TransactionDigest>,
    ) -> Self {
        let mut waypoint = Waypoint::new(sequence_number);
        transactions.iter().for_each(|tx| {
            waypoint.insert(tx);
        });

        CheckpointProposal {
            name,
            sequence_number,
            waypoint,
            transactions,
        }
    }

    /// Returns the sequence number of this proposal
    pub fn sequence_number(&self) -> &CheckpointSequenceNumber {
        &self.sequence_number
    }

    /// Construct a Diff structure between this proposal and another
    /// proposal. A diff structure has to contain keys (TODO: down the
    /// line include AuthorityName in the proposals). The diff represents
    /// the elements that each proposal need to be augmented by to
    /// contain the same elements.
    ///
    /// TODO: down the line we can include other methods to get diffs
    /// line MerkleTrees or IBLT filters that do not require O(n) download
    /// of both proposals.
    pub fn diff_with(
        &self,
        other_proposal: &CheckpointProposal<K>,
    ) -> WaypointDiff<K, TransactionDigest> {
        let all_elements = self
            .transactions
            .iter()
            .chain(other_proposal.transactions.iter())
            .collect::<HashSet<_>>();

        let my_transactions = self.transactions.iter().collect();
        let iter_missing_me = all_elements.difference(&my_transactions).map(|x| **x);
        let other_transactions = other_proposal.transactions.iter().collect();
        let iter_missing_ot = all_elements.difference(&other_transactions).map(|x| **x);

        WaypointDiff::new(
            self.name.clone(),
            self.waypoint.clone(),
            iter_missing_me,
            other_proposal.name.clone(),
            other_proposal.waypoint.clone(),
            iter_missing_ot,
        )
    }
}

pub struct CheckpointStore<K> {
    /// The list of all transactions that are checkpointed mapping to the checkpoint
    /// sequence number they were assigned to.
    pub transactions_to_checkpoint:
        DBMap<TransactionDigest, (CheckpointSequenceNumber, TxSequenceNumber)>,

    /// The mapping from checkpoint to transactions contained within the checkpoint.
    /// The second part of the key is the local sequence number if the transaction was
    /// processed or Max(u64) / 2 + offset if not. It allows the authority to store and serve
    /// checkpoints in a causal order that can be processed in order. (Note the set
    /// of transactions in the checkpoint is global but not the order.)
    pub checkpoint_contents: DBMap<(CheckpointSequenceNumber, TxSequenceNumber), TransactionDigest>,

    /// The set of pending transactions that were included in the last checkpoint
    /// but that this authority has not yet processed.
    pub unprocessed_transactions: DBMap<TransactionDigest, CheckpointSequenceNumber>,

    /// The set of transactions this authority has processed but have not yet been
    /// included in a checkpoint, and their sequence number in the local sequence
    /// of this authority.
    pub extra_transactions: DBMap<TransactionDigest, TxSequenceNumber>,

    /// The local sequence at which the proposal for the next checkpoint is created
    /// This is a sequence number containing all unprocessed trasnactions lower than
    /// this sequence number. At this point the unprocessed_transactions sequence
    /// should be empty. It is none if there is no active proposal. We also include here
    /// the proposal, although we could re-create it from the database.
    proposal_checkpoint: Option<(TxSequenceNumber, CheckpointProposal<K>)>,
}

impl<K> CheckpointStore<K>
where
    K: Clone,
{
    pub fn open<P: AsRef<Path>>(path: P, db_options: Option<Options>) -> CheckpointStore<K> {
        let mut options = db_options.unwrap_or_default();

        /* The table cache is locked for updates and this determines the number
           of shareds, ie 2^10. Increase in case of lock contentions.
        */
        let row_cache = rocksdb::Cache::new_lru_cache(1_000_000).expect("Cache is ok");
        options.set_row_cache(&row_cache);
        options.set_table_cache_num_shard_bits(10);
        options.set_compression_type(rocksdb::DBCompressionType::None);

        let mut point_lookup = options.clone();
        point_lookup.optimize_for_point_lookup(1024 * 1024);
        point_lookup.set_memtable_whole_key_filtering(true);

        let transform = rocksdb::SliceTransform::create("bytes_8_to_16", |key| &key[8..16], None);
        point_lookup.set_prefix_extractor(transform);
        point_lookup.set_memtable_prefix_bloom_ratio(0.2);

        let db = open_cf_opts(
            &path,
            Some(options.clone()),
            &[
                ("transactions_to_checkpoint", &point_lookup),
                ("checkpoint_contents", &options),
                ("unprocessed_transactions", &point_lookup),
                ("extra_transactions", &point_lookup),
            ],
        )
        .expect("Cannot open DB.");

        let (
            transactions_to_checkpoint,
            checkpoint_contents,
            unprocessed_transactions,
            extra_transactions,
        ) = reopen! (
            &db,
            "transactions_to_checkpoint";<TransactionDigest,(CheckpointSequenceNumber, TxSequenceNumber)>,
            "checkpoint_contents";<(CheckpointSequenceNumber,TxSequenceNumber),TransactionDigest>,
            "unprocessed_transactions";<TransactionDigest,CheckpointSequenceNumber>,
            "extra_transactions";<TransactionDigest,TxSequenceNumber>
        );
        CheckpointStore {
            transactions_to_checkpoint,
            checkpoint_contents,
            unprocessed_transactions,
            extra_transactions,
            proposal_checkpoint: None,
        }
    }

    /// Set the next checkpoint proposal.
    pub fn set_proposal(&mut self, name: K) -> Result<CheckpointProposal<K>, SuiError> {
        // Check that:
        // - there is no current proposal.
        // - there are no unprocessed transactions.
        // - there are some extra transactions to include.

        if self.proposal_checkpoint.is_some() {
            return Err(SuiError::GenericAuthorityError {
                error: "Proposal already set.".to_string(),
            });
        }

        if self.unprocessed_transactions.iter().count() > 0 {
            return Err(SuiError::GenericAuthorityError {
                error: "Cannot propose with unprocessed trasnactions from the previous checkpoint."
                    .to_string(),
            });
        }

        if self.extra_transactions.iter().count() == 0 {
            return Err(SuiError::GenericAuthorityError {
                error: "Cannot propose an empty set.".to_string(),
            });
        }

        // Include the sequence number of all extra trasnactions not already in a
        // checkpoint. And make a list of the transactions.
        let sequence_number = self.next_checkpoint_sequence();
        let next_local_tx_sequence = self.extra_transactions.values().max().unwrap() + 1;
        let transactions: Vec<_> = self.extra_transactions.keys().collect();

        let ckp = CheckpointProposal::new(name, sequence_number, transactions);

        self.proposal_checkpoint = Some((next_local_tx_sequence, ckp.clone()));

        Ok(ckp)
    }

    /// Get the current proposal or error if there is no current proposal
    pub fn get_proposal(&self) -> Result<CheckpointProposal<K>, SuiError> {
        self.proposal_checkpoint
            .as_ref()
            .ok_or_else(|| SuiError::GenericAuthorityError {
                error: "No checkpoint proposal found.".to_string(),
            })
            .map(|x| x.1.clone())
    }

    /// Return the seq number of the last checkpoint we have recorded.
    pub fn next_checkpoint_sequence(&self) -> CheckpointSequenceNumber {
        self.checkpoint_contents
            .iter()
            .last()
            .map(|((seq, _), _)| seq + 1)
            .unwrap_or_else(|| 0)
    }

    /// Returns the lowest checkpoint sequence number with unprocessed transactions
    /// if any, otherwise the next checkpoint (not seen).
    pub fn lowest_unprocessed_sequence(&self) -> CheckpointSequenceNumber {
        self.unprocessed_transactions
            .iter()
            .map(|(_, chk_seq)| chk_seq)
            .min()
            .unwrap_or_else(|| self.next_checkpoint_sequence())
    }

    /// Add transactions associated with a new checkpoint in the structure, and
    /// updates all tables including unprocessed and extra transactions.
    pub fn update_new_checkpoint(
        &mut self,
        seq: CheckpointSequenceNumber,
        transactions: &[TransactionDigest],
    ) -> Result<(), SuiError> {
        // Check that this checkpoint seq is new, and directly follows the last
        // highest checkpoint seen. First checkpoint is always zero.
        let expected_seq = self.next_checkpoint_sequence();

        if seq != expected_seq {
            return Err(SuiError::CheckpointingError {
                error: "Unexpected checkpoint sequence number.".to_string(),
            });
        }

        // Reset the proposal, it should already have been used by this
        // point or not included in the checkpoint. Either way it is stale.
        self.proposal_checkpoint = None;

        // Process transactions not already in a checkpoint
        let new_transactions = self
            .transactions_to_checkpoint
            .multi_get(transactions.iter())?
            .into_iter()
            .zip(transactions.iter())
            .filter_map(
                |(opt_seq, tx)| {
                    if opt_seq.is_none() {
                        Some(*tx)
                    } else {
                        None
                    }
                },
            )
            .collect::<Vec<_>>();

        let high_seq = u64::MAX / 2;
        let transactions_with_seq = self.extra_transactions.multi_get(new_transactions.iter())?;

        let batch = self.transactions_to_checkpoint.batch();

        // Update the unprocessed transactions
        let batch = batch.insert_batch(
            &self.unprocessed_transactions,
            transactions_with_seq
                .iter()
                .zip(new_transactions.iter())
                .filter_map(
                    |(opt, tx)| {
                        if opt.is_none() {
                            Some((tx, seq))
                        } else {
                            None
                        }
                    },
                ),
        )?;

        // Delete the extra transactions now used
        let batch = batch.delete_batch(
            &self.extra_transactions,
            transactions_with_seq
                .iter()
                .zip(new_transactions.iter())
                .filter_map(|(opt, tx)| if opt.is_some() { Some(tx) } else { None }),
        )?;

        // Now write the checkpoint data to the database
        //
        // All unknown sequence numbers are replaced with high sequence number
        // of u64::max / 2 and greater.

        let checkpoint_data: Vec<_> = new_transactions
            .iter()
            .zip(transactions_with_seq.iter())
            .enumerate()
            .map(|(i, (tx, opt))| {
                let iseq = opt.unwrap_or(i as u64 + high_seq);
                ((seq, iseq), *tx)
            })
            .collect();

        let batch = batch.insert_batch(
            &self.transactions_to_checkpoint,
            checkpoint_data.iter().map(|(a, b)| (b, a)),
        )?;

        let batch = batch.insert_batch(&self.checkpoint_contents, checkpoint_data.into_iter())?;

        // Write to the database.
        batch.write()?;

        Ok(())
    }

    /// Updates the store on the basis of transactions that have been processed. This is idempotent
    /// and nothing unsafe happens if it is called twice. Returns the lowest checkpoint number with
    /// unprocessed transactions (this is the low watermark).
    pub fn update_processed_transactions(
        &mut self, // We take by &mut to prevent concurrent access.
        transactions: &[(TxSequenceNumber, TransactionDigest)],
    ) -> Result<CheckpointSequenceNumber, TypedStoreError> {
        let in_checkpoint = self
            .transactions_to_checkpoint
            .multi_get(transactions.iter().map(|(_, tx)| tx))?;

        let batch = self.transactions_to_checkpoint.batch();

        // If the transactions were in a checkpoint but we had not processed them yet, then
        // we delete them from the unprocessed transaction set.
        let batch = batch.delete_batch(
            &self.unprocessed_transactions,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(
                    |((_seq, tx), in_chk)| {
                        if in_chk.is_some() {
                            Some(tx)
                        } else {
                            None
                        }
                    },
                ),
        )?;

        // Delete the entries with the old sequence numbers
        let batch = batch.delete_batch(
            &self.transactions_to_checkpoint,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(
                    |((_seq, tx), in_chk)| {
                        if in_chk.is_some() {
                            Some(tx)
                        } else {
                            None
                        }
                    },
                ),
        )?;

        let batch = batch.delete_batch(
            &self.checkpoint_contents,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(|((_seq, _tx), in_chk)| {
                    if in_chk.is_some() {
                        Some(in_chk.unwrap())
                    } else {
                        None
                    }
                }),
        )?;

        // Update the entry to the transactions_to_checkpoint

        let batch = batch.insert_batch(
            &self.transactions_to_checkpoint,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(|((seq, tx), in_chk)| {
                    if in_chk.is_some() {
                        Some((tx, (in_chk.unwrap().0, *seq)))
                    } else {
                        None
                    }
                }),
        )?;

        // Update the checkpoint local sequence number
        let batch = batch.insert_batch(
            &self.checkpoint_contents,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(|((seq, tx), in_chk)| {
                    if in_chk.is_some() {
                        Some(((in_chk.unwrap().0, *seq), tx))
                    } else {
                        None
                    }
                }),
        )?;

        // If the transactions processed did not belong to a checkpoint yet, we add them to the list
        // of `extra` trasnactions, that we should be activelly propagating to others.
        let batch = batch.insert_batch(
            &self.extra_transactions,
            transactions
                .iter()
                .zip(&in_checkpoint)
                .filter_map(|((seq, tx), in_chk)| {
                    if in_chk.is_none() {
                        Some((tx, seq))
                    } else {
                        None
                    }
                }),
        )?;

        // Write to the database.
        batch.write()?;

        Ok(self.lowest_unprocessed_sequence())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::authority_tests::max_files_authority_tests;
    use std::{collections::HashSet, env, fs, path::PathBuf};
    use sui_types::{base_types::ObjectID, waypoint::GlobalCheckpoint};

    fn random_ckpoint_store<K>() -> (PathBuf, CheckpointStore<K>)
    where
        K: Clone,
    {
        let dir = env::temp_dir();
        let path = dir.join(format!("SC_{:?}", ObjectID::random()));
        fs::create_dir(&path).unwrap();

        // Create an authority
        let mut opts = rocksdb::Options::default();
        opts.set_max_open_files(max_files_authority_tests());

        let cps = CheckpointStore::open(path.clone(), Some(opts));
        (path, cps)
    }

    #[test]
    fn make_checkpoint_db() {
        let (_, mut cps) = random_ckpoint_store::<u64>();

        let t1 = TransactionDigest::random();
        let t2 = TransactionDigest::random();
        let t3 = TransactionDigest::random();
        let t4 = TransactionDigest::random();
        let t5 = TransactionDigest::random();
        let t6 = TransactionDigest::random();

        cps.update_processed_transactions(&[(1, t1), (2, t2), (3, t3)])
            .unwrap();
        assert!(cps.checkpoint_contents.iter().count() == 0);
        assert!(cps.extra_transactions.iter().count() == 3);
        assert!(cps.unprocessed_transactions.iter().count() == 0);

        assert!(cps.next_checkpoint_sequence() == 0);

        cps.update_new_checkpoint(0, &[t1, t2, t4, t5]).unwrap();
        assert!(cps.checkpoint_contents.iter().count() == 4);
        assert_eq!(cps.extra_transactions.iter().count(), 1);
        assert!(cps.unprocessed_transactions.iter().count() == 2);

        assert_eq!(cps.lowest_unprocessed_sequence(), 0);

        let (_cp_seq, tx_seq) = cps.transactions_to_checkpoint.get(&t4).unwrap().unwrap();
        assert!(tx_seq >= u64::MAX / 2);

        assert!(cps.next_checkpoint_sequence() == 1);

        cps.update_processed_transactions(&[(4, t4), (5, t5), (6, t6)])
            .unwrap();
        assert!(cps.checkpoint_contents.iter().count() == 4);
        assert_eq!(cps.extra_transactions.iter().count(), 2); // t3 & t6
        assert!(cps.unprocessed_transactions.iter().count() == 0);

        assert_eq!(cps.lowest_unprocessed_sequence(), 1);

        let (_cp_seq, tx_seq) = cps.transactions_to_checkpoint.get(&t4).unwrap().unwrap();
        assert_eq!(tx_seq, 4);
    }

    #[test]
    fn make_proposals() {
        let (_, mut cps1) = random_ckpoint_store::<u64>();
        let (_, mut cps2) = random_ckpoint_store::<u64>();
        let (_, mut cps3) = random_ckpoint_store::<u64>();
        let (_, mut cps4) = random_ckpoint_store::<u64>();

        let t1 = TransactionDigest::random();
        let t2 = TransactionDigest::random();
        let t3 = TransactionDigest::random();
        let t4 = TransactionDigest::random();
        let t5 = TransactionDigest::random();
        // let t6 = TransactionDigest::random();

        cps1.update_processed_transactions(&[(1, t2), (2, t3)])
            .unwrap();

        cps2.update_processed_transactions(&[(1, t1), (2, t2)])
            .unwrap();

        cps3.update_processed_transactions(&[(1, t3), (2, t4)])
            .unwrap();

        cps4.update_processed_transactions(&[(1, t4), (2, t5)])
            .unwrap();

        let p1 = cps1.set_proposal(1).unwrap();
        let p2 = cps2.set_proposal(2).unwrap();
        let p3 = cps3.set_proposal(3).unwrap();

        let ckp_items: Vec<_> = p1
            .transactions
            .into_iter()
            .chain(p2.transactions.into_iter())
            .chain(p3.transactions.into_iter())
            .collect();

        cps1.update_new_checkpoint(0, &ckp_items[..]).unwrap();
        cps2.update_new_checkpoint(0, &ckp_items[..]).unwrap();
        cps3.update_new_checkpoint(0, &ckp_items[..]).unwrap();
        cps4.update_new_checkpoint(0, &ckp_items[..]).unwrap();

        assert!(
            cps4.unprocessed_transactions.keys().collect::<HashSet<_>>()
                == [t1, t2, t3].into_iter().collect::<HashSet<_>>()
        );

        assert!(
            cps4.extra_transactions.keys().collect::<HashSet<_>>()
                == [t5].into_iter().collect::<HashSet<_>>()
        );
    }

    #[test]
    fn make_diffs() {
        let (_, mut cps1) = random_ckpoint_store();
        let (_, mut cps2) = random_ckpoint_store();
        let (_, mut cps3) = random_ckpoint_store();
        let (_, mut cps4) = random_ckpoint_store();

        let t1 = TransactionDigest::random();
        let t2 = TransactionDigest::random();
        let t3 = TransactionDigest::random();
        let t4 = TransactionDigest::random();
        let t5 = TransactionDigest::random();
        // let t6 = TransactionDigest::random();

        cps1.update_processed_transactions(&[(1, t2), (2, t3)])
            .unwrap();

        cps2.update_processed_transactions(&[(1, t1), (2, t2)])
            .unwrap();

        cps3.update_processed_transactions(&[(1, t3), (2, t4)])
            .unwrap();

        cps4.update_processed_transactions(&[(1, t4), (2, t5)])
            .unwrap();

        let p1 = cps1.set_proposal(1).unwrap();
        let p2 = cps2.set_proposal(2).unwrap();
        let p3 = cps3.set_proposal(3).unwrap();
        let p4 = cps4.set_proposal(4).unwrap();

        let diff12 = p1.diff_with(&p2);
        let diff23 = p2.diff_with(&p3);

        let mut global = GlobalCheckpoint::<i32, TransactionDigest>::new(0);
        global.insert(diff12.clone()).unwrap();
        global.insert(diff23).unwrap();

        // P4 proposal not selected
        let diff41 = p4.diff_with(&p1);
        let all_items4 = global
            .checkpoint_items(diff41, p4.transactions.iter().cloned().collect())
            .unwrap();

        // P1 proposal selected
        let all_items1 = global
            .checkpoint_items(diff12, p1.transactions.iter().cloned().collect())
            .unwrap();

        // All get the same set for the proposal
        assert_eq!(all_items1, all_items4);
    }
}
