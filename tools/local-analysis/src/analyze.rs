use crate::csv::{escape_text, push_record_end, write_field};
use crate::decimal::{Decimal, add_decimals};
use crate::fault::Fault;
use crate::limits::{MAX_GROUPS, MAX_OUTPUT_BYTES};
use crate::source::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Sum,
    Count,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub key: String,
    pub text_key: bool,
    pub value: Decimal,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub groups: Vec<Group>,
    pub null_groups: u64,
    pub null_values: u64,
    pub csv: Vec<u8>,
}

pub fn analyze(
    rows: &[Vec<Cell>],
    operation: Operation,
    group_column: usize,
    value_column: usize,
    group_header: &str,
    value_header: &str,
) -> Result<Analysis, Fault> {
    if rows.first().is_some_and(|row| group_column >= row.len() || value_column >= row.len()) {
        return Err(Fault::request());
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut null_groups = 0_u64;
    let mut null_values = 0_u64;
    let mut seen_kind: Option<SumKind> = None;
    for row in rows {
        if group_column >= row.len() || value_column >= row.len() {
            return Err(Fault::source());
        }
        let Some((key, text_key)) = group_key(&row[group_column])? else {
            null_groups += 1;
            continue;
        };
        match operation {
            Operation::Count => count_one(&mut groups, key, text_key)?,
            Operation::Sum => {
                let kind = sum_kind(&row[value_column])?;
                match kind {
                    None => null_values += 1,
                    Some(kind) => {
                        if seen_kind.is_some_and(|seen| seen != kind) {
                            return Err(Fault::source());
                        }
                        if seen_kind.is_none() {
                            seen_kind = Some(kind);
                        }
                        let value = match &row[value_column] {
                            Cell::Int(dec) | Cell::Decimal(dec) => dec.clone(),
                            Cell::Null | Cell::Bool(_) | Cell::Text(_) => return Err(Fault::source()),
                        };
                        add_group(&mut groups, key, text_key, value)?;
                    }
                }
            }
        }
    }
    let csv = render_csv(&groups, group_header, value_header)?;
    Ok(Analysis {
        groups,
        null_groups,
        null_values,
        csv,
    })
}

fn count_one(groups: &mut Vec<Group>, key: String, text_key: bool) -> Result<(), Fault> {
    if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
        let one = Decimal {
            negative: false,
            coefficient: "1".to_string(),
            scale: 0,
            text: "1".to_string(),
        };
        group.value = add_decimals(&group.value, &one)?;
        return Ok(());
    }
    if groups.len() >= MAX_GROUPS {
        return Err(Fault::budget());
    }
    groups.push(Group {
        key,
        text_key,
        value: Decimal {
            negative: false,
            coefficient: "1".to_string(),
            scale: 0,
            text: "1".to_string(),
        },
    });
    Ok(())
}

fn add_group(
    groups: &mut Vec<Group>,
    key: String,
    text_key: bool,
    value: Decimal,
) -> Result<(), Fault> {
    if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
        group.value = add_decimals(&group.value, &value)?;
        return Ok(());
    }
    if groups.len() >= MAX_GROUPS {
        return Err(Fault::budget());
    }
    groups.push(Group {
        key,
        text_key,
        value,
    });
    Ok(())
}

fn group_key(cell: &Cell) -> Result<Option<(String, bool)>, Fault> {
    match cell {
        Cell::Null => Ok(None),
        Cell::Text(text) => Ok(Some((text.clone(), true))),
        Cell::Bool(true) => Ok(Some(("true".to_string(), false))),
        Cell::Bool(false) => Ok(Some(("false".to_string(), false))),
        Cell::Int(dec) | Cell::Decimal(dec) => Ok(Some((dec.text.clone(), false))),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SumKind {
    Int,
    Decimal,
}

fn sum_kind(cell: &Cell) -> Result<Option<SumKind>, Fault> {
    match cell {
        Cell::Null => Ok(None),
        Cell::Int(_) => Ok(Some(SumKind::Int)),
        Cell::Decimal(_) => Ok(Some(SumKind::Decimal)),
        Cell::Bool(_) | Cell::Text(_) => Err(Fault::source()),
    }
}

fn render_csv(
    groups: &[Group],
    group_header: &str,
    value_header: &str,
) -> Result<Vec<u8>, Fault> {
    let mut csv = Vec::new();
    write_field(&mut csv, &escape_text(group_header));
    csv.push(b',');
    write_field(&mut csv, &escape_text(value_header));
    push_record_end(&mut csv);
    for group in groups {
        let key = if group.text_key {
            escape_text(&group.key)
        } else {
            group.key.clone()
        };
        write_field(&mut csv, &key);
        csv.push(b',');
        write_field(&mut csv, &group.value.text);
        push_record_end(&mut csv);
        if csv.len() > MAX_OUTPUT_BYTES {
            return Err(Fault::budget());
        }
    }
    if csv.len() > MAX_OUTPUT_BYTES {
        return Err(Fault::budget());
    }
    Ok(csv)
}
