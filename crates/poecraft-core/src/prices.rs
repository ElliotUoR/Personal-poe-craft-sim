//! Price table: chaos value per price key (port of `poecraft/prices.py`).
//!
//! Default file: data/prices.json. data/prices.local.json (git-ignored) and any extra files
//! are merged on top, in that order.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

pub fn unit_alias(k: &str) -> &str {
    match k {
        "c" | "chaos" => "chaos",
        "div" | "divine" | "divines" => "divine",
        "ex" | "exalt" => "exalt",
        o => o,
    }
}

#[derive(Debug, Default)]
pub struct Prices {
    pub table: BTreeMap<String, f64>,
    pub meta: BTreeMap<String, Value>,
    pub missing: Mutex<BTreeSet<String>>,
}

impl Prices {
    pub fn load(
        data_dir: &Path,
        overrides: &[&Path],
        include_local: bool,
    ) -> Result<Prices, String> {
        let read = |p: &Path| -> Result<Value, String> {
            let txt = std::fs::read_to_string(p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            serde_json::from_str(&txt).map_err(|e| format!("bad price file {}: {e}", p.display()))
        };
        let base = read(&data_dir.join("prices.json"))?;
        let mut pr = Prices::default();
        if let Value::Object(o) = &base {
            for (k, v) in o {
                if k != "prices" {
                    pr.meta.insert(k.clone(), v.clone());
                }
            }
        }
        pr.merge(&base);
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        let local = data_dir.join("prices.local.json");
        if include_local && local.exists() {
            files.push(local);
        }
        files.extend(overrides.iter().map(|p| p.to_path_buf()));
        let mut applied = Vec::new();
        for f in files {
            pr.merge(&read(&f)?);
            applied.push(Value::String(f.display().to_string()));
        }
        if !applied.is_empty() {
            pr.meta.insert("overrides".into(), Value::Array(applied));
        }
        Ok(pr)
    }

    fn merge(&mut self, v: &Value) {
        let src = v.get("prices").unwrap_or(v);
        if let Value::Object(o) = src {
            for (k, x) in o {
                if let Some(n) = x.as_f64() {
                    self.table.insert(k.clone(), n);
                }
            }
        }
    }

    pub fn from_table(t: BTreeMap<String, f64>) -> Prices {
        Prices {
            table: t,
            ..Default::default()
        }
    }

    /// Chaos value of one unit of `key`. Unknown keys count as 0 and are remembered.
    pub fn get(&self, key: &str) -> f64 {
        let key = unit_alias(key);
        if key.starts_with('_') {
            return 0.0;
        }
        match self.table.get(key) {
            Some(v) => *v,
            None => {
                self.missing.lock().unwrap().insert(key.to_string());
                0.0
            }
        }
    }

    pub fn chaos(&self, cost: &[(String, f64)]) -> f64 {
        cost.iter().map(|(k, n)| self.get(k) * n).sum()
    }

    pub fn divine(&self) -> f64 {
        self.get("divine")
    }
}
