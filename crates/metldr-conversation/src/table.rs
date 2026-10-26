use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_TABLE_BYTES: usize = 262_144;

const MAX_TITLE_UTF16: usize = 300;
const MAX_COLUMN_UTF16: usize = 200;
const MAX_CELL_UTF16: usize = 2_000;
const MAX_SOURCE_UTF16: usize = 1_000;
const MAX_COLUMNS: usize = 12;
const MAX_ROWS: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Table {
    title: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    source: String,
}

impl Table {
    pub fn new(
        title: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        source: String,
    ) -> Result<Self, String> {
        if title.encode_utf16().count() > MAX_TITLE_UTF16 {
            return Err("table title exceeds its UTF-16 limit".into());
        }
        if !(1..=MAX_COLUMNS).contains(&columns.len()) {
            return Err("table must have between 1 and 12 columns".into());
        }
        if columns
            .iter()
            .any(|column| column.encode_utf16().count() > MAX_COLUMN_UTF16)
        {
            return Err("table column exceeds its UTF-16 limit".into());
        }
        if rows.len() > MAX_ROWS {
            return Err("table exceeds its row limit".into());
        }
        if rows.iter().any(|row| row.len() != columns.len()) {
            return Err("table rows must match the column count".into());
        }
        if rows
            .iter()
            .flatten()
            .any(|cell| cell.encode_utf16().count() > MAX_CELL_UTF16)
        {
            return Err("table cell exceeds its UTF-16 limit".into());
        }
        if source.encode_utf16().count() > MAX_SOURCE_UTF16 {
            return Err("table source exceeds its UTF-16 limit".into());
        }

        let table = Self {
            title,
            columns,
            rows,
            source,
        };
        if serde_json::to_vec(&table)
            .map_err(|error| format!("could not encode table: {error}"))?
            .len()
            > MAX_TABLE_BYTES
        {
            return Err("table exceeds its canonical JSON byte limit".into());
        }
        Ok(table)
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    title: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    source: String,
}

impl<'de> Deserialize<'de> for Table {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawTable::deserialize(deserializer)?;
        Self::new(raw.title, raw.columns, raw.rows, raw.source).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn source_fixture_serializes_and_deserializes() {
        let table = Table::new(
            "Quarterly sales".into(),
            vec!["Quarter".into(), "Sales".into()],
            vec![
                vec!["Q1".into(), "$100".into()],
                vec!["Q2".into(), "$250".into()],
            ],
            "User-provided data".into(),
        )
        .unwrap();
        assert_eq!(table.title(), "Quarterly sales");
        assert_eq!(table.columns(), &["Quarter", "Sales"]);
        assert_eq!(table.rows()[1], ["Q2", "$250"]);
        assert_eq!(table.source(), "User-provided data");
        let bytes = serde_json::to_vec(&table).unwrap();
        assert_eq!(serde_json::from_slice::<Table>(&bytes).unwrap(), table);
    }

    #[test]
    fn utf16_limit_counts_non_bmp_characters_as_two_units() {
        let accepted = Table::new("😀".repeat(150), vec!["c".into()], vec![], "s".into());
        assert!(accepted.is_ok());
        let rejected = Table::new("😀".repeat(151), vec!["c".into()], vec![], "s".into());
        assert!(rejected.is_err());
    }

    #[test]
    fn deserialization_rejects_bad_rows_and_unknown_fields() {
        assert!(
            serde_json::from_str::<Table>(
                r#"{"title":"t","columns":["a","b"],"rows":[["one"]],"source":"s"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Table>(
                r#"{"title":"t","columns":["a"],"rows":[],"source":"s","extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn canonical_json_bound_rejects_without_changing_caller_values() {
        let title = "t".to_owned();
        let columns = vec!["c".to_owned(); 12];
        let rows = vec![vec!["😀".repeat(2_000); 12]; 200];
        let source = "s".to_owned();
        assert!(Table::new(title.clone(), columns.clone(), rows.clone(), source.clone()).is_err());
        assert_eq!(title, "t");
        assert_eq!(columns.len(), 12);
        assert_eq!(rows.len(), 200);
        assert_eq!(source, "s");
    }
}
