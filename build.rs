use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let i18n_dir = manifest_dir.join("i18n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let out_file = out_dir.join("embedded_i18n.rs");

    println!("cargo:rerun-if-changed={}", i18n_dir.display());

    let catalogs = read_catalogs(&i18n_dir).unwrap_or_else(|err| {
        panic!(
            "failed to embed i18n catalogs from {}: {err}",
            i18n_dir.display()
        )
    });
    let supported_locales = supported_locales(&catalogs).unwrap_or_else(|err| {
        panic!(
            "failed to read supported locales from {}: {err}",
            i18n_dir.join("locales.json").display()
        )
    });
    assert_supported_catalogs(&catalogs, &supported_locales);

    let mut generated =
        String::from("pub fn locale_json(locale: &str) -> Option<&'static str> {\n");
    generated.push_str("    match locale {\n");
    for (locale, raw_json) in catalogs {
        generated.push_str("        ");
        generated.push_str(&rust_string_literal(&locale));
        generated.push_str(" => Some(");
        generated.push_str(&rust_raw_string_literal(&raw_json));
        generated.push_str("),\n");
    }
    generated.push_str("        _ => None,\n");
    generated.push_str("    }\n");
    generated.push_str("}\n");
    generated.push('\n');
    generated.push_str("pub fn supported_locales() -> &'static [&'static str] {\n");
    generated.push_str("    &[\n");
    for locale in supported_locales {
        generated.push_str("        ");
        generated.push_str(&rust_string_literal(&locale));
        generated.push_str(",\n");
    }
    generated.push_str("    ]\n");
    generated.push_str("}\n");

    fs::write(out_file, generated).expect("failed to write embedded i18n module");
}

fn read_catalogs(i18n_dir: &Path) -> io::Result<Vec<(String, String)>> {
    let mut catalogs = Vec::new();

    for entry in fs::read_dir(i18n_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        println!("cargo:rerun-if-changed={}", path.display());

        let locale = path
            .file_stem()
            .and_then(|value| value.to_str())
            .expect("i18n file name should be valid UTF-8")
            .to_string();
        let raw_json = fs::read_to_string(&path)?;
        catalogs.push((locale, raw_json));
    }

    catalogs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(catalogs)
}

fn supported_locales(catalogs: &[(String, String)]) -> io::Result<Vec<String>> {
    let locales_raw = catalogs
        .iter()
        .find(|(locale, _)| locale == "locales")
        .map(|(_, raw_json)| raw_json)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing i18n/locales.json"))?;
    parse_json_string_array(locales_raw)
}

fn assert_supported_catalogs(catalogs: &[(String, String)], supported_locales: &[String]) {
    let catalog_names = catalogs
        .iter()
        .map(|(locale, _)| locale.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut missing = Vec::new();
    for locale in supported_locales {
        if !catalog_names.contains(locale.as_str()) {
            missing.push(locale.as_str());
        }
    }
    if !missing.is_empty() {
        panic!(
            "i18n/locales.json lists locales without bundled catalogs: {}",
            missing.join(", ")
        );
    }
}

fn parse_json_string_array(raw: &str) -> io::Result<Vec<String>> {
    let mut values = Vec::new();
    let mut chars = raw.chars().peekable();

    skip_ws(&mut chars);
    if chars.next() != Some('[') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected JSON array",
        ));
    }

    loop {
        skip_ws(&mut chars);
        match chars.peek().copied() {
            Some(']') => {
                chars.next();
                break;
            }
            Some('"') => values.push(parse_json_string(&mut chars)?),
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expected JSON string",
                ));
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unterminated JSON array",
                ));
            }
        }

        skip_ws(&mut chars);
        match chars.peek().copied() {
            Some(',') => {
                chars.next();
            }
            Some(']') => {}
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expected comma or array end",
                ));
            }
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unterminated JSON array",
                ));
            }
        }
    }

    skip_ws(&mut chars);
    if chars.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected data after JSON array",
        ));
    }
    Ok(values)
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while matches!(chars.peek(), Some(value) if value.is_whitespace()) {
        chars.next();
    }
}

fn parse_json_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> io::Result<String> {
    if chars.next() != Some('"') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected string opening quote",
        ));
    }

    let mut value = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Ok(value),
            '\\' => match chars.next() {
                Some('"') => value.push('"'),
                Some('\\') => value.push('\\'),
                Some('/') => value.push('/'),
                Some('b') => value.push('\u{0008}'),
                Some('f') => value.push('\u{000c}'),
                Some('n') => value.push('\n'),
                Some('r') => value.push('\r'),
                Some('t') => value.push('\t'),
                Some('u') => {
                    for _ in 0..4 {
                        match chars.next() {
                            Some(hex) if hex.is_ascii_hexdigit() => {}
                            _ => {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "invalid unicode escape",
                                ));
                            }
                        }
                    }
                    value.push('?');
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid string escape",
                    ));
                }
            },
            _ => value.push(ch),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "unterminated JSON string",
    ))
}

fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn rust_raw_string_literal(value: &str) -> String {
    for hashes in 0..16 {
        let delimiter = "#".repeat(hashes);
        let terminator = format!("\"{delimiter}");
        if !value.contains(&terminator) {
            return format!("r{delimiter}\"{value}\"{delimiter}");
        }
    }

    rust_string_literal(value)
}
