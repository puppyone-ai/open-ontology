use rust_xlsxwriter::{Format, Workbook};

use crate::analyze::Analysis;
use crate::csv::escape_text;
use crate::fault::Fault;

pub fn write(analysis: &Analysis, group_header: &str, value_header: &str) -> Result<Vec<u8>, Fault> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let text = Format::new().set_num_format("@");
    worksheet
        .write_string_with_format(0, 0, escape_text(group_header), &text)
        .map_err(|_| Fault::xlsx())?;
    worksheet
        .write_string_with_format(0, 1, escape_text(value_header), &text)
        .map_err(|_| Fault::xlsx())?;
    for (index, group) in analysis.groups.iter().enumerate() {
        let row = u32::try_from(index + 1).map_err(|_| Fault::xlsx())?;
        let key = if group.text_key {
            escape_text(&group.key)
        } else {
            group.key.clone()
        };
        worksheet
            .write_string_with_format(row, 0, key, &text)
            .map_err(|_| Fault::xlsx())?;
        worksheet
            .write_string_with_format(row, 1, group.value.text.clone(), &text)
            .map_err(|_| Fault::xlsx())?;
    }
    workbook.save_to_buffer().map_err(|_| Fault::xlsx())
}
