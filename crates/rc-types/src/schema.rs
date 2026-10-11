use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};

/// The 7 register tables. `name()` is the SQL table name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Mints,
    Burns,
    Transfers,
    Balances,
    AuthorityChanges,
    Reserves,
    Allowlist,
}

impl Table {
    pub const ALL: [Table; 7] = [
        Table::Mints,
        Table::Burns,
        Table::Transfers,
        Table::Balances,
        Table::AuthorityChanges,
        Table::Reserves,
        Table::Allowlist,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Table::Mints => "mints",
            Table::Burns => "burns",
            Table::Transfers => "transfers",
            Table::Balances => "balances",
            Table::AuthorityChanges => "authority_changes",
            Table::Reserves => "reserves",
            Table::Allowlist => "allowlist",
        }
    }
}

fn utf8(name: &str) -> Field {
    Field::new(name, DataType::Utf8, false)
}

fn u64f(name: &str) -> Field {
    Field::new(name, DataType::UInt64, false)
}

fn u32f(name: &str) -> Field {
    Field::new(name, DataType::UInt32, false)
}

pub fn schema(table: Table) -> SchemaRef {
    let fields = match table {
        Table::Mints => vec![
            utf8("mint"),
            u64f("slot"),
            u32f("tx_index"),
            u32f("ix_index"),
            utf8("signature"),
            u64f("amount"),
            u64f("supply_after"),
            utf8("authority"),
        ],
        Table::Burns => vec![
            utf8("mint"),
            u64f("slot"),
            u32f("tx_index"),
            u32f("ix_index"),
            utf8("signature"),
            u64f("amount"),
            u64f("supply_after"),
            utf8("authority"),
            Field::new("via_permanent_delegate", DataType::Boolean, false),
        ],
        Table::Transfers => vec![
            utf8("mint"),
            u64f("slot"),
            u32f("tx_index"),
            u32f("ix_index"),
            utf8("signature"),
            utf8("from_owner"),
            utf8("to_owner"),
            u64f("amount"),
        ],
        Table::Balances => vec![utf8("mint"), utf8("owner"), u64f("amount"), u64f("slot")],
        Table::AuthorityChanges => vec![
            utf8("mint"),
            u64f("slot"),
            u32f("tx_index"),
            u32f("ix_index"),
            utf8("signature"),
            utf8("change_kind"),
            Field::new("old_value", DataType::Utf8, true),
            Field::new("new_value", DataType::Utf8, true),
        ],
        Table::Reserves => vec![
            utf8("mint"),
            Field::new(
                "as_of_ts",
                DataType::Timestamp(TimeUnit::Second, Some("UTC".into())),
                false,
            ),
            u64f("reserve_amount"),
            utf8("attestor"),
            utf8("sig"),
            utf8("source_url"),
        ],
        Table::Allowlist => vec![utf8("mint"), utf8("owner")],
    };
    Arc::new(Schema::new(fields))
}
