//! The workbook's Get & Transform queries, in a part of our own: `xl/gridcraft/queries.json`.
//!
//! Excel keeps a query in a `customXml` DataMashup part (a proprietary compressed container).
//! Reading or writing that is out of reach of a clean-room implementation, and a guess would corrupt
//! a real workbook instead of round-tripping it, so GridCraft stores its own queries in a part of its
//! own. An extra OPC part is legal: Excel reads the parts its relationships name and leaves the rest
//! alone.
//!
//! The rules here, and why:
//!
//! - **Nothing relates to the part and `_rels` is not touched.** An unreferenced part whose content
//!   type the package declares is the quiet option — Excel has no reason to look at it — while a
//!   relationship to a part it does not understand is the kind of thing that ends in a repair
//!   prompt. `[Content_Types].xml` gains one `<Default Extension="json">`, and only when the part is
//!   written.
//! - **The model is the truth.** An empty `wb.queries` writes no part and declares no extension. A
//!   file that *had* the part leaves with none once the model has no queries: the pass-through
//!   carries only the parts a relationship reaches (`xl/gridcraft/` is in no keep list, and this part
//!   is reached by nothing), so a stale copy cannot outlive the queries it held.
//! - **The JSON is the model's own serde shape** — externally tagged enums, fields in declaration
//!   order, `null` for `None` — and `tests::queries` proves that against `serde_json` on the same
//!   `Vec<Query>`, so a change to the model's serde shape fails a test instead of quietly changing
//!   the file format. That is also why the JSON reader and writer below are hand-written: this crate
//!   takes no dependency it does not need, and `serde_json` is a *dev*-dependency here, used as that
//!   oracle.
//! - **Reading is lenient**, like the rest of the reader. No part means no queries; a part that is
//!   over `MAX_PART_BYTES`, is not JSON, or names a variant or a field the model does not have keeps
//!   no queries and becomes one warning naming the part. A broken query list never fails the open of
//!   the workbook it sits in, and a parser message is echoed shortened (a 4 KB complaint is noise in
//!   a report).

use std::fmt::Write as _;

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{
    DataModel, MAX_MEASURES, MAX_MODEL_CELLS, MAX_MODEL_TABLES, MAX_RELATIONSHIPS, Measure, ModelTable, Query, QueryAggregate, QueryColumnType,
    QueryFilterOp, QueryLoad, QuerySource, QueryStep, Relationship, Workbook,
};

use crate::IoError;
use crate::read::Ctx;
use crate::write::Out;

/// The part the queries are written to: a folder of our own, so no Excel part can collide with it.
pub(crate) const PART: &str = "xl/gridcraft/queries.json";
/// The extension the package has to declare a content type for.
pub(crate) const EXTENSION: &str = "json";
/// Its content type: plain JSON, which is exactly what the part holds.
pub(crate) const CONTENT_TYPE: &str = "application/json";

/// The most this reader will decompress and parse. The pipeline's own row and column caps are
/// elsewhere; a query list that needs more than 32 MB of JSON is a file making work, not a
/// workbook, and refusing it costs one warning.
pub(crate) const MAX_PART_BYTES: u64 = 32 * 1024 * 1024;

/// The most of a parser's message a warning echoes.
const MAX_MESSAGE: usize = 200;

/// The most queries one part may hold. Each needs a JSON object of its own, so this is far past
/// anything a person builds and short of the allocation a 32 MB part of list-shaped junk could ask
/// for.
const MAX_QUERIES: usize = 65_536;

/// The most fields in one object or items in one list, for the same reason: bounded by what a
/// person writes, not by what a hostile part could contain.
const MAX_ITEMS: usize = 1 << 20;

/// The deepest nesting the reader walks. No query nests, so anything deeper is junk — and walking
/// a million open brackets without a bound is a stack overflow, which is a crash.
const MAX_DEPTH: usize = 64;

/// Reads the workbook's queries from `PART`, if it has one: a missing, oversized, unreadable or
/// unparseable part leaves the model with no queries and one warning naming the part.
pub(crate) fn read(cx: &mut Ctx<'_>, wb: &mut Workbook) {
    let bytes = match cx.pkg.read_limited(PART, MAX_PART_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return,
        Err(IoError::TooLarge(_)) => {
            cx.warn(format!(
                "{PART}: the part is larger than {} MB and was not read, so the workbook has no queries",
                MAX_PART_BYTES / (1024 * 1024)
            ));
            return;
        }
        Err(e) => {
            cx.warn(format!("{PART}: the part could not be read ({e}), so the workbook has no queries"));
            return;
        }
    };
    match decode(&bytes) {
        Ok((queries, model)) => {
            wb.queries = queries;
            wb.model = model;
        }
        Err(why) => cx.warn(format!("{PART}: {}; the workbook has no queries", short(&why))),
    }
}

/// Writes the workbook's queries into `PART` and declares the extension's content type, or writes
/// nothing at all when the workbook has no queries (the model is the truth: no queries, no part, no
/// default).
pub(crate) fn write(wb: &Workbook, out: &mut Out) {
    if wb.queries.is_empty() && wb.model.is_empty() {
        return;
    }
    out.part(PART, None, encode(&wb.queries, &wb.model).into_bytes());
    // A kept part brings its content type as an Override, so a Default already registered is never
    // duplicated: the map keeps whichever was declared first.
    out.default_type(EXTENSION, CONTENT_TYPE);
}

/// The first `MAX_MESSAGE` bytes of `m`, on a character boundary, with an ellipsis when it was cut.
fn short(m: &str) -> String {
    if m.len() <= MAX_MESSAGE {
        return m.to_string();
    }
    let mut cut = MAX_MESSAGE;
    while cut > 0 && !m.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", m.get(..cut).unwrap_or_default())
}

// ---------------------------------------------------------------- writing

/// The queries as JSON: the model's own serde shape, written by hand (see the module doc).
fn encode(queries: &[Query], model: &DataModel) -> String {
    let mut list = String::new();
    list.push('[');
    for (i, q) in queries.iter().enumerate() {
        if i > 0 {
            list.push(',');
        }
        push_query(&mut list, q);
    }
    list.push(']');
    if model.is_empty() {
        return list;
    }
    let mut s = String::from("{\"queries\":");
    s.push_str(&list);
    s.push_str(",\"model\":");
    encode_model(&mut s, model);
    s.push('}');
    s
}

fn push_query(s: &mut String, q: &Query) {
    let _ = write!(s, "{{\"id\":{},\"name\":", q.id);
    push_text(s, &q.name);
    s.push_str(",\"source\":");
    push_source(s, &q.source);
    s.push_str(",\"steps\":[");
    for (i, step) in q.steps.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_step(s, step);
    }
    s.push_str("],\"load\":");
    push_load(s, &q.load);
    s.push_str(",\"columns\":");
    push_texts(s, &q.columns);
    s.push('}');
}

fn push_source(s: &mut String, source: &QuerySource) {
    match source {
        QuerySource::Csv { path, text, delimiter } => {
            s.push_str("{\"Csv\":{\"path\":");
            push_optional(s, path.as_deref());
            s.push_str(",\"text\":");
            push_optional(s, text.as_deref());
            s.push_str(",\"delimiter\":");
            let mut buf = [0u8; 4];
            push_text(s, delimiter.encode_utf8(&mut buf));
            s.push_str("}}");
        }
        QuerySource::Json { path, text } => {
            s.push_str("{\"Json\":{\"path\":");
            push_optional(s, path.as_deref());
            s.push_str(",\"text\":");
            push_optional(s, text.as_deref());
            s.push_str("}}");
        }
        QuerySource::Xlsx { path, sheet } => {
            s.push_str("{\"Xlsx\":{\"path\":");
            push_text(s, path);
            s.push_str(",\"sheet\":");
            push_optional(s, sheet.as_deref());
            s.push_str("}}");
        }
        QuerySource::Parquet { path } => {
            s.push_str("{\"Parquet\":{\"path\":");
            push_text(s, path);
            s.push_str("}}");
        }
        QuerySource::Folder { path, filter } => {
            s.push_str("{\"Folder\":{\"path\":");
            push_text(s, path);
            s.push_str(",\"filter\":");
            push_text(s, filter);
            s.push_str("}}");
        }
        QuerySource::Table { sheet, name } => {
            s.push_str("{\"Table\":{\"sheet\":");
            push_text(s, sheet);
            s.push_str(",\"name\":");
            push_text(s, name);
            s.push_str("}}");
        }
        QuerySource::Range { sheet, range } => {
            s.push_str("{\"Range\":{\"sheet\":");
            push_text(s, sheet);
            s.push_str(",\"range\":{\"start\":");
            push_cell(s, range.start);
            s.push_str(",\"end\":");
            push_cell(s, range.end);
            s.push_str("}}}");
        }
    }
}

/// A `CellRef`'s serde shape; [`push_source`] writes the `RangeRef` around two of them.
fn push_cell(s: &mut String, c: CellRef) {
    let _ = write!(s, "{{\"row\":{},\"col\":{}}}", c.row, c.col);
}

fn push_step(s: &mut String, step: &QueryStep) {
    match step {
        QueryStep::PromoteHeaders => s.push_str("\"PromoteHeaders\""),
        QueryStep::DemoteHeaders => s.push_str("\"DemoteHeaders\""),
        QueryStep::ChangeType { columns, to } => {
            s.push_str("{\"ChangeType\":{\"columns\":");
            push_texts(s, columns);
            s.push_str(",\"to\":");
            push_column_type(s, *to);
            s.push_str("}}");
        }
        QueryStep::RemoveColumns { columns } => {
            s.push_str("{\"RemoveColumns\":{\"columns\":");
            push_texts(s, columns);
            s.push_str("}}");
        }
        QueryStep::KeepColumns { columns } => {
            s.push_str("{\"KeepColumns\":{\"columns\":");
            push_texts(s, columns);
            s.push_str("}}");
        }
        QueryStep::RenameColumn { from, to } => {
            s.push_str("{\"RenameColumn\":{\"from\":");
            push_text(s, from);
            s.push_str(",\"to\":");
            push_text(s, to);
            s.push_str("}}");
        }
        QueryStep::Filter { column, op, value } => {
            s.push_str("{\"Filter\":{\"column\":");
            push_text(s, column);
            s.push_str(",\"op\":");
            push_filter_op(s, *op);
            s.push_str(",\"value\":");
            push_optional(s, value.as_deref());
            s.push_str("}}");
        }
        QueryStep::Sort { keys } => {
            s.push_str("{\"Sort\":{\"keys\":[");
            for (i, (column, descending)) in keys.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push('[');
                push_text(s, column);
                let _ = write!(s, ",{descending}]");
            }
            s.push_str("]}}");
        }
        QueryStep::RemoveDuplicates { columns } => {
            s.push_str("{\"RemoveDuplicates\":{\"columns\":");
            push_texts(s, columns);
            s.push_str("}}");
        }
        QueryStep::ReplaceValues { column, from, to } => {
            s.push_str("{\"ReplaceValues\":{\"column\":");
            push_text(s, column);
            s.push_str(",\"from\":");
            push_text(s, from);
            s.push_str(",\"to\":");
            push_text(s, to);
            s.push_str("}}");
        }
        QueryStep::FillDown { column } => {
            s.push_str("{\"FillDown\":{\"column\":");
            push_text(s, column);
            s.push_str("}}");
        }
        QueryStep::Unpivot { attributes, values } => {
            s.push_str("{\"Unpivot\":{\"attributes\":");
            push_texts(s, attributes);
            s.push_str(",\"values\":");
            push_texts(s, values);
            s.push_str("}}");
        }
        QueryStep::GroupBy { by, aggregates } => {
            s.push_str("{\"GroupBy\":{\"by\":");
            push_texts(s, by);
            s.push_str(",\"aggregates\":[");
            for (i, (aggregate, column, name)) in aggregates.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push('[');
                push_aggregate(s, *aggregate);
                s.push(',');
                push_optional(s, column.as_deref());
                s.push(',');
                push_text(s, name);
                s.push(']');
            }
            s.push_str("]}}");
        }
    }
}

fn push_column_type(s: &mut String, t: QueryColumnType) {
    push_text(
        s,
        match t {
            QueryColumnType::Text => "Text",
            QueryColumnType::Number => "Number",
            QueryColumnType::WholeNumber => "WholeNumber",
            QueryColumnType::Date => "Date",
            QueryColumnType::Boolean => "Boolean",
        },
    );
}

fn push_filter_op(s: &mut String, op: QueryFilterOp) {
    push_text(
        s,
        match op {
            QueryFilterOp::Equals => "Equals",
            QueryFilterOp::NotEquals => "NotEquals",
            QueryFilterOp::Contains => "Contains",
            QueryFilterOp::NotContains => "NotContains",
            QueryFilterOp::StartsWith => "StartsWith",
            QueryFilterOp::EndsWith => "EndsWith",
            QueryFilterOp::Greater => "Greater",
            QueryFilterOp::GreaterOrEqual => "GreaterOrEqual",
            QueryFilterOp::Less => "Less",
            QueryFilterOp::LessOrEqual => "LessOrEqual",
            QueryFilterOp::IsBlank => "IsBlank",
            QueryFilterOp::IsNotBlank => "IsNotBlank",
        },
    );
}

fn push_aggregate(s: &mut String, a: QueryAggregate) {
    push_text(
        s,
        match a {
            QueryAggregate::Count => "Count",
            QueryAggregate::Sum => "Sum",
            QueryAggregate::Average => "Average",
            QueryAggregate::Min => "Min",
            QueryAggregate::Max => "Max",
        },
    );
}

fn push_load(s: &mut String, load: &QueryLoad) {
    match load {
        QueryLoad::Sheet { sheet } => {
            s.push_str("{\"Sheet\":{\"sheet\":");
            push_optional(s, sheet.as_deref());
            s.push_str("}}");
        }
        QueryLoad::Model => s.push_str("\"Model\""),
    }
}

fn push_texts(s: &mut String, list: &[String]) {
    s.push('[');
    for (i, t) in list.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        push_text(s, t);
    }
    s.push(']');
}

/// `None` is JSON's `null`: that is what serde writes for an `Option::None`.
fn push_optional(s: &mut String, text: Option<&str>) {
    match text {
        Some(t) => push_text(s, t),
        None => s.push_str("null"),
    }
}

fn push_text(s: &mut String, text: &str) {
    s.push('"');
    for c in text.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\u{8}' => s.push_str("\\b"),
            '\u{c}' => s.push_str("\\f"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(s, "\\u{:04x}", c as u32);
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

// ---------------------------------------------------------------- reading

/// The queries a part holds, or the reason it holds none.
fn decode(bytes: &[u8]) -> Result<(Vec<Query>, DataModel), String> {
    let root = Parser::new(bytes).parse()?;
    // An object is the current shape ({"queries": [...], "model": {...}}); a bare list is the
    // shape the first version wrote, and still reads.
    let (list, model) = match &root {
        Json::Object(o) => {
            // An object that names neither key is junk, not an empty workbook: say so, so a
            // corrupted part still leaves the warning it always did.
            if opt_field(o, "queries").is_none() && opt_field(o, "model").is_none() {
                return Err("the part must hold a list of queries or a query object".into());
            }
            let list = match opt_field(o, "queries") {
                Some(v) => as_list(v, "the query list")?.to_vec(),
                None => Vec::new(),
            };
            let model = match opt_field(o, "model") {
                Some(v) => as_model(v)?,
                None => DataModel::default(),
            };
            (list, model)
        }
        Json::Array(_) => (as_list(&root, "the query list")?.to_vec(), DataModel::default()),
        _ => return Err("the part must hold a list of queries or a query object".into()),
    };
    let list = &list[..];
    if list.len() > MAX_QUERIES {
        return Err(format!("the part holds more than {MAX_QUERIES} queries"));
    }
    let mut queries = Vec::with_capacity(list.len());
    for (i, value) in list.iter().enumerate() {
        queries.push(as_query(value).map_err(|e| format!("query {}: {e}", i + 1))?);
    }
    Ok((queries, model))
}

fn as_query(v: &Json) -> Result<Query, String> {
    let o = as_object(v, "a query")?;
    let id = as_u32(field(o, "id", "a query")?, "a query's id")?;
    let name = as_text(field(o, "name", "a query")?, "a query's name")?;
    let source = as_source(field(o, "source", "a query")?)?;
    let steps = as_steps(field(o, "steps", "a query")?)?;
    let load = as_load(field(o, "load", "a query")?)?;
    // columns is the model's one serde(default) field: absent reads as empty, as serde has it.
    let columns = match o.iter().find(|(k, _)| k == "columns").map(|(_, v)| v) {
        Some(v) => as_texts(v, "a query's columns")?,
        None => Vec::new(),
    };
    Ok(Query { id, name, source, steps, load, columns })
}

fn as_source(v: &Json) -> Result<QuerySource, String> {
    let (name, body) = variant(v, "a query's source")?;
    let what = "the source";
    Ok(match name {
        "Csv" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Csv {
                path: as_optional(field(o, "path", what)?, "the Csv source's path")?,
                text: as_optional(field(o, "text", what)?, "the Csv source's text")?,
                delimiter: as_char(field(o, "delimiter", what)?)?,
            }
        }
        "Json" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Json {
                path: as_optional(field(o, "path", what)?, "the Json source's path")?,
                text: as_optional(field(o, "text", what)?, "the Json source's text")?,
            }
        }
        "Xlsx" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Xlsx {
                path: as_text(field(o, "path", what)?, "the Xlsx source's path")?,
                sheet: as_optional(field(o, "sheet", what)?, "the Xlsx source's sheet")?,
            }
        }
        "Parquet" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Parquet { path: as_text(field(o, "path", what)?, "the Parquet source's path")? }
        }
        "Folder" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Folder {
                path: as_text(field(o, "path", what)?, "the Folder source's path")?,
                filter: as_text(field(o, "filter", what)?, "the Folder source's filter")?,
            }
        }
        "Table" => {
            let o = as_object(details(body, name)?, what)?;
            QuerySource::Table {
                sheet: as_text(field(o, "sheet", what)?, "the Table source's sheet")?,
                name: as_text(field(o, "name", what)?, "the Table source's name")?,
            }
        }
        "Range" => {
            let o = as_object(details(body, name)?, what)?;
            let sheet = as_text(field(o, "sheet", what)?, "the Range source's sheet")?;
            let range = as_range(field(o, "range", what)?)?;
            QuerySource::Range { sheet, range }
        }
        other => return Err(format!("unknown query source \"{other}\"")),
    })
}

fn as_range(v: &Json) -> Result<RangeRef, String> {
    let o = as_object(v, "the Range source's range")?;
    let start = as_cell(field(o, "start", "the range")?, "the range's start")?;
    let end = as_cell(field(o, "end", "the range")?, "the range's end")?;
    Ok(RangeRef::new(start, end))
}

fn as_cell(v: &Json, what: &str) -> Result<CellRef, String> {
    let o = as_object(v, what)?;
    Ok(CellRef::new(as_u32(field(o, "row", what)?, "a row")?, as_u32(field(o, "col", what)?, "a column")?))
}

fn as_steps(v: &Json) -> Result<Vec<QueryStep>, String> {
    let list = as_list(v, "a query's steps")?;
    let mut steps = Vec::with_capacity(list.len().min(64));
    for value in list {
        steps.push(as_step(value)?);
    }
    Ok(steps)
}

fn as_step(v: &Json) -> Result<QueryStep, String> {
    let (name, body) = variant(v, "a step")?;
    let what = "the step";
    Ok(match name {
        "PromoteHeaders" => QueryStep::PromoteHeaders,
        "DemoteHeaders" => QueryStep::DemoteHeaders,
        "ChangeType" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::ChangeType {
                columns: as_texts(field(o, "columns", what)?, "the changed columns")?,
                to: as_column_type(field(o, "to", what)?)?,
            }
        }
        "RemoveColumns" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::RemoveColumns { columns: as_texts(field(o, "columns", what)?, "the removed columns")? }
        }
        "KeepColumns" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::KeepColumns { columns: as_texts(field(o, "columns", what)?, "the kept columns")? }
        }
        "RenameColumn" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::RenameColumn {
                from: as_text(field(o, "from", what)?, "the renamed column")?,
                to: as_text(field(o, "to", what)?, "the new name")?,
            }
        }
        "Filter" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::Filter {
                column: as_text(field(o, "column", what)?, "the filtered column")?,
                op: as_filter_op(field(o, "op", what)?)?,
                value: as_optional(field(o, "value", what)?, "the filter's value")?,
            }
        }
        "Sort" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::Sort { keys: as_sort_keys(field(o, "keys", what)?)? }
        }
        "RemoveDuplicates" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::RemoveDuplicates { columns: as_texts(field(o, "columns", what)?, "the columns of the duplicate check")? }
        }
        "ReplaceValues" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::ReplaceValues {
                column: as_text(field(o, "column", what)?, "the replaced column")?,
                from: as_text(field(o, "from", what)?, "the replaced value")?,
                to: as_text(field(o, "to", what)?, "the replacement")?,
            }
        }
        "FillDown" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::FillDown { column: as_text(field(o, "column", what)?, "the filled column")? }
        }
        "Unpivot" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::Unpivot {
                attributes: as_texts(field(o, "attributes", what)?, "the kept attributes")?,
                values: as_texts(field(o, "values", what)?, "the unpivoted values")?,
            }
        }
        "GroupBy" => {
            let o = as_object(details(body, name)?, what)?;
            QueryStep::GroupBy {
                by: as_texts(field(o, "by", what)?, "the grouping columns")?,
                aggregates: as_aggregates(field(o, "aggregates", what)?)?,
            }
        }
        other => return Err(format!("unknown step \"{other}\"")),
    })
}

/// A `Vec<(String, bool)>`: serde writes a tuple as a two-item array.
fn as_sort_keys(v: &Json) -> Result<Vec<(String, bool)>, String> {
    let list = as_list(v, "the sort keys")?;
    let mut keys = Vec::with_capacity(list.len().min(64));
    for value in list {
        let pair = as_list(value, "a sort key")?;
        match (pair.first(), pair.get(1), pair.get(2)) {
            (Some(column), Some(descending), None) => {
                keys.push((as_text(column, "a sort key's column")?, as_bool(descending, "a sort key's descending flag")?));
            }
            _ => return Err("a sort key is not a [column, descending] pair".into()),
        }
    }
    Ok(keys)
}

fn as_aggregates(v: &Json) -> Result<Vec<(QueryAggregate, Option<String>, String)>, String> {
    let list = as_list(v, "the aggregates")?;
    let mut out = Vec::with_capacity(list.len().min(64));
    for value in list {
        let parts = as_list(value, "an aggregate")?;
        match (parts.first(), parts.get(1), parts.get(2), parts.get(3)) {
            (Some(aggregate), Some(column), Some(name), None) => {
                out.push((as_aggregate(aggregate)?, as_optional(column, "an aggregate's column")?, as_text(name, "an aggregate's name")?))
            }
            _ => return Err("an aggregate is not an [aggregate, column, name] triple".into()),
        }
    }
    Ok(out)
}

fn as_column_type(v: &Json) -> Result<QueryColumnType, String> {
    let (name, _) = variant(v, "a column type")?;
    Ok(match name {
        "Text" => QueryColumnType::Text,
        "Number" => QueryColumnType::Number,
        "WholeNumber" => QueryColumnType::WholeNumber,
        "Date" => QueryColumnType::Date,
        "Boolean" => QueryColumnType::Boolean,
        other => return Err(format!("unknown column type \"{other}\"")),
    })
}

fn as_filter_op(v: &Json) -> Result<QueryFilterOp, String> {
    let (name, _) = variant(v, "a filter operator")?;
    Ok(match name {
        "Equals" => QueryFilterOp::Equals,
        "NotEquals" => QueryFilterOp::NotEquals,
        "Contains" => QueryFilterOp::Contains,
        "NotContains" => QueryFilterOp::NotContains,
        "StartsWith" => QueryFilterOp::StartsWith,
        "EndsWith" => QueryFilterOp::EndsWith,
        "Greater" => QueryFilterOp::Greater,
        "GreaterOrEqual" => QueryFilterOp::GreaterOrEqual,
        "Less" => QueryFilterOp::Less,
        "LessOrEqual" => QueryFilterOp::LessOrEqual,
        "IsBlank" => QueryFilterOp::IsBlank,
        "IsNotBlank" => QueryFilterOp::IsNotBlank,
        other => return Err(format!("unknown filter operator \"{other}\"")),
    })
}

fn as_aggregate(v: &Json) -> Result<QueryAggregate, String> {
    let (name, _) = variant(v, "an aggregate")?;
    Ok(match name {
        "Count" => QueryAggregate::Count,
        "Sum" => QueryAggregate::Sum,
        "Average" => QueryAggregate::Average,
        "Min" => QueryAggregate::Min,
        "Max" => QueryAggregate::Max,
        other => return Err(format!("unknown aggregate \"{other}\"")),
    })
}

fn as_load(v: &Json) -> Result<QueryLoad, String> {
    let (name, body) = variant(v, "a query's load")?;
    Ok(match name {
        "Sheet" => {
            let o = as_object(details(body, name)?, "the load")?;
            QueryLoad::Sheet { sheet: as_optional(field(o, "sheet", "the load")?, "the loaded sheet")? }
        }
        "Model" => QueryLoad::Model,
        other => return Err(format!("unknown load \"{other}\"")),
    })
}

/// The variant name of an externally tagged enum: "Model" for a unit variant, the single key of
/// {"Sheet": ...} for the rest, with that key's value as the body.
fn variant<'a>(v: &'a Json, what: &str) -> Result<(&'a str, Option<&'a Json>), String> {
    match v {
        Json::Str(name) => Ok((name.as_str(), None)),
        Json::Object(fields) => match fields.as_slice() {
            [(name, body)] => Ok((name.as_str(), Some(body))),
            _ => Err(format!("{what} names {} variants where one was expected", fields.len())),
        },
        _ => Err(format!("{what} is not a variant name")),
    }
}

fn details<'a>(body: Option<&'a Json>, variant: &str) -> Result<&'a Json, String> {
    body.ok_or_else(|| format!("the variant \"{variant}\" has no details"))
}

fn as_object<'a>(v: &'a Json, what: &str) -> Result<&'a [(String, Json)], String> {
    match v {
        Json::Object(fields) => Ok(fields.as_slice()),
        _ => Err(format!("{what} is not an object")),
    }
}

fn as_list<'a>(v: &'a Json, what: &str) -> Result<&'a [Json], String> {
    match v {
        Json::Array(items) => Ok(items.as_slice()),
        _ => Err(format!("{what} is not a list")),
    }
}

fn field<'a>(o: &'a [(String, Json)], name: &str, what: &str) -> Result<&'a Json, String> {
    o.iter().find(|(k, _)| k == name).map(|(_, v)| v).ok_or_else(|| format!("{what} has no \"{name}\""))
}

fn as_text(v: &Json, what: &str) -> Result<String, String> {
    match v {
        Json::Str(t) => Ok(t.clone()),
        _ => Err(format!("{what} is not text")),
    }
}

fn as_texts(v: &Json, what: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for item in as_list(v, what)? {
        out.push(as_text(item, what)?);
    }
    Ok(out)
}

fn as_optional(v: &Json, what: &str) -> Result<Option<String>, String> {
    match v {
        Json::Null => Ok(None),
        Json::Str(t) => Ok(Some(t.clone())),
        _ => Err(format!("{what} is neither text nor null")),
    }
}

fn as_u32(v: &Json, what: &str) -> Result<u32, String> {
    match v {
        Json::Number(n) => n.parse::<u32>().map_err(|_| format!("{what} is not a whole number from 0 to 4294967295")),
        _ => Err(format!("{what} is not a number")),
    }
}

fn as_bool(v: &Json, what: &str) -> Result<bool, String> {
    match v {
        Json::Bool(b) => Ok(*b),
        _ => Err(format!("{what} is not true or false")),
    }
}

fn as_char(v: &Json) -> Result<char, String> {
    let text = as_text(v, "the Csv source's delimiter")?;
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err("the Csv source's delimiter is not one character".into()),
    }
}

// ---------------------------------------------------------------- JSON values

/// A parsed JSON value, as much of one as this part needs. Numbers stay text: the model's numbers
/// are `u32`, and a detour through `f64` would turn 3 into 3.0.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// A recursive-descent JSON reader over the part's bytes. Every step is bounded (depth, item
/// count, and the part's own size), so hostile input is an error and never a crash.
struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn new(b: &'a [u8]) -> Parser<'a> {
        Parser { b, at: 0 }
    }

    fn parse(mut self) -> Result<Json, String> {
        let value = self.value(0)?;
        self.ws();
        if self.at < self.b.len() {
            return Err(format!("unexpected text after the JSON value at byte {}", self.at));
        }
        Ok(value)
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        self.ws();
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => Ok(Json::Number(self.number()?)),
            Some(_) => Err(format!("unexpected text at byte {}", self.at)),
            None => Err("the JSON ends where a value was expected".into()),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.enter(depth)?;
        self.expect(b'{')?;
        let mut fields: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(format!("a field name is missing at byte {}", self.at));
            }
            if fields.len() >= MAX_ITEMS {
                return Err(format!("an object holds more than {MAX_ITEMS} fields"));
            }
            let key = self.string()?;
            self.ws();
            self.expect(b':')?;
            let value = self.value(depth)?;
            fields.push((key, value));
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(fields));
                }
                Some(_) => return Err(format!("expected a comma or a closing brace at byte {}", self.at)),
                None => return Err("the JSON ends inside an object".into()),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.enter(depth)?;
        self.expect(b'[')?;
        let mut items: Vec<Json> = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            if items.len() >= MAX_ITEMS {
                return Err(format!("a list holds more than {MAX_ITEMS} items"));
            }
            items.push(self.value(depth)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                Some(_) => return Err(format!("expected a comma or a closing bracket at byte {}", self.at)),
                None => return Err("the JSON ends inside a list".into()),
            }
        }
    }

    fn enter(&self, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH { Err(format!("the JSON is nested more than {MAX_DEPTH} deep")) } else { Ok(()) }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        let end = self.at.saturating_add(word.len());
        if self.b.get(self.at..end) == Some(word.as_bytes()) {
            self.at = end;
            Ok(value)
        } else {
            Err(format!("expected {word} at byte {}", self.at))
        }
    }

    fn number(&mut self) -> Result<String, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        self.digits(start)?;
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits(start)?;
        }
        if self.peek().is_some_and(|c| c == b'e' || c == b'E') {
            self.at += 1;
            if self.peek().is_some_and(|c| c == b'+' || c == b'-') {
                self.at += 1;
            }
            self.digits(start)?;
        }
        let raw = self.b.get(start..self.at).ok_or_else(|| format!("a number at byte {start} is not there"))?;
        std::str::from_utf8(raw).map(str::to_string).map_err(|_| format!("a number at byte {start} is not text"))
    }

    /// At least one digit, from where the reader stands.
    fn digits(&mut self, start: usize) -> Result<(), String> {
        if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
            return Err(format!("a number is malformed at byte {start}"));
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        let mut start = self.at;
        loop {
            let Some(c) = self.peek() else { return Err("a string is not closed".into()) };
            match c {
                b'"' => {
                    out.push_str(self.text(start, self.at)?);
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(self.text(start, self.at)?);
                    self.at += 1;
                    let escape_at = self.at;
                    let Some(e) = self.peek() else { return Err("a string ends after a backslash".into()) };
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.escape()?),
                        _ => return Err(format!("a string has an unknown escape at byte {escape_at}")),
                    }
                    start = self.at;
                }
                c if c < 0x20 => return Err(format!("a string holds a control character at byte {}", self.at)),
                _ => self.at += 1,
            }
        }
    }

    /// The bytes of the string run so far, which must be UTF-8 (the part is read as bytes).
    fn text(&self, from: usize, to: usize) -> Result<&'a str, String> {
        let raw = self.b.get(from..to).ok_or_else(|| format!("a string runs past the end of the part at byte {from}"))?;
        std::str::from_utf8(raw).map_err(|_| format!("a string is not valid UTF-8 at byte {from}"))
    }

    /// The character of a backslash-u escape whose u has been consumed.
    fn escape(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        let code = if (0xD800..=0xDBFF).contains(&high) {
            // A surrogate pair is written as two escapes; half of one is not a character.
            if self.peek() == Some(b'\\') && self.b.get(self.at + 1) == Some(&b'u') {
                self.at += 2;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err("a \\u escape pair is not a surrogate pair".into());
                }
                0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
            } else {
                return Err("a \\u escape is half a surrogate pair".into());
            }
        } else {
            u32::from(high)
        };
        char::from_u32(code).ok_or_else(|| "a \\u escape is not a character".into())
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let digits = self.b.get(self.at..self.at.saturating_add(4)).ok_or_else(|| "a \\u escape is short".to_string())?;
        let text = std::str::from_utf8(digits).map_err(|_| "a \\u escape is not text".to_string())?;
        let value = u16::from_str_radix(text, 16).map_err(|_| "a \\u escape is not four hex digits".to_string())?;
        self.at += 4;
        Ok(value)
    }
}

// ---------------------------------------------------------------- the data model

/// The model's tables, relationships and measures, written beside the queries: the part holds the
/// workbook's GridCraft-only state, and a workbook that has one usually has the other.
fn encode_model(s: &mut String, m: &DataModel) {
    s.push_str("{\"tables\":[");
    for (i, t) in m.tables.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"name\":");
        push_text(s, &t.name);
        s.push_str(",\"columns\":");
        push_texts(s, &t.columns);
        s.push_str(",\"rows\":[");
        for (j, row) in t.rows.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push('[');
            for (k, v) in row.iter().enumerate() {
                if k > 0 {
                    s.push(',');
                }
                push_value(s, v);
            }
            s.push(']');
        }
        s.push_str("],\"source_query\":");
        push_optional(s, t.source_query.as_deref());
        s.push('}');
    }
    s.push_str("],\"relationships\":[");
    for (i, r) in m.relationships.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"from_table\":");
        push_text(s, &r.from_table);
        s.push_str(",\"from_column\":");
        push_text(s, &r.from_column);
        s.push_str(",\"to_table\":");
        push_text(s, &r.to_table);
        s.push_str(",\"to_column\":");
        push_text(s, &r.to_column);
        s.push('}');
    }
    s.push_str("],\"measures\":[");
    for (i, m2) in m.measures.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"name\":");
        push_text(s, &m2.name);
        s.push_str(",\"table\":");
        push_text(s, &m2.table);
        s.push_str(",\"formula\":");
        push_text(s, &m2.formula);
        s.push_str(",\"format\":");
        push_optional(s, m2.format.as_deref());
        s.push('}');
    }
    s.push_str("]}");
}

/// A cell value as JSON: a number is Rust's shortest round-tripping form (Excel's General format
/// would lose digits), a blank is null, and an error is its own tagged object so it comes back an
/// error rather than the text of one.
fn push_value(s: &mut String, v: &Value) {
    match v {
        Value::Empty => s.push_str("null"),
        Value::Number(n) => {
            if n.is_finite() {
                let _ = write!(s, "{n}");
            } else {
                // A JSON number cannot hold an infinity or a NaN; an error is what the cell was.
                s.push_str("{\"e\":\"NUM\"}");
            }
        }
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Text(t) => push_text(s, t),
        Value::Error(e) => {
            s.push_str("{\"e\":");
            push_text(s, e.as_str());
            s.push('}');
        }
        Value::Array(_) => s.push_str("null"),
    }
}

fn as_value(v: &Json) -> Result<Value, String> {
    Ok(match v {
        Json::Null => Value::Empty,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.parse::<f64>() {
            Ok(f) if f.is_finite() => Value::number(f),
            _ => Value::Error(CellError::Num),
        },
        Json::Str(s) => Value::text(s.clone()),
        Json::Object(o) => {
            let code = o.iter().find(|(k, _)| k == "e").map(|(_, v)| v).ok_or("a cell object must hold an error")?;
            let text = as_text(code, "an error code")?;
            Value::Error(CellError::parse(&text).unwrap_or(CellError::Value))
        }
        Json::Array(_) => return Err("a cell cannot be a list".into()),
    })
}

fn as_model(v: &Json) -> Result<DataModel, String> {
    let o = as_object(v, "the data model")?;
    let mut model = DataModel::default();
    if let Some(tables) = opt_field(o, "tables") {
        for (i, t) in as_list(tables, "the model's tables")?.iter().enumerate() {
            model.tables.push(as_model_table(t).map_err(|e| format!("table {}: {e}", i + 1))?);
        }
    }
    if let Some(rels) = opt_field(o, "relationships") {
        for (i, r) in as_list(rels, "the model's relationships")?.iter().enumerate() {
            model.relationships.push(as_relationship(r).map_err(|e| format!("relationship {}: {e}", i + 1))?);
        }
    }
    if let Some(measures) = opt_field(o, "measures") {
        for (i, m) in as_list(measures, "the model's measures")?.iter().enumerate() {
            model.measures.push(as_measure(m).map_err(|e| format!("measure {}: {e}", i + 1))?);
        }
    }
    if model.tables.len() > MAX_MODEL_TABLES {
        return Err(format!("the model holds more than {MAX_MODEL_TABLES} tables"));
    }
    if model.relationships.len() > MAX_RELATIONSHIPS {
        return Err(format!("the model holds more than {MAX_RELATIONSHIPS} relationships"));
    }
    if model.measures.len() > MAX_MEASURES {
        return Err(format!("the model holds more than {MAX_MEASURES} measures"));
    }
    if model.cells() > MAX_MODEL_CELLS {
        return Err(format!("the model holds more than {MAX_MODEL_CELLS} cells"));
    }
    Ok(model)
}

fn as_model_table(v: &Json) -> Result<ModelTable, String> {
    let o = as_object(v, "a model table")?;
    let name = as_text(field(o, "name", "a model table")?, "a model table's name")?;
    let columns = as_texts(field(o, "columns", "a model table")?, "a model table's columns")?;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    if let Some(list) = opt_field(o, "rows") {
        for (i, row) in as_list(list, "a model table's rows")?.iter().enumerate() {
            let cells = as_list(row, "a model table's row")?;
            if cells.len() > columns.len() {
                return Err(format!("row {} holds more cells than the table has columns", i + 1));
            }
            let mut out = Vec::with_capacity(columns.len());
            for c in cells {
                out.push(as_value(c)?);
            }
            out.resize(columns.len(), Value::Empty);
            rows.push(out);
            if rows.len() > gridcraft_core::MAX_ROWS as usize {
                return Err("the table holds more rows than a sheet".into());
            }
        }
    }
    let source_query = match opt_field(o, "source_query") {
        Some(v) => as_optional(v, "a model table's query")?,
        None => None,
    };
    Ok(ModelTable { name, columns, rows, source_query })
}

fn as_relationship(v: &Json) -> Result<Relationship, String> {
    let o = as_object(v, "a relationship")?;
    Ok(Relationship {
        from_table: as_text(field(o, "from_table", "a relationship")?, "a relationship's from_table")?,
        from_column: as_text(field(o, "from_column", "a relationship")?, "a relationship's from_column")?,
        to_table: as_text(field(o, "to_table", "a relationship")?, "a relationship's to_table")?,
        to_column: as_text(field(o, "to_column", "a relationship")?, "a relationship's to_column")?,
    })
}

fn as_measure(v: &Json) -> Result<Measure, String> {
    let o = as_object(v, "a measure")?;
    let format = match opt_field(o, "format") {
        Some(v) => as_optional(v, "a measure's format")?,
        None => None,
    };
    Ok(Measure {
        name: as_text(field(o, "name", "a measure")?, "a measure's name")?,
        table: as_text(field(o, "table", "a measure")?, "a measure's table")?,
        formula: as_text(field(o, "formula", "a measure")?, "a measure's formula")?,
        format,
    })
}

/// A field that may be absent (the model's own serde defaults match: absent = empty).
fn opt_field<'a>(o: &'a [(String, Json)], name: &str) -> Option<&'a Json> {
    o.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}
