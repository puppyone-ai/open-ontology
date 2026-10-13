//! JSON object duplicate keys fail closed. Identity fields must not be shadowed.

use serde::de::DeserializeOwned;

#[derive(Debug)]
pub struct ParseError {
    pub fact: String,
}

pub fn parse_strict<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError {
        fact: "utf8".to_string(),
    })?;
    reject_duplicate_keys(text)?;
    serde_json::from_slice(bytes).map_err(|err| ParseError {
        fact: err.to_string(),
    })
}

fn reject_duplicate_keys(text: &str) -> Result<(), ParseError> {
    let bytes = text.as_bytes();
    let mut i = 0;
    scan_value(bytes, &mut i)?;
    skip_ws(bytes, &mut i);
    if i != bytes.len() {
        return Err(ParseError {
            fact: "trailing".to_string(),
        });
    }
    Ok(())
}

fn scan_value(bytes: &[u8], i: &mut usize) -> Result<(), ParseError> {
    skip_ws(bytes, i);
    if *i >= bytes.len() {
        return Err(ParseError {
            fact: "truncated".to_string(),
        });
    }
    match bytes[*i] {
        b'{' => scan_object(bytes, i),
        b'[' => scan_array(bytes, i),
        b'"' => {
            scan_string(bytes, i)?;
            Ok(())
        }
        b't' | b'f' | b'n' | b'-' | b'0'..=b'9' => {
            while *i < bytes.len()
                && !matches!(bytes[*i], b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t')
            {
                *i += 1;
            }
            Ok(())
        }
        _ => Err(ParseError {
            fact: "json".to_string(),
        }),
    }
}

fn scan_object(bytes: &[u8], i: &mut usize) -> Result<(), ParseError> {
    *i += 1;
    let mut keys = Vec::<String>::new();
    loop {
        skip_ws(bytes, i);
        if *i >= bytes.len() {
            return Err(ParseError {
                fact: "truncated".to_string(),
            });
        }
        if bytes[*i] == b'}' {
            *i += 1;
            return Ok(());
        }
        let key = scan_string(bytes, i)?;
        if keys.iter().any(|seen| seen == &key) {
            return Err(ParseError {
                fact: format!("duplicate_key:{key}"),
            });
        }
        keys.push(key);
        skip_ws(bytes, i);
        if *i >= bytes.len() || bytes[*i] != b':' {
            return Err(ParseError {
                fact: "json".to_string(),
            });
        }
        *i += 1;
        scan_value(bytes, i)?;
        skip_ws(bytes, i);
        if *i >= bytes.len() {
            return Err(ParseError {
                fact: "truncated".to_string(),
            });
        }
        if bytes[*i] == b',' {
            *i += 1;
            continue;
        }
        if bytes[*i] == b'}' {
            *i += 1;
            return Ok(());
        }
        return Err(ParseError {
            fact: "json".to_string(),
        });
    }
}

fn scan_array(bytes: &[u8], i: &mut usize) -> Result<(), ParseError> {
    *i += 1;
    loop {
        skip_ws(bytes, i);
        if *i >= bytes.len() {
            return Err(ParseError {
                fact: "truncated".to_string(),
            });
        }
        if bytes[*i] == b']' {
            *i += 1;
            return Ok(());
        }
        scan_value(bytes, i)?;
        skip_ws(bytes, i);
        if *i >= bytes.len() {
            return Err(ParseError {
                fact: "truncated".to_string(),
            });
        }
        if bytes[*i] == b',' {
            *i += 1;
            continue;
        }
        if bytes[*i] == b']' {
            *i += 1;
            return Ok(());
        }
        return Err(ParseError {
            fact: "json".to_string(),
        });
    }
}

fn scan_string(bytes: &[u8], i: &mut usize) -> Result<String, ParseError> {
    if *i >= bytes.len() || bytes[*i] != b'"' {
        return Err(ParseError {
            fact: "json".to_string(),
        });
    }
    *i += 1;
    let mut out = String::new();
    while *i < bytes.len() {
        let c = bytes[*i];
        *i += 1;
        if c == b'"' {
            return Ok(out);
        }
        if c == b'\\' {
            if *i >= bytes.len() {
                return Err(ParseError {
                    fact: "truncated".to_string(),
                });
            }
            let esc = bytes[*i];
            *i += 1;
            match esc {
                b'"' | b'\\' | b'/' => out.push(esc as char),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    if *i + 4 > bytes.len() {
                        return Err(ParseError {
                            fact: "truncated".to_string(),
                        });
                    }
                    let hex = std::str::from_utf8(&bytes[*i..*i + 4]).map_err(|_| ParseError {
                        fact: "json".to_string(),
                    })?;
                    let code = u32::from_str_radix(hex, 16).map_err(|_| ParseError {
                        fact: "json".to_string(),
                    })?;
                    out.push(char::from_u32(code).ok_or_else(|| ParseError {
                        fact: "json".to_string(),
                    })?);
                    *i += 4;
                }
                _ => {
                    return Err(ParseError {
                        fact: "json".to_string(),
                    });
                }
            }
        } else {
            out.push(c as char);
        }
    }
    Err(ParseError {
        fact: "truncated".to_string(),
    })
}

fn skip_ws(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && matches!(bytes[*i], b' ' | b'\n' | b'\r' | b'\t') {
        *i += 1;
    }
}
