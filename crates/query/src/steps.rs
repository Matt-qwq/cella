//! One function per step, applied to a [`Table`](crate::Table).

use std::collections::HashMap;

use cella_core::Value;
use cella_model::{QueryAggregate, QueryStep};

use crate::{QueryError, Table, cell_text, compare, convert, matches};

/// Applies one step. `index` is its place in the pipeline, which is what the error names.
pub(crate) fn apply_step(mut table: Table, step: &QueryStep, index: usize) -> Result<Table, QueryError> {
    match step {
        QueryStep::PromoteHeaders => promote(&mut table, index, step),
        QueryStep::DemoteHeaders => demote(&mut table, index, step),
        QueryStep::ChangeType { columns, to } => change_type(&mut table, index, step, columns, *to),
        QueryStep::RemoveColumns { columns } => remove_columns(&mut table, index, step, columns),
        QueryStep::KeepColumns { columns } => keep_columns(&mut table, index, step, columns),
        QueryStep::RenameColumn { from, to } => rename(&mut table, index, step, from, to),
        QueryStep::Filter { column, op, value } => filter(&mut table, index, step, column, *op, value.as_deref()),
        QueryStep::Sort { keys } => sort(&mut table, index, step, keys),
        QueryStep::RemoveDuplicates { columns } => remove_duplicates(&mut table, index, step, columns),
        QueryStep::ReplaceValues { column, from, to } => replace(&mut table, index, step, column, from, to),
        QueryStep::FillDown { column } => fill_down(&mut table, index, step, column),
        QueryStep::Unpivot { attributes, values } => unpivot(&mut table, index, step, attributes, values),
        QueryStep::GroupBy { by, aggregates } => group_by(&mut table, index, step, by, aggregates),
    }
    .map(|()| table)
}

fn column_index(table: &Table, step: &QueryStep, index: usize, name: &str) -> Result<usize, QueryError> {
    table.index(name).ok_or_else(|| QueryError::NoColumn { index, step: step.label(), column: name.to_string() })
}

fn invalid(index: usize, step: &QueryStep, reason: impl Into<String>) -> QueryError {
    QueryError::Invalid { index, step: step.label(), reason: reason.into() }
}

fn promote(table: &mut Table, index: usize, step: &QueryStep) -> Result<(), QueryError> {
    if table.rows.is_empty() {
        return Err(invalid(index, step, "there is no row to promote: the table has no rows"));
    }
    let Some(first) = table.rows.first().cloned() else {
        return Err(invalid(index, step, "there is no row to promote: the table has no rows"));
    };
    let names: Vec<String> = (0..table.width()).map(|i| first.get(i).map(cell_text).unwrap_or_default()).collect();
    table.columns = crate::unique_headers(names);
    table.rows.remove(0);
    Ok(())
}

fn demote(table: &mut Table, index: usize, step: &QueryStep) -> Result<(), QueryError> {
    if table.width() == 0 {
        return Err(invalid(index, step, "there are no columns to demote"));
    }
    let header: Vec<Value> = table.columns.iter().map(|c| Value::text(c.clone())).collect();
    table.rows.insert(0, header);
    table.columns = crate::unique_headers((0..table.width()).map(|_| String::new()).collect());
    Ok(())
}

fn change_type(table: &mut Table, index: usize, step: &QueryStep, columns: &[String], to: cella_model::QueryColumnType) -> Result<(), QueryError> {
    let targets: Vec<usize> = if columns.is_empty() {
        (0..table.width()).collect()
    } else {
        columns.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?
    };
    for row in &mut table.rows {
        for &c in &targets {
            if let Some(cell) = row.get_mut(c) {
                *cell = convert(cell, to);
            }
        }
    }
    Ok(())
}

fn remove_columns(table: &mut Table, index: usize, step: &QueryStep, columns: &[String]) -> Result<(), QueryError> {
    let mut drop: Vec<usize> = columns.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?;
    drop.sort_unstable();
    drop.dedup();
    if drop.len() >= table.width() && !table.columns.is_empty() {
        return Err(invalid(index, step, "removing every column would leave nothing to load"));
    }
    // Remove from the right so the earlier indices stay valid.
    for &c in drop.iter().rev() {
        table.columns.remove(c);
        for row in &mut table.rows {
            if c < row.len() {
                row.remove(c);
            }
        }
    }
    Ok(())
}

fn keep_columns(table: &mut Table, index: usize, step: &QueryStep, columns: &[String]) -> Result<(), QueryError> {
    if columns.is_empty() {
        return Err(invalid(index, step, "no column was kept"));
    }
    let mut keep: Vec<usize> = columns.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?;
    keep.sort_unstable();
    keep.dedup();
    let mut names = Vec::with_capacity(keep.len());
    for &c in &keep {
        names.push(table.columns.get(c).cloned().unwrap_or_default());
    }
    let mut rows = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        rows.push(keep.iter().map(|&c| row.get(c).cloned().unwrap_or(Value::Empty)).collect());
    }
    table.columns = names;
    table.rows = rows;
    Ok(())
}

fn rename(table: &mut Table, index: usize, step: &QueryStep, from: &str, to: &str) -> Result<(), QueryError> {
    let c = column_index(table, step, index, from)?;
    let name = to.trim();
    if name.is_empty() {
        return Err(invalid(index, step, "the new name is blank"));
    }
    if name.chars().count() > cella_model::MAX_NAME {
        return Err(invalid(index, step, format!("the new name is longer than {} characters", cella_model::MAX_NAME)));
    }
    if let Some(other) = table.columns.iter().position(|x| x.eq_ignore_ascii_case(name))
        && other != c
    {
        return Err(invalid(index, step, format!("the table already has a column named \"{name}\"")));
    }
    if let Some(slot) = table.columns.get_mut(c) {
        *slot = name.to_string();
    }
    Ok(())
}

fn filter(
    table: &mut Table,
    index: usize,
    step: &QueryStep,
    column: &str,
    op: cella_model::QueryFilterOp,
    value: Option<&str>,
) -> Result<(), QueryError> {
    let c = column_index(table, step, index, column)?;
    let keeps: Vec<bool> = table.rows.iter().map(|r| r.get(c).is_some_and(|v| matches(v, op, value))).collect();
    let mut iter = keeps.into_iter();
    table.rows.retain(|_| iter.next().unwrap_or(false));
    Ok(())
}

fn sort(table: &mut Table, index: usize, step: &QueryStep, keys: &[(String, bool)]) -> Result<(), QueryError> {
    if keys.is_empty() {
        return Err(invalid(index, step, "no sort key was given"));
    }
    let cols: Vec<(usize, bool)> =
        keys.iter().map(|(name, desc)| column_index(table, step, index, name).map(|c| (c, *desc))).collect::<Result<_, _>>()?;
    // `sort_by` is stable, so equal keys keep the order the rows arrived in — which is what a
    // person expects from a step that only names one column.
    table.rows.sort_by(|a, b| {
        for &(c, desc) in &cols {
            let (x, y) = (a.get(c), b.get(c));
            // Blanks go last whichever way the column is sorted, as they do in Excel: reversing
            // them to the top is the one thing a person never means by "descending".
            let ord = match (x, y) {
                (Some(a), Some(b)) if a.is_empty() && b.is_empty() => std::cmp::Ordering::Equal,
                (Some(a), Some(_)) if a.is_empty() => std::cmp::Ordering::Greater,
                (Some(_), Some(b)) if b.is_empty() => std::cmp::Ordering::Less,
                (Some(x), Some(y)) if desc => compare(x, y).reverse(),
                (Some(x), Some(y)) => compare(x, y),
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
            };
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(())
}

fn remove_duplicates(table: &mut Table, index: usize, step: &QueryStep, columns: &[String]) -> Result<(), QueryError> {
    let cols: Vec<usize> = if columns.is_empty() {
        (0..table.width()).collect()
    } else {
        columns.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?
    };
    let mut seen: std::collections::HashSet<Vec<String>> = std::collections::HashSet::new();
    table.rows.retain(|row| {
        let key: Vec<String> = cols.iter().map(|&c| row.get(c).map(cell_text).unwrap_or_default()).collect();
        seen.insert(key)
    });
    Ok(())
}

fn replace(table: &mut Table, index: usize, step: &QueryStep, column: &str, from: &str, to: &str) -> Result<(), QueryError> {
    let c = column_index(table, step, index, column)?;
    // The replacement keeps the cell's type: a number replaced by a number stays a number, and a
    // number replaced by text becomes text. Replacing a blank fills it in.
    let new_value = Value::text(to);
    for row in &mut table.rows {
        let Some(cell) = row.get_mut(c) else { continue };
        let hit = match &*cell {
            Value::Empty => from.is_empty(),
            other => cell_text(other) == from,
        };
        if !hit {
            continue;
        }
        // A number replaced by a number stays a number; anything else becomes the text given.
        *cell = if cell.is_number() {
            match to.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Value::number(n),
                _ => new_value.clone(),
            }
        } else {
            new_value.clone()
        };
    }
    Ok(())
}

fn fill_down(table: &mut Table, index: usize, step: &QueryStep, column: &str) -> Result<(), QueryError> {
    let c = column_index(table, step, index, column)?;
    let mut last: Option<Value> = None;
    for row in &mut table.rows {
        let Some(cell) = row.get_mut(c) else { continue };
        if cell.is_empty() {
            if let Some(v) = &last {
                *cell = v.clone();
            }
        } else {
            last = Some(cell.clone());
        }
    }
    Ok(())
}

fn unpivot(table: &mut Table, index: usize, step: &QueryStep, attributes: &[String], values: &[String]) -> Result<(), QueryError> {
    let attr_cols: Vec<usize> = attributes.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?;
    let value_cols: Vec<usize> = if values.is_empty() {
        (0..table.width()).filter(|c| !attr_cols.contains(c)).collect()
    } else {
        values.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?
    };
    if value_cols.is_empty() {
        return Err(invalid(index, step, "no column was unpivoted"));
    }
    if value_cols.iter().any(|c| attr_cols.contains(c)) {
        return Err(invalid(index, step, "a column cannot be both kept and unpivoted"));
    }
    let attribute_name = unique_name(&table.columns, "Attribute");
    let value_name = unique_name(&table.columns, "Value");
    let mut names: Vec<String> = attr_cols.iter().map(|&c| table.columns.get(c).cloned().unwrap_or_default()).collect();
    names.push(attribute_name.clone());
    names.push(value_name);
    let mut rows = Vec::new();
    for row in &table.rows {
        let kept: Vec<Value> = attr_cols.iter().map(|&c| row.get(c).cloned().unwrap_or(Value::Empty)).collect();
        for &c in &value_cols {
            let mut out = kept.clone();
            out.push(Value::text(table.columns.get(c).cloned().unwrap_or_default()));
            out.push(row.get(c).cloned().unwrap_or(Value::Empty));
            rows.push(out);
        }
    }
    table.columns = names;
    table.rows = rows;
    table.check_size()?;
    Ok(())
}

/// `base` or `base2`, `base3`… whichever the table does not already use.
fn unique_name(columns: &[String], base: &str) -> String {
    let mut candidate = base.to_string();
    let mut n = 1u32;
    while columns.iter().any(|c| c.eq_ignore_ascii_case(&candidate)) {
        n = n.saturating_add(1);
        candidate = format!("{base}{n}");
    }
    candidate
}

fn group_by(
    table: &mut Table,
    index: usize,
    step: &QueryStep,
    by: &[String],
    aggregates: &[(QueryAggregate, Option<String>, String)],
) -> Result<(), QueryError> {
    if by.is_empty() {
        return Err(invalid(index, step, "no column was given to group by"));
    }
    let by_cols: Vec<usize> = by.iter().map(|c| column_index(table, step, index, c)).collect::<Result<_, _>>()?;
    let mut aggs: Vec<(QueryAggregate, Option<usize>, String)> = Vec::with_capacity(aggregates.len());
    for (op, column, name) in aggregates {
        let c = match column {
            Some(c) => Some(column_index(table, step, index, c)?),
            None => None,
        };
        if *op != QueryAggregate::Count && c.is_none() {
            return Err(invalid(index, step, format!("{} needs a column", agg_label(*op))));
        }
        let clean = name.trim();
        if clean.is_empty() {
            return Err(invalid(index, step, "an aggregate has a blank name"));
        }
        let clash = by_cols.iter().any(|&k| table.columns.get(k).is_some_and(|n| n.eq_ignore_ascii_case(clean)))
            || aggs.iter().any(|(_, _, n)| n.eq_ignore_ascii_case(clean));
        if clash {
            return Err(invalid(index, step, format!("the result already has a column named \"{clean}\"")));
        }
        aggs.push((*op, c, clean.to_string()));
    }
    // Groups in first-seen order, keyed by the text of the grouping columns.
    let mut order: Vec<Vec<Value>> = Vec::new();
    let mut lookup: HashMap<String, usize> = HashMap::new();
    let mut members: Vec<Vec<&Vec<Value>>> = Vec::new();
    for row in &table.rows {
        let key: Vec<String> = by_cols.iter().map(|&c| row.get(c).map(cell_text).unwrap_or_default()).collect();
        let joined = key.join("\u{1}");
        let slot = match lookup.get(&joined) {
            Some(&i) => i,
            None => {
                let i = order.len();
                lookup.insert(joined, i);
                order.push(by_cols.iter().map(|&c| row.get(c).cloned().unwrap_or(Value::Empty)).collect());
                members.push(Vec::new());
                i
            }
        };
        if let Some(group) = members.get_mut(slot) {
            group.push(row);
        }
    }
    let mut names: Vec<String> = by_cols.iter().map(|&c| table.columns.get(c).cloned().unwrap_or_default()).collect();
    names.extend(aggs.iter().map(|(_, _, n)| n.clone()));
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(order.len());
    for (i, key) in order.into_iter().enumerate() {
        let mut out = key;
        let group = members.get(i).cloned().unwrap_or_default();
        for (op, column, _) in &aggs {
            let values: Vec<&Value> = match column {
                Some(c) => group.iter().filter_map(|r| r.get(*c)).collect(),
                None => Vec::new(),
            };
            out.push(aggregate(*op, group.len(), &values));
        }
        rows.push(out);
    }
    table.columns = names;
    table.rows = rows;
    table.check_size()?;
    Ok(())
}

fn agg_label(op: QueryAggregate) -> &'static str {
    match op {
        QueryAggregate::Count => "Count",
        QueryAggregate::Sum => "Sum",
        QueryAggregate::Average => "Average",
        QueryAggregate::Min => "Min",
        QueryAggregate::Max => "Max",
    }
}

fn aggregate(op: QueryAggregate, rows: usize, values: &[&Value]) -> Value {
    if op == QueryAggregate::Count {
        // Count counts rows, not numbers: a group of ten blanks is ten rows.
        return Value::number(rows as f64);
    }
    let nums: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).filter(|n| n.is_finite()).collect();
    if nums.is_empty() {
        return Value::Empty;
    }
    match op {
        QueryAggregate::Count => Value::number(nums.len() as f64),
        QueryAggregate::Sum => Value::number(nums.iter().sum()),
        QueryAggregate::Average => Value::number(nums.iter().sum::<f64>() / nums.len() as f64),
        QueryAggregate::Min => nums.iter().copied().reduce(f64::min).map_or(Value::Empty, Value::number),
        QueryAggregate::Max => nums.iter().copied().reduce(f64::max).map_or(Value::Empty, Value::number),
    }
}
