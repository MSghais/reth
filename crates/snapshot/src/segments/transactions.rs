use crate::segments::{prepare_jar, Segment};
use reth_db::{
    cursor::DbCursorRO, database::Database, snapshot::create_snapshot_T1, table::Table, tables,
    transaction::DbTx, RawKey, RawTable,
};
use reth_interfaces::RethResult;
use reth_primitives::{
    snapshot::{Compression, Filters},
    BlockNumber, SnapshotSegment, TxNumber,
};
use reth_provider::{DatabaseProviderRO, TransactionsProviderExt};
use std::ops::RangeInclusive;

/// Snapshot segment responsible for [SnapshotSegment::Transactions] part of data.
#[derive(Debug)]
pub struct Transactions {
    compression: Compression,
    filters: Filters,
}

impl Transactions {
    /// Creates new instance of [Transactions] snapshot segment.
    pub fn new(compression: Compression, filters: Filters) -> Self {
        Self { compression, filters }
    }

    // Generates the dataset to train a zstd dictionary with the most recent rows (at most 1000).
    fn dataset_for_compression<'tx, T: Table<Key = TxNumber>>(
        &self,
        tx: &impl DbTx<'tx>,
        range: &RangeInclusive<TxNumber>,
        range_len: usize,
    ) -> RethResult<Vec<Vec<u8>>> {
        let mut cursor = tx.cursor_read::<RawTable<T>>()?;
        Ok(cursor
            .walk_back(Some(RawKey::from(*range.end())))?
            .take(range_len.min(1000))
            .map(|row| row.map(|(_key, value)| value.into_value()).expect("should exist"))
            .collect::<Vec<_>>())
    }
}

impl Segment for Transactions {
    fn snapshot<DB: Database>(
        &self,
        provider: &DatabaseProviderRO<'_, DB>,
        block_range: RangeInclusive<BlockNumber>,
    ) -> RethResult<()> {
        let range = provider.transaction_range_by_block_range(block_range)?;
        let range_len = range.clone().count();

        let mut jar = prepare_jar::<DB, 1, tables::Transactions>(
            provider,
            SnapshotSegment::Transactions,
            self.filters,
            self.compression,
            range.clone(),
            range_len,
            || {
                Ok([self.dataset_for_compression::<tables::Transactions>(
                    provider.tx_ref(),
                    &range,
                    range_len,
                )?])
            },
        )?;

        // Generate list of hashes for filters & PHF
        let mut hashes = None;
        if self.filters.has_filters() {
            hashes = Some(
                tables::Transactions::recover_hashes(provider.tx_ref(), 0..10)?
                    .into_iter()
                    .map(|(tx, _)| Ok(tx)),
            );
        }

        create_snapshot_T1::<tables::Transactions, TxNumber>(
            provider.tx_ref(),
            range,
            None,
            // We already prepared the dictionary beforehand
            None::<Vec<std::vec::IntoIter<Vec<u8>>>>,
            hashes,
            range_len,
            &mut jar,
        )?;

        Ok(())
    }
}
