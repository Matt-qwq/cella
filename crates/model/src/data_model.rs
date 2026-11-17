//! The data model: tables loaded from queries, the relationships between them, and the measures
//! (DAX-class formulas) defined over them.
//!
//! This is the document half of Power Pivot. A model table is a query's result **kept in memory**
//! rather than written onto a sheet: that is what makes a pivot over 2 million rows possible
//! without a sheet holding them, and what lets a measure reference a column by name. The rows are
//! cached here (as Excel caches them in `xl/model/*`) so a reopened workbook shows numbers before
//! anything is refreshed.
//!
//! The evaluation lives in `cella-dax`; this module is only the shape of the data and the
//! rules about what a valid relationship is.

use cella_core::Value;
use serde::{Deserialize, Serialize};

/// Tables one workbook may hold in its model.
pub const MAX_MODEL_TABLES: usize = 64;
/// Relationships a workbook may define.
pub const MAX_RELATIONSHIPS: usize = 256;
/// Measures a workbook may define.
pub const MAX_MEASURES: usize = 512;
/// Cells all model tables together may hold: the model lives in memory twice (once as columns,
/// once as a pivot's output), so the budget is the same 4M one query result gets.
pub const MAX_MODEL_CELLS: usize = 4_194_304;

/// A table in the model: named columns and rows of values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelTable {
    pub name: String,
    pub columns: Vec<String>,
    /// One row per record, each exactly [`ModelTable::width`] long.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<Value>>,
    /// The query this table came from, so a refresh knows what to run. `None` for a table that was
    /// put in the model by a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_query: Option<String>,
}

impl ModelTable {
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn index(&self, column: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == column).or_else(|| self.columns.iter().position(|c| c.eq_ignore_ascii_case(column)))
    }
}

/// A relationship between two model tables: the sheet's shape of Excel's many-to-one link.
///
/// Filters flow **from the "to" table to the "from" table** (`to_table` is the one side, the
/// lookup table; `from_table` is the many side, the fact table). A pivot that groups by a column of
/// the lookup table therefore filters the fact rows through the relationship, which is what makes
/// a measure over two files mean anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
}

/// A measure: a formula over the model, named, and groupable in the field list under a table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    pub name: String,
    /// The table the measure is filed under (Excel calls it the home table). It does not restrict
    /// the formula; it decides where the field list shows it.
    pub table: String,
    /// The DAX subset: `SUM(Sales[Amount])`, `CALCULATE(SUM(Sales[Amount]), Region[Name] = "West")`,
    /// `DIVIDE(...)`, arithmetic and parentheses.
    pub formula: String,
    /// A number-format code for the measure's output cells, if the workbook gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// Everything the workbook knows about its own model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DataModel {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<ModelTable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measures: Vec<Measure>,
}

impl DataModel {
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.relationships.is_empty() && self.measures.is_empty()
    }

    pub fn table(&self, name: &str) -> Option<&ModelTable> {
        self.tables.iter().find(|t| t.name == name).or_else(|| self.tables.iter().find(|t| t.name.eq_ignore_ascii_case(name)))
    }

    pub fn table_index(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| t.name == name).or_else(|| self.tables.iter().position(|t| t.name.eq_ignore_ascii_case(name)))
    }

    pub fn measure(&self, name: &str) -> Option<&Measure> {
        self.measures.iter().find(|m| m.name == name).or_else(|| self.measures.iter().find(|m| m.name.eq_ignore_ascii_case(name)))
    }

    /// The relationships that filter `table`: the ones where it is the many side.
    pub fn incoming(&self, table: &str) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter().filter(move |r| r.from_table.eq_ignore_ascii_case(table))
    }

    /// Cells the model holds, for the budget that keeps a huge import from becoming a huge model.
    pub fn cells(&self) -> usize {
        self.tables.iter().fold(0, |n: usize, t| n.saturating_add(t.width().saturating_mul(t.height())))
    }

    /// The first problem a proposed relationship has, in words a dialog can show. `None` = usable.
    ///
    /// The rules are Excel's: both tables and both columns must exist, the two ends must be one
    /// column each, a column cannot be related to itself, the pair must not repeat, and a loop that
    /// would make a filter context ambiguous is refused.
    pub fn relationship_problem(&self, r: &Relationship, except: Option<usize>) -> Option<String> {
        let Some(from) = self.table(&r.from_table) else {
            return Some(format!("The model has no table named \"{}\".", r.from_table));
        };
        let Some(to) = self.table(&r.to_table) else {
            return Some(format!("The model has no table named \"{}\".", r.to_table));
        };
        if r.from_table.eq_ignore_ascii_case(&r.to_table) && r.from_column.eq_ignore_ascii_case(&r.to_column) {
            return Some("A column can't be related to itself.".into());
        }
        if from.index(&r.from_column).is_none() {
            return Some(format!("The table \"{}\" has no column named \"{}\".", from.name, r.from_column));
        }
        if to.index(&r.to_column).is_none() {
            return Some(format!("The table \"{}\" has no column named \"{}\".", to.name, r.to_column));
        }
        if self.relationships.iter().enumerate().any(|(i, e)| {
            Some(i) != except
                && e.from_table.eq_ignore_ascii_case(&r.from_table)
                && e.from_column.eq_ignore_ascii_case(&r.from_column)
                && e.to_table.eq_ignore_ascii_case(&r.to_table)
                && e.to_column.eq_ignore_ascii_case(&r.to_column)
        }) {
            return Some("This relationship already exists.".into());
        }
        // A loop of more than two tables would need a whole path semantics (DAX's USERELATIONSHIP
        // and cross-filter directions) to stay unambiguous; two tables linked twice is the case a
        // person actually hits, and it is also ambiguous, so both are refused.
        // Filters flow `to` → `from`, so the graph's edge for a relationship is to_table →
        // from_table. The new edge closes a loop when the tables it already joins are joined the
        // other way round too: following edges out of `from_table` reaches `to_table`.
        let loop_text = "That relationship would create a loop the filter context can't resolve.";
        let mut seen = vec![r.from_table.to_ascii_lowercase()];
        let mut cur = r.from_table.to_ascii_lowercase();
        for _ in 0..=MAX_RELATIONSHIPS {
            let next = self
                .relationships
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != except)
                .find(|(_, e)| e.to_table.eq_ignore_ascii_case(&cur))
                .map(|(_, e)| e.from_table.to_ascii_lowercase())?;
            if next.eq_ignore_ascii_case(&r.to_table) || seen.contains(&next) {
                return Some(loop_text.into());
            }
            seen.push(next.clone());
            cur = next;
        }
        Some(loop_text.into())
    }

    /// Whether `name` is a usable measure or table name: not blank, at most 255 characters, and
    /// not already taken by a measure (case-insensitively, as Excel compares them).
    pub fn measure_name_is_free(&self, name: &str, except: Option<&str>) -> bool {
        let n = name.trim();
        !n.is_empty()
            && n.chars().count() <= 255
            && self.table(n).is_none()
            && !self.measures.iter().any(|m| Some(m.name.as_str()) != except && m.name.eq_ignore_ascii_case(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, columns: &[&str]) -> ModelTable {
        ModelTable { name: name.into(), columns: columns.iter().map(|c| c.to_string()).collect(), rows: Vec::new(), source_query: None }
    }

    fn model() -> DataModel {
        DataModel {
            tables: vec![table("Sales", &["RegionId", "Amount"]), table("Region", &["Id", "Name"])],
            relationships: Vec::new(),
            measures: Vec::new(),
        }
    }

    fn rel(from: &str, from_col: &str, to: &str, to_col: &str) -> Relationship {
        Relationship { from_table: from.into(), from_column: from_col.into(), to_table: to.into(), to_column: to_col.into() }
    }

    #[test]
    fn a_relationship_needs_both_columns_and_both_tables() {
        let m = model();
        assert_eq!(m.relationship_problem(&rel("Sales", "RegionId", "Region", "Id"), None), None);
        let problem = m.relationship_problem(&rel("Sales", "Nope", "Region", "Id"), None).unwrap_or_default();
        assert!(problem.contains("Nope"), "{problem}");
        let problem = m.relationship_problem(&rel("Ghost", "Id", "Region", "Id"), None).unwrap_or_default();
        assert!(!problem.is_empty());
    }

    #[test]
    fn the_same_relationship_twice_and_a_loop_are_refused() {
        let mut m = model();
        m.relationships.push(rel("Sales", "RegionId", "Region", "Id"));
        assert_eq!(m.relationship_problem(&rel("Sales", "RegionId", "Region", "Id"), None).as_deref(), Some("This relationship already exists."));
        // Region -> Sales would close the loop Sales -> Region.
        let problem = m.relationship_problem(&rel("Region", "Id", "Sales", "RegionId"), None).unwrap_or_default();
        assert!(problem.contains("loop"), "{problem}");
    }

    #[test]
    fn a_column_cannot_be_related_to_itself() {
        let m = model();
        assert!(m.relationship_problem(&rel("Sales", "Amount", "Sales", "Amount"), None).is_some());
    }

    #[test]
    fn measure_names_cannot_repeat_a_table_or_another_measure() {
        let mut m = model();
        m.measures.push(Measure { name: "Total".into(), table: "Sales".into(), formula: "SUM(Sales[Amount])".into(), format: None });
        assert!(!m.measure_name_is_free("total", None));
        assert!(m.measure_name_is_free("total", Some("Total")));
        assert!(!m.measure_name_is_free("sales", None), "a table name is taken");
        assert!(!m.measure_name_is_free("   ", None));
    }

    #[test]
    fn the_model_round_trips_through_json() {
        let mut m = model();
        m.relationships.push(rel("Sales", "RegionId", "Region", "Id"));
        m.measures.push(Measure { name: "Total".into(), table: "Sales".into(), formula: "SUM(Sales[Amount])".into(), format: Some("#,##0".into()) });
        m.tables[0].rows.push(vec![Value::number(1.0), Value::number(42.0)]);
        let text = serde_json::to_string(&m).unwrap();
        let back: DataModel = serde_json::from_str(&text).unwrap();
        assert_eq!(m, back);
    }
}
