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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RuleConfigUpdate {
    pub description: Option<String>,
    pub severity: Option<Severity>,
    #[serde(rename = "class")]
    pub class_name: Option<String>,
    pub zero_width: Option<bool>,
}

impl RuleConfigUpdate {
    fn apply_to(self, target: &mut RuleConfig) {
        if let Some(description) = self.description {
            target.description = Some(description);
        }
        if let Some(severity) = self.severity {
            target.severity = Some(severity);
        }
        if let Some(class_name) = self.class_name {
            target.class_name = Some(class_name);
        }
        if let Some(zero_width) = self.zero_width {
            target.zero_width = Some(zero_width);
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LanguageOverrideUpdate {
    pub rules: BTreeMap<String, RuleConfigUpdate>,
}

impl LanguageOverrideUpdate {
    fn apply_to(self, target: &mut LanguageOverride) {
        for (rule_key, rule_update) in self.rules {
            rule_update.apply_to(target.rules.entry(rule_key).or_default());
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    pub max_diagnostics_per_document: usize,
    pub rules: BTreeMap<String, RuleConfig>,
    pub language_overrides: BTreeMap<String, LanguageOverride>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfigUpdate {
    pub max_diagnostics_per_document: Option<usize>,
    pub rules: BTreeMap<String, RuleConfigUpdate>,
    pub language_overrides: BTreeMap<String, LanguageOverrideUpdate>,
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

    pub fn apply_update(&mut self, update: ServerConfigUpdate) {
        if let Some(max_diagnostics_per_document) = update.max_diagnostics_per_document {
            self.max_diagnostics_per_document = max_diagnostics_per_document;
        }

        for (rule_key, rule_update) in update.rules {
            rule_update.apply_to(self.rules.entry(rule_key).or_default());
        }

        for (language, override_update) in update.language_overrides {
            override_update.apply_to(self.language_overrides.entry(language).or_default());
        }
    }
}

impl ServerConfigUpdate {
    pub fn from_value(value: Value) -> Result<Option<Self>> {
        match unwrap_nested(value) {
            Value::Null => Ok(None),
            value => serde_json::from_value(value)
                .context("failed to parse Critters configuration update")
                .map(Some),
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

    use super::{RuleConfig, ServerConfig, ServerConfigUpdate, Severity};

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

    #[test]
    fn configuration_updates_merge_rule_fields_instead_of_replacing_them() {
        let mut config = ServerConfig::default();
        config.rules.insert(
            "00A0".to_string(),
            RuleConfig {
                description: Some("NO-BREAK SPACE".to_string()),
                severity: Some(Severity::Warning),
                class_name: Some("unicode-space".to_string()),
                zero_width: Some(false),
            },
        );

        let update = ServerConfigUpdate::from_value(json!({
            "rules": {
                "00A0": {
                    "severity": "error"
                }
            }
        }))
        .expect("configuration update to parse")
        .expect("update to be present");

        config.apply_update(update);

        assert_eq!(
            config.rules.get("00A0"),
            Some(&RuleConfig {
                description: Some("NO-BREAK SPACE".to_string()),
                severity: Some(Severity::Error),
                class_name: Some("unicode-space".to_string()),
                zero_width: Some(false),
            })
        );
    }

    #[test]
    fn null_configuration_update_resets_to_defaults() {
        let update = ServerConfigUpdate::from_value(json!({ "critters-lsp": null }))
            .expect("null update to parse");

        assert_eq!(update, None);
    }
}
