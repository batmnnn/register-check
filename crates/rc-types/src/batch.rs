use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, StringArray, TimestampSecondArray, UInt32Array, UInt64Array,
};
use arrow::record_batch::RecordBatch;

use crate::{schema, Register, Table, TypesError};

fn s<'a, T: 'a>(rows: &'a [T], f: impl Fn(&'a T) -> &'a str) -> ArrayRef {
    Arc::new(StringArray::from_iter_values(rows.iter().map(f)))
}

fn opt_s<'a, T: 'a>(rows: &'a [T], f: impl Fn(&'a T) -> Option<&'a str>) -> ArrayRef {
    Arc::new(rows.iter().map(f).collect::<StringArray>())
}

fn u64a<T>(rows: &[T], f: impl Fn(&T) -> u64) -> ArrayRef {
    Arc::new(UInt64Array::from_iter_values(rows.iter().map(f)))
}

fn u32a<T>(rows: &[T], f: impl Fn(&T) -> u32) -> ArrayRef {
    Arc::new(UInt32Array::from_iter_values(rows.iter().map(f)))
}

fn build(table: Table, columns: Vec<ArrayRef>) -> Result<RecordBatch, TypesError> {
    RecordBatch::try_new(schema(table), columns).map_err(|source| TypesError::Arrow {
        table: table.name(),
        source,
    })
}

/// Build one RecordBatch per table (always all 7, possibly with zero rows), in `Table::ALL` order.
pub fn to_batches(reg: &Register) -> Result<Vec<(Table, RecordBatch)>, TypesError> {
    Table::ALL
        .iter()
        .map(|&t| table_batch(reg, t).map(|b| (t, b)))
        .collect()
}

fn table_batch(reg: &Register, table: Table) -> Result<RecordBatch, TypesError> {
    let cols: Vec<ArrayRef> = match table {
        Table::Mints => {
            let r = &reg.mints;
            vec![
                s(r, |e| &e.mint),
                u64a(r, |e| e.slot),
                u32a(r, |e| e.tx_index),
                u32a(r, |e| e.ix_index),
                s(r, |e| &e.signature),
                u64a(r, |e| e.amount),
                u64a(r, |e| e.supply_after),
                s(r, |e| &e.authority),
            ]
        }
        Table::Burns => {
            let r = &reg.burns;
            vec![
                s(r, |e| &e.mint),
                u64a(r, |e| e.slot),
                u32a(r, |e| e.tx_index),
                u32a(r, |e| e.ix_index),
                s(r, |e| &e.signature),
                u64a(r, |e| e.amount),
                u64a(r, |e| e.supply_after),
                s(r, |e| &e.authority),
                Arc::new(BooleanArray::from_iter(
                    r.iter().map(|e| Some(e.via_permanent_delegate)),
                )),
            ]
        }
        Table::Transfers => {
            let r = &reg.transfers;
            vec![
                s(r, |e| &e.mint),
                u64a(r, |e| e.slot),
                u32a(r, |e| e.tx_index),
                u32a(r, |e| e.ix_index),
                s(r, |e| &e.signature),
                s(r, |e| &e.from_owner),
                s(r, |e| &e.to_owner),
                u64a(r, |e| e.amount),
            ]
        }
        Table::Balances => {
            let r = &reg.balances;
            vec![
                s(r, |e| &e.mint),
                s(r, |e| &e.owner),
                u64a(r, |e| e.amount),
                u64a(r, |e| e.slot),
            ]
        }
        Table::AuthorityChanges => {
            let r = &reg.authority_changes;
            vec![
                s(r, |e| &e.mint),
                u64a(r, |e| e.slot),
                u32a(r, |e| e.tx_index),
                u32a(r, |e| e.ix_index),
                s(r, |e| &e.signature),
                s(r, |e| e.change_kind.as_str()),
                opt_s(r, |e| e.old_value.as_deref()),
                opt_s(r, |e| e.new_value.as_deref()),
            ]
        }
        Table::Reserves => {
            let r = &reg.reserves;
            vec![
                s(r, |e| &e.mint),
                Arc::new(
                    TimestampSecondArray::from_iter_values(r.iter().map(|e| e.as_of_ts))
                        .with_timezone("UTC"),
                ),
                u64a(r, |e| e.reserve_amount),
                s(r, |e| &e.attestor),
                s(r, |e| &e.sig),
                s(r, |e| &e.source_url),
            ]
        }
        Table::Allowlist => {
            let r = &reg.allowlist;
            vec![s(r, |e| &e.mint), s(r, |e| &e.owner)]
        }
    };
    build(table, cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthorityChange, ChangeKind, MintEvent, Reserve};

    fn demo() -> Register {
        Register::from_json(include_str!("../../../fixtures/demo/register.json"))
            .expect("demo fixture parses")
    }

    #[test]
    fn empty_register_builds_all_seven_tables_with_zero_rows() {
        let batches = to_batches(&Register::default()).expect("builds");
        assert_eq!(batches.len(), 7);
        for (t, b) in &batches {
            assert_eq!(b.num_rows(), 0, "{}", t.name());
            assert_eq!(b.schema(), schema(*t));
        }
    }

    #[test]
    fn demo_fixture_round_trips_json_to_batches() {
        let reg = demo();
        let again = Register::from_json(&serde_json::to_string(&reg).expect("ser")).expect("de");
        assert_eq!(reg, again);

        let rows: Vec<(&str, usize)> = to_batches(&reg)
            .expect("builds")
            .iter()
            .map(|(t, b)| (t.name(), b.num_rows()))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("mints", 6),
                ("burns", 0),
                ("transfers", 1),
                ("balances", 6),
                ("authority_changes", 0),
                ("reserves", 1),
                ("allowlist", 6),
            ]
        );
    }

    #[test]
    fn u64_max_amount_survives() {
        let reg = Register {
            mints: vec![MintEvent {
                mint: "M".into(),
                slot: u64::MAX,
                tx_index: u32::MAX,
                ix_index: 0,
                signature: "S".into(),
                amount: u64::MAX,
                supply_after: u64::MAX,
                authority: "A".into(),
            }],
            ..Default::default()
        };
        let batches = to_batches(&reg).expect("builds");
        let col = batches[0]
            .1
            .column_by_name("supply_after")
            .expect("column")
            .as_any()
            .downcast_ref::<UInt64Array>()
            .expect("u64")
            .value(0);
        assert_eq!(col, u64::MAX);
    }

    #[test]
    fn nullable_authority_values_and_change_kind_strings() {
        let reg = Register {
            authority_changes: vec![AuthorityChange {
                mint: "M".into(),
                slot: 1,
                tx_index: 0,
                ix_index: 0,
                signature: "S".into(),
                change_kind: ChangeKind::TransferHookProgram,
                old_value: None,
                new_value: Some("Hook".into()),
            }],
            reserves: vec![Reserve {
                mint: "M".into(),
                as_of_ts: 1_760_000_000,
                reserve_amount: 1,
                attestor: "A".into(),
                sig: "sig".into(),
                source_url: "https://example.invalid".into(),
            }],
            ..Default::default()
        };
        let batches = to_batches(&reg).expect("builds");
        let (_, ac) = &batches[4];
        let kind = ac
            .column_by_name("change_kind")
            .expect("column")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("utf8");
        assert_eq!(kind.value(0), "transfer_hook_program");
        assert!(ac.column_by_name("old_value").expect("column").is_null(0));
    }

    #[test]
    fn unknown_table_in_json_is_rejected() {
        assert!(Register::from_json(r#"{"mintz": []}"#).is_err());
    }
}
