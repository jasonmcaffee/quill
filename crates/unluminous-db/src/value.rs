//! A cell, and what a column's type is called.
//!
//! **Every value arrives as the text the server printed**, and that is a decision rather than a
//! shortcut. PostgreSQL will send a column in text or in binary, and asking for binary means one
//! decoder per type OID — for `numeric`, for every `timestamp` variant, for arrays, for `jsonb`, for
//! the geometric types — each of which is a place to render a value differently from the way `psql`
//! renders it. Asking for text means the grid shows exactly what the server itself would print, and
//! Unluminous decides nothing about how a `numeric` looks.
//!
//! What the type is still needed for is four things the text alone cannot answer: whether the column
//! is right-aligned, whether an empty cell is NULL or the empty string, whether the bytes are worth
//! showing as characters at all, and what to send back when the cell is changed.

/// One cell.
///
/// NULL is a variant rather than an empty string, because they are different values and a grid that
/// drew both as nothing would be a grid nobody could trust. Everything else is the text the server
/// printed, except bytes, which have no text form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Value {
    #[default]
    Null,
    /// The text the server printed for this cell.
    Text(String),
    /// Bytes with no text form: PostgreSQL `bytea`, a SQLite blob.
    Bytes(Vec<u8>),
}

impl Value {
    /// What this cell is worth showing as, with bytes given a size and a few of their hex digits.
    ///
    /// NULL answers the empty string, because the drawing paints its own dim `NULL` rather than
    /// letting the four letters be mistaken for a value that says "NULL".
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Text(text) => text.clone(),
            Value::Bytes(bytes) => {
                let head: String =
                    bytes.iter().take(8).map(|byte| format!("{byte:02x}")).collect::<Vec<String>>().join(" ");
                match bytes.len() > 8 {
                    true => format!("{} bytes: {head}…", bytes.len()),
                    false => format!("{} bytes: {head}", bytes.len()),
                }
            }
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// The text of this cell, when it has one.
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// A cell built from what somebody typed into it.
    ///
    /// Nothing here interprets the text — an empty field is the empty string and NULL is asked for by
    /// the menu entry that sets [`Value::Null`], because a field that turned `` into NULL would make
    /// it impossible to write an empty string, and one that turned the word `null` into NULL would
    /// make it impossible to write the word.
    pub fn typed(text: impl Into<String>) -> Self {
        Value::Text(text.into())
    }
}

/// One column of a result: its name, what the server calls its type, and whether it is a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The type as the engine names it — `int4`, `timestamptz`, `TEXT`, `INTEGER`.
    pub type_name: String,
    /// Right-aligned in the grid, which is the one piece of formatting the type decides.
    pub numeric: bool,
    /// True when the column will not take a NULL, which is what the row editor checks before letting
    /// a cell be cleared.
    pub not_null: bool,
    /// True when this column is part of the table's primary key.
    pub in_key: bool,
}

impl Column {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        let type_name = type_name.into();
        let numeric = type_is_numeric(&type_name);
        Self { name: name.into(), type_name, numeric, not_null: false, in_key: false }
    }
}

/// Whether a type name means a number, for alignment.
///
/// By name rather than by OID, so the same function answers for both engines. SQLite's declared types
/// are whatever the schema said, so `DECIMAL(10,2)` and `BIGINT` both have to be recognised by their
/// prefix — which is also what SQLite's own type-affinity rules do.
pub fn type_is_numeric(type_name: &str) -> bool {
    let lower = type_name.to_ascii_lowercase();
    const NUMBERS: &[&str] = &[
        "int", "serial", "float", "double", "real", "numeric", "decimal", "money", "oid", "num",
    ];
    NUMBERS.iter().any(|kind| lower.contains(kind))
}

/// The type a PostgreSQL OID names.
///
/// The common ones by number, and anything else as `oid <n>` — which is honest rather than wrong, and
/// is only ever used for the type shown in a column header and for alignment. Nothing is decoded from
/// it, because every value arrives as text.
pub fn postgres_type_name(oid: u32) -> String {
    let known = match oid {
        16 => "bool",
        17 => "bytea",
        18 => "char",
        19 => "name",
        20 => "int8",
        21 => "int2",
        23 => "int4",
        25 => "text",
        26 => "oid",
        114 => "json",
        142 => "xml",
        700 => "float4",
        701 => "float8",
        790 => "money",
        1042 => "bpchar",
        1043 => "varchar",
        1082 => "date",
        1083 => "time",
        1114 => "timestamp",
        1184 => "timestamptz",
        1186 => "interval",
        1266 => "timetz",
        1700 => "numeric",
        2950 => "uuid",
        3802 => "jsonb",
        3614 => "tsvector",
        // The array types worth naming, because a column of them is common and `oid 1007` says
        // nothing to anybody.
        1000 => "bool[]",
        1005 => "int2[]",
        1007 => "int4[]",
        1016 => "int8[]",
        1009 => "text[]",
        1015 => "varchar[]",
        _ => return format!("oid {oid}"),
    };
    known.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_and_the_empty_string_are_different_values() {
        // The fault this exists to prevent: a grid that draws both as nothing, so nobody can tell a
        // column that has never been filled in from one that was filled in with nothing.
        assert!(Value::Null.is_null());
        assert!(!Value::Text(String::new()).is_null());
        assert_eq!(Value::Null.display(), "");
        assert_eq!(Value::Text(String::new()).display(), "");
        // Which is why the drawing paints its own mark for NULL rather than relying on the text.
    }

    #[test]
    fn bytes_are_shown_as_a_size_and_some_hex_rather_than_as_characters() {
        let short = Value::Bytes(vec![0x00, 0xff, 0x41]);
        assert_eq!(short.display(), "3 bytes: 00 ff 41");
        let long = Value::Bytes(vec![1; 40]);
        assert!(long.display().starts_with("40 bytes: 01 01"));
        assert!(long.display().ends_with('…'));
    }

    #[test]
    fn a_type_name_decides_alignment_for_both_engines() {
        for name in ["int4", "int8", "numeric", "float8", "BIGINT", "DECIMAL(10,2)", "serial"] {
            assert!(type_is_numeric(name), "{name} should be right-aligned");
        }
        for name in ["text", "varchar", "timestamptz", "uuid", "jsonb", "bool", "TEXT"] {
            assert!(!type_is_numeric(name), "{name} should not be right-aligned");
        }
    }

    #[test]
    fn an_unknown_oid_says_so_rather_than_guessing() {
        assert_eq!(postgres_type_name(23), "int4");
        assert_eq!(postgres_type_name(1184), "timestamptz");
        assert_eq!(postgres_type_name(1007), "int4[]");
        assert_eq!(postgres_type_name(987654), "oid 987654");
    }
}
