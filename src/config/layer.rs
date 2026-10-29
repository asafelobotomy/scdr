//! The settings layer: `settings.toml` in scdrd's state directory, written
//! by scdrd on behalf of the widget's settings pages and read as the
//! highest-precedence config file. Admin files in /etc are never rewritten.
//!
//! The widget works on documents in the config file's own schema, as JSON:
//! the layer itself, and the merge of every file below it (so it can show
//! what a setting falls back to). In the merged document each rule and
//! profile carries a `_source` key naming its file.

use super::{Config, ConfigError, Paths};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const LAYER_FILE: &str = "settings.toml";

const HEADER: &str = "# Written by scdrd from the SCDR widget's settings. Edit it there, or\n\
                      # delete this file to drop every setting made in the widget.\n\n";

impl Paths {
    pub fn layer(&self) -> PathBuf {
        self.state.join(LAYER_FILE)
    }

    /// Config files below the settings layer, lowest precedence first.
    pub fn lower_files(&self) -> Vec<PathBuf> {
        let layer = self.layer();
        self.files().into_iter().filter(|f| *f != layer).collect()
    }

    /// The settings layer as JSON; an empty object when there is none.
    pub fn layer_document(&self) -> Result<Value, ConfigError> {
        let path = self.layer();
        if !path.is_file() {
            return Ok(Value::Object(Map::new()));
        }
        read_document(&path)
    }

    /// Every file below the layer, merged the way the config loader layers them.
    pub fn base_document(&self) -> Result<Value, ConfigError> {
        let mut merged = Map::new();
        for path in self.lower_files() {
            let Value::Object(doc) = read_document(&path)? else { continue };
            merge(&mut merged, doc, &path.display().to_string());
        }
        Ok(Value::Object(merged))
    }

    /// Validates `doc` on top of the lower files, then replaces the layer.
    /// Returns the resulting config.
    pub fn write_layer(&self, doc: &Value) -> Result<Config, ConfigError> {
        let path = self.layer();
        let text = to_toml(doc).map_err(|e| ConfigError::invalid(&path, e))?;
        let mut sources = Vec::new();
        for file in self.lower_files() {
            let contents = std::fs::read_to_string(&file)
                .map_err(|source| ConfigError::Io { path: file.clone(), source })?;
            sources.push((file, contents));
        }
        sources.push((path.clone(), text.clone()));
        let config = Config::from_sources(sources)?;

        let io = |source| ConfigError::Io { path: path.clone(), source };
        if text.is_empty() {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
                _ => {}
            }
        } else {
            let tmp = path.with_extension("toml.new");
            std::fs::write(&tmp, format!("{HEADER}{text}")).map_err(io)?;
            std::fs::rename(&tmp, &path).map_err(io)?;
        }
        Ok(config)
    }
}

fn read_document(path: &Path) -> Result<Value, ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|source| ConfigError::Io { path: path.to_owned(), source })?;
    let doc: toml::Table = toml::from_str(&text).map_err(|e| ConfigError::invalid(path, e.to_string()))?;
    serde_json::to_value(doc).map_err(|e| ConfigError::invalid(path, e.to_string()))
}

/// The layering rules of `Config::from_sources`: whole profiles replace
/// profiles of the same name, rules from later files are tried first,
/// exceptions add up, and every other table merges key by key.
fn merge(into: &mut Map<String, Value>, from: Map<String, Value>, source: &str) {
    for (key, value) in from {
        match (key.as_str(), value) {
            ("profile", Value::Object(profiles)) => {
                let dst = table(into, "profile");
                for (name, mut p) in profiles {
                    if let Value::Object(p) = &mut p {
                        p.insert("_source".into(), source.into());
                    }
                    dst.insert(name, p);
                }
            }
            ("rule", Value::Array(mut rules)) => {
                for r in &mut rules {
                    if let Value::Object(r) = r {
                        r.insert("_source".into(), source.into());
                    }
                }
                if let Some(Value::Array(older)) = into.remove("rule") {
                    rules.extend(older);
                }
                into.insert(key, Value::Array(rules));
            }
            ("exceptions", Value::Object(ex)) => {
                let dst = table(into, "exceptions");
                for (k, v) in ex {
                    match (dst.get_mut(&k), v) {
                        (Some(Value::Array(a)), Value::Array(b)) => a.extend(b),
                        (_, v) => {
                            dst.insert(k, v);
                        }
                    }
                }
            }
            (_, Value::Object(sub)) => {
                let mut dst = match into.remove(&key) {
                    Some(Value::Object(m)) => m,
                    _ => Map::new(),
                };
                merge(&mut dst, sub, source);
                into.insert(key, Value::Object(dst));
            }
            (_, v) => {
                into.insert(key, v);
            }
        }
    }
}

fn table<'a>(m: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let v = m.entry(key).or_insert_with(|| Value::Object(Map::new()));
    if !v.is_object() {
        *v = Value::Object(Map::new());
    }
    v.as_object_mut().unwrap()
}

/// JSON to TOML, dropping nulls, empty tables and `_`-prefixed keys (the
/// annotations `base_document` adds). An empty document becomes "".
fn to_toml(doc: &Value) -> Result<String, String> {
    let Some(clean) = clean(doc) else { return Ok(String::new()) };
    if !clean.is_object() {
        return Err("settings must be a table".into());
    }
    let table = toml::Table::try_from(clean).map_err(|e| e.to_string())?;
    toml::to_string(&table).map_err(|e| e.to_string())
}

fn clean(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Object(m) => {
            let m: Map<String, Value> = m
                .iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .filter_map(|(k, v)| Some((k.clone(), clean(v)?)))
                .collect();
            (!m.is_empty()).then_some(Value::Object(m))
        }
        Value::Array(a) => Some(Value::Array(a.iter().filter_map(clean).collect())),
        // Integral numbers from JavaScript arrive as floats (e.g. 30.0).
        Value::Number(n) => Some(match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => Value::from(f as i64),
            _ => v.clone(),
        }),
        _ => Some(v.clone()),
    }
}

#[cfg(test)]
mod tests;
