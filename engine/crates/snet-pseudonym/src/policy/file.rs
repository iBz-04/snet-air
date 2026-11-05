use toml::{Table, Value};

use crate::detect::Builtin;
use crate::errors::PolicyError;

/// The policy file's fields as written, before dictionaries are read and
/// patterns compiled.
pub(crate) struct PolicyFile {
    pub(crate) enabled: Vec<Builtin>,
    pub(crate) dictionaries: Vec<DictionaryEntry>,
    pub(crate) patterns: Vec<PatternEntry>,
    pub(crate) passthrough: Vec<PassthroughEntry>,
}

pub(crate) struct DictionaryEntry {
    pub(crate) category: String,
    pub(crate) path: String,
}

pub(crate) struct PatternEntry {
    pub(crate) category: String,
    pub(crate) regex: String,
}

pub(crate) struct PassthroughEntry {
    pub(crate) category: String,
    pub(crate) endpoint_name: String,
    pub(crate) host: String,
    pub(crate) legal_basis: String,
}

const TOP_LEVEL: &[&str] = &["builtin", "dictionaries", "patterns", "passthrough"];

pub(crate) fn parse(text: &str) -> Result<PolicyFile, PolicyError> {
    let table: Table = toml::from_str(text).map_err(|error| PolicyError::Syntax {
        line: error
            .span()
            .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1)
            .unwrap_or(1),
        message: error.message().to_string(),
    })?;
    exact_keys(&table, TOP_LEVEL, "")?;

    let builtin = table_field(&table, "builtin", "builtin")?;
    let builtin_names: Vec<&str> = Builtin::ALL.iter().map(|kind| kind.name()).collect();
    exact_keys(builtin, &builtin_names, "builtin.")?;
    let mut enabled = Vec::new();
    for kind in Builtin::ALL {
        let field = format!("builtin.{}", kind.name());
        match builtin.get(kind.name()) {
            Some(Value::Boolean(true)) => enabled.push(kind),
            Some(Value::Boolean(false)) => {}
            _ => return Err(invalid(field, "must be true or false")),
        }
    }

    let dictionaries = entries(&table, "dictionaries", &["category", "path"])?
        .into_iter()
        .map(|mut fields| DictionaryEntry {
            category: fields.remove(0),
            path: fields.remove(0),
        })
        .collect();
    let patterns = entries(&table, "patterns", &["category", "regex"])?
        .into_iter()
        .map(|mut fields| PatternEntry {
            category: fields.remove(0),
            regex: fields.remove(0),
        })
        .collect();
    let passthrough = entries(
        &table,
        "passthrough",
        &["category", "endpoint_name", "host", "legal_basis"],
    )?
    .into_iter()
    .map(|mut fields| PassthroughEntry {
        category: fields.remove(0),
        endpoint_name: fields.remove(0),
        host: fields.remove(0),
        legal_basis: fields.remove(0),
    })
    .collect();

    Ok(PolicyFile {
        enabled,
        dictionaries,
        patterns,
        passthrough,
    })
}

pub(crate) fn invalid(field: impl Into<String>, reason: impl Into<String>) -> PolicyError {
    PolicyError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

fn exact_keys(table: &Table, allowed: &[&str], prefix: &str) -> Result<(), PolicyError> {
    if let Some(unknown) = table.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(PolicyError::Unknown {
            field: format!("{prefix}{unknown}"),
        });
    }
    if let Some(missing) = allowed.iter().find(|key| !table.contains_key(**key)) {
        return Err(PolicyError::Missing {
            field: format!("{prefix}{missing}"),
        });
    }
    Ok(())
}

fn table_field<'t>(table: &'t Table, key: &str, field: &str) -> Result<&'t Table, PolicyError> {
    match table.get(key) {
        Some(Value::Table(inner)) => Ok(inner),
        _ => Err(invalid(field, "must be a table")),
    }
}

/// An array of tables whose every entry has exactly `fields`, each a string,
/// returned in the order of `fields`.
fn entries(
    table: &Table,
    key: &'static str,
    fields: &[&str],
) -> Result<Vec<Vec<String>>, PolicyError> {
    let Some(Value::Array(items)) = table.get(key) else {
        return Err(invalid(key, "must be an array of tables"));
    };
    let mut parsed = Vec::with_capacity(items.len());
    for (position, item) in items.iter().enumerate() {
        let prefix = format!("{key}[{position}].");
        let Value::Table(entry) = item else {
            return Err(invalid(format!("{key}[{position}]"), "must be a table"));
        };
        exact_keys(entry, fields, &prefix)?;
        let mut values = Vec::with_capacity(fields.len());
        for field in fields {
            match entry.get(*field) {
                Some(Value::String(value)) => values.push(value.clone()),
                _ => return Err(invalid(format!("{prefix}{field}"), "must be a string")),
            }
        }
        parsed.push(values);
    }
    Ok(parsed)
}
