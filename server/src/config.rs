use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    None,
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn rank(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::Info => 1,
            Self::Warning => 2,
            Self::Error => 3,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct RuleConfig {
    pub description: Option<String>,
    pub severity: Option<Severity>,
    #[serde(rename = "class")]
    pub class_name: Option<String>,
    pub zero_width: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct LanguageOverride {
    pub rules: BTreeMap<String, RuleConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    pub max_diagnostics_per_document: usize,
    pub rules: BTreeMap<String, RuleConfig>,
    pub language_overrides: BTreeMap<String, LanguageOverride>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_diagnostics_per_document: 500,
            rules: BTreeMap::new(),
            language_overrides: BTreeMap::new(),
        }
    }
}

impl ServerConfig {
    pub fn from_optional_value(value: Option<Value>) -> Result<Self> {
        match value {
            Some(value) => Self::from_value(value),
            None => Ok(Self::default()),
        }
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let value = normalize_config_value(unwrap_nested(value));
        serde_json::from_value(value).context("failed to parse Critters configuration")
    }

    pub fn merge(&mut self, other: Self) {
        self.max_diagnostics_per_document = other.max_diagnostics_per_document;
        self.rules.extend(other.rules);

        for (language, override_config) in other.language_overrides {
            self.language_overrides
                .entry(language)
                .or_default()
                .rules
                .extend(override_config.rules);
        }
    }
}

fn unwrap_nested(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            if let Some(nested) = object.remove("critters-lsp") {
                nested
            } else {
                Value::Object(object)
            }
        }
        other => other,
    }
}

fn normalize_config_value(value: Value) -> Value {
    match value {
        Value::Null => Value::Object(Default::default()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ServerConfig;

    #[test]
    fn top_level_null_configuration_resets_to_defaults() {
        let config = ServerConfig::from_value(serde_json::Value::Null)
            .expect("null configuration to parse as defaults");

        assert_eq!(config, ServerConfig::default());
    }

    #[test]
    fn nested_null_configuration_resets_to_defaults() {
        let config = ServerConfig::from_value(json!({ "critters-lsp": null }))
            .expect("nested null configuration to parse as defaults");

        assert_eq!(config, ServerConfig::default());
    }
}
