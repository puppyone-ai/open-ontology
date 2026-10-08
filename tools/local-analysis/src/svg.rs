use crate::analyze::Group;
use crate::csv::xml_text;
use crate::decimal::Decimal;
use crate::fault::Fault;
use crate::limits::MAX_OUTPUT_BYTES;

pub struct Chart {
    pub bytes: Vec<u8>,
    pub label_omitted: bool,
}

pub fn render(title: &str, query_id: &str, citable: bool, groups: &[Group]) -> Result<Chart, Fault> {
    let (title_xml, title_omitted) = xml_text(title);
    let (id_xml, id_omitted) = xml_text(query_id);
    let mut omitted = title_omitted || id_omitted;
    let desc = if citable {
        "数值已核对。DuckDB、Arrow、Excel未准入。字体未随包提供。"
    } else {
        "不可引用。DuckDB、Arrow、Excel未准入。字体未随包提供。"
    };
    let height = 72 + groups.len().max(1) * 36;
    let mut body = String::new();
    body.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    body.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1280\" height=\"{height}\" viewBox=\"0 0 1280 {height}\">\n"
    ));
    body.push_str("<rect width=\"1280\" height=\"100%\" fill=\"#f7f4ef\"/>\n");
    body.push_str(&format!("<title>{title_xml}</title>\n<desc>{desc}</desc>\n"));
    body.push_str(&format!(
        "<text x=\"24\" y=\"36\" font-family=\"Noto Sans CJK SC, Source Han Sans SC, sans-serif\" font-size=\"20\" fill=\"#1c1917\">{title_xml}</text>\n"
    ));
    body.push_str(&format!(
        "<text x=\"24\" y=\"60\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#44403c\">{id_xml}</text>\n"
    ));
    if groups.is_empty() {
        body.push_str("<text x=\"24\" y=\"96\" font-family=\"Noto Sans CJK SC, Source Han Sans SC, sans-serif\" font-size=\"16\" fill=\"#1c1917\">无分组</text>\n");
    }
    let scaled = scaled_coefficients(groups)?;
    let max_abs = max_text(&scaled);
    for (index, group) in groups.iter().enumerate() {
        let y = 84 + index * 36;
        let (label, skip) = xml_text(&group.key);
        omitted |= skip;
        let (value, skip_value) = xml_text(&group.value.text);
        omitted |= skip_value;
        let width = bar_width(&group.value, &scaled[index], &max_abs)?;
        let fill = if group.value.negative {
            "#9b2226"
        } else if group.value.coefficient == "0" {
            "#6c757d"
        } else {
            "#3c6e71"
        };
        body.push_str(&format!(
            "<text x=\"24\" y=\"{y}\" font-family=\"Noto Sans CJK SC, Source Han Sans SC, sans-serif\" font-size=\"14\" fill=\"#1c1917\">{label}</text>\n"
        ));
        let bar_y = y + 6;
        body.push_str(&format!(
            "<rect x=\"280\" y=\"{bar_y}\" width=\"{width}\" height=\"14\" fill=\"{fill}\"/>\n"
        ));
        let value_x = 288 + width;
        body.push_str(&format!(
            "<text x=\"{value_x}\" y=\"{y}\" font-family=\"sans-serif\" font-size=\"14\" fill=\"#1c1917\">{value}</text>\n"
        ));
    }
    body.push_str("</svg>\n");
    if body.len() > MAX_OUTPUT_BYTES {
        return Err(Fault::budget());
    }
    Ok(Chart {
        bytes: body.into_bytes(),
        label_omitted: omitted,
    })
}

fn scaled_coefficients(groups: &[Group]) -> Result<Vec<String>, Fault> {
    let scale = groups.iter().map(|group| group.value.scale).max().unwrap_or(0);
    let mut out = Vec::with_capacity(groups.len());
    for group in groups {
        let delta = scale - group.value.scale;
        if delta < 0 {
            return Err(Fault::budget());
        }
        let extra = usize::try_from(delta).map_err(|_| Fault::budget())?;
        if group.value.coefficient.len().saturating_add(extra) > crate::limits::MAX_SUM_DIGITS {
            return Err(Fault::budget());
        }
        let mut digits = group.value.coefficient.clone();
        digits.extend(std::iter::repeat_n('0', extra));
        out.push(digits);
    }
    Ok(out)
}

fn max_text(values: &[String]) -> String {
    let mut max = "0".to_string();
    for value in values {
        if cmp_mag(value, &max) == std::cmp::Ordering::Greater {
            max = value.clone();
        }
    }
    max
}

fn bar_width(value: &Decimal, scaled: &str, max_abs: &str) -> Result<u32, Fault> {
    if value.coefficient == "0" || max_abs == "0" || max_abs.bytes().all(|byte| byte == b'0') {
        return Ok(0);
    }
    let width = ratio(scaled, max_abs)?;
    Ok(if width == 0 { 1 } else { width })
}

fn ratio(value: &str, max_abs: &str) -> Result<u32, Fault> {
    let numer = mul_small(value, 400)?;
    div_floor(&numer, max_abs)
}

fn mul_small(value: &str, factor: u32) -> Result<String, Fault> {
    let mut carry = 0_u32;
    let bytes = value.as_bytes();
    let mut out = vec![b'0'; bytes.len() + 4];
    for index in 0..bytes.len() {
        let digit = u32::from(bytes[bytes.len() - 1 - index] - b'0');
        let prod = digit * factor + carry;
        let slot = out.len() - 1 - index;
        out[slot] = b'0' + (prod % 10) as u8;
        carry = prod / 10;
    }
    let mut place = out.len() - bytes.len();
    while carry > 0 {
        if place == 0 {
            return Err(Fault::budget());
        }
        place -= 1;
        out[place] = b'0' + (carry % 10) as u8;
        carry /= 10;
    }
    Ok(strip(&out[place..]))
}

fn div_floor(numer: &str, denom: &str) -> Result<u32, Fault> {
    if denom.is_empty() || denom.bytes().all(|byte| byte == b'0') {
        return Err(Fault::budget());
    }
    let mut remainder = String::new();
    let mut quot = String::new();
    for ch in numer.chars() {
        if remainder == "0" {
            remainder.clear();
        }
        remainder.push(ch);
        let mut digit = 0_u8;
        while cmp_mag(&remainder, denom) != std::cmp::Ordering::Less {
            remainder = sub_mag(&remainder, denom)?;
            digit += 1;
            if digit > 9 {
                return Err(Fault::budget());
            }
        }
        quot.push(char::from(b'0' + digit));
    }
    let text = strip(quot.as_bytes());
    text.parse::<u32>().map_err(|_| Fault::budget())
}

fn sub_mag(left: &str, right: &str) -> Result<String, Fault> {
    let a = left.as_bytes();
    let b = right.as_bytes();
    if cmp_mag(left, right) == std::cmp::Ordering::Less {
        return Err(Fault::budget());
    }
    let mut out = vec![b'0'; a.len()];
    let mut borrow = 0_i16;
    for index in 0..a.len() {
        let av = i16::from(if index < a.len() {
            a[a.len() - 1 - index] - b'0'
        } else {
            0
        });
        let bv = i16::from(if index < b.len() {
            b[b.len() - 1 - index] - b'0'
        } else {
            0
        });
        let mut diff = av - bv - borrow;
        if diff < 0 {
            diff += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[a.len() - 1 - index] = b'0' + diff as u8;
    }
    Ok(strip(&out))
}

fn strip(bytes: &[u8]) -> String {
    let text: String = bytes.iter().map(|byte| char::from(*byte)).collect();
    let trimmed = text.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn cmp_mag(left: &str, right: &str) -> std::cmp::Ordering {
    let a = left.trim_start_matches('0');
    let b = right.trim_start_matches('0');
    let a = if a.is_empty() { "0" } else { a };
    let b = if b.is_empty() { "0" } else { b };
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
