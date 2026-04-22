use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};

use crate::config::{RuleConfig, ServerConfig, Severity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveRule {
    pub code_point: u32,
    pub description: String,
    pub severity: Severity,
    pub class_name: String,
    pub zero_width: bool,
}

#[derive(Clone, Debug)]
struct BuiltinRuleSpec {
    key: &'static str,
    description: &'static str,
    severity: Severity,
    class_name: &'static str,
    zero_width: bool,
}

const BUILTIN_RULES: &[BuiltinRuleSpec] = &[
    BuiltinRuleSpec {
        key: "0003",
        description: "END OF TEXT",
        severity: Severity::Warning,
        class_name: "control",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "000B",
        description: "LINE TABULATION",
        severity: Severity::Warning,
        class_name: "control",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "00A0",
        description: "NO-BREAK SPACE",
        severity: Severity::Info,
        class_name: "spacing",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "00AD",
        description: "SOFT HYPHEN",
        severity: Severity::Info,
        class_name: "format",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "034F",
        description: "COMBINING GRAPHEME JOINER",
        severity: Severity::Warning,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "061C",
        description: "ARABIC LETTER MARK",
        severity: Severity::Error,
        class_name: "bidi",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "200B",
        description: "ZERO WIDTH SPACE",
        severity: Severity::Error,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "200C",
        description: "ZERO WIDTH NON-JOINER",
        severity: Severity::Warning,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "200D",
        description: "ZERO WIDTH JOINER",
        severity: Severity::Warning,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "200E",
        description: "LEFT-TO-RIGHT MARK",
        severity: Severity::Error,
        class_name: "bidi",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "200F",
        description: "RIGHT-TO-LEFT MARK",
        severity: Severity::Error,
        class_name: "bidi",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "2013",
        description: "EN DASH",
        severity: Severity::Warning,
        class_name: "typography",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "2018",
        description: "LEFT SINGLE QUOTATION MARK",
        severity: Severity::Warning,
        class_name: "typography",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "2019",
        description: "RIGHT SINGLE QUOTATION MARK",
        severity: Severity::Warning,
        class_name: "typography",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "201C",
        description: "LEFT DOUBLE QUOTATION MARK",
        severity: Severity::Warning,
        class_name: "typography",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "201D",
        description: "RIGHT DOUBLE QUOTATION MARK",
        severity: Severity::Warning,
        class_name: "typography",
        zero_width: false,
    },
    BuiltinRuleSpec {
        key: "2028",
        description: "LINE SEPARATOR",
        severity: Severity::Error,
        class_name: "separator",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "2029",
        description: "PARAGRAPH SEPARATOR",
        severity: Severity::Error,
        class_name: "separator",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "202A-202E",
        description: "BIDIRECTIONAL FORMATTING OR OVERRIDE CONTROL",
        severity: Severity::Error,
        class_name: "bidi",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "2060",
        description: "WORD JOINER",
        severity: Severity::Warning,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "2066-2069",
        description: "BIDIRECTIONAL ISOLATE CONTROL",
        severity: Severity::Error,
        class_name: "bidi",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "FEFF",
        description: "ZERO WIDTH NO-BREAK SPACE OR BYTE ORDER MARK",
        severity: Severity::Error,
        class_name: "zero-width",
        zero_width: true,
    },
    BuiltinRuleSpec {
        key: "FFFC",
        description: "OBJECT REPLACEMENT CHARACTER",
        severity: Severity::Error,
        class_name: "object",
        zero_width: true,
    },
];

pub fn effective_rules(
    config: &ServerConfig,
    language_id: &str,
) -> Result<BTreeMap<u32, EffectiveRule>> {
    let mut resolved = builtin_rules()?;
    apply_rule_set(&mut resolved, &config.rules)?;

    if let Some(language_override) = config.language_overrides.get(language_id) {
        apply_rule_set(&mut resolved, &language_override.rules)?;
    }

    Ok(resolved)
}

fn builtin_rules() -> Result<BTreeMap<u32, EffectiveRule>> {
    let mut resolved = BTreeMap::new();

    for spec in BUILTIN_RULES {
        let config = RuleConfig {
            description: Some(spec.description.to_string()),
            severity: Some(spec.severity.clone()),
            class_name: Some(spec.class_name.to_string()),
            zero_width: Some(spec.zero_width),
        };
        apply_single_rule(&mut resolved, spec.key, &config)?;
    }

    Ok(resolved)
}

fn apply_rule_set(
    target: &mut BTreeMap<u32, EffectiveRule>,
    rules: &BTreeMap<String, RuleConfig>,
) -> Result<()> {
    for (key, config) in rules {
        apply_single_rule(target, key, config)?;
    }

    Ok(())
}

fn apply_single_rule(
    target: &mut BTreeMap<u32, EffectiveRule>,
    key: &str,
    config: &RuleConfig,
) -> Result<()> {
    let (start, end) = parse_rule_key(key)?;

    for code_point in start..=end {
        if let Some(Severity::None) = config.severity {
            target.remove(&code_point);
            continue;
        }

        let existing = target.get(&code_point);
        let description = config
            .description
            .clone()
            .or_else(|| existing.map(|value| value.description.clone()))
            .unwrap_or_else(|| format!("U+{:04X}", code_point));
        let severity = config
            .severity
            .clone()
            .or_else(|| existing.map(|value| value.severity.clone()))
            .unwrap_or(Severity::Error);
        let class_name = config
            .class_name
            .clone()
            .or_else(|| existing.map(|value| value.class_name.clone()))
            .unwrap_or_else(|| "unicode".to_string());
        let zero_width = config
            .zero_width
            .or_else(|| existing.map(|value| value.zero_width))
            .unwrap_or(false);

        target.insert(
            code_point,
            EffectiveRule {
                code_point,
                description,
                severity,
                class_name,
                zero_width,
            },
        );
    }

    Ok(())
}

fn parse_rule_key(key: &str) -> Result<(u32, u32)> {
    let cleaned = key.trim().to_ascii_uppercase();
    let mut parts = cleaned.split('-');
    let start = parse_scalar(
        parts
            .next()
            .ok_or_else(|| anyhow!("missing start code point"))?,
    )?;
    let end = match parts.next() {
        Some(value) => parse_scalar(value)?,
        None => start,
    };

    if parts.next().is_some() {
        bail!("invalid rule key {key}: too many separators");
    }

    if start > end {
        bail!("invalid rule key {key}: range start was after range end");
    }

    Ok((start, end))
}

fn parse_scalar(value: &str) -> Result<u32> {
    let code_point = u32::from_str_radix(value, 16)
        .map_err(|error| anyhow!("invalid hexadecimal code point {value}: {error}"))?;
    if code_point > 0x10FFFF {
        bail!("code point U+{code_point:04X} was outside Unicode range");
    }
    if char::from_u32(code_point).is_none() {
        bail!("code point U+{code_point:04X} was not a valid Unicode scalar value");
    }
    Ok(code_point)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::{LanguageOverride, RuleConfig, ServerConfig, Severity};

    use super::effective_rules;

    #[test]
    fn range_overrides_expand_to_individual_code_points() {
        let mut config = ServerConfig::default();
        config.rules.insert(
            "0080-0082".to_string(),
            RuleConfig {
                description: Some("LATIN-1 SUPPLEMENT".to_string()),
                severity: Some(Severity::Error),
                class_name: Some("latin-1".to_string()),
                zero_width: Some(false),
            },
        );

        let rules = effective_rules(&config, "plaintext").expect("rules to build");
        assert!(rules.contains_key(&0x0080));
        assert!(rules.contains_key(&0x0081));
        assert!(rules.contains_key(&0x0082));
    }

    #[test]
    fn language_override_can_disable_default_rule() {
        let mut config = ServerConfig::default();
        config.language_overrides.insert(
            "markdown".to_string(),
            LanguageOverride {
                rules: BTreeMap::from([(
                    "00A0".to_string(),
                    RuleConfig {
                        description: None,
                        severity: Some(Severity::None),
                        class_name: None,
                        zero_width: None,
                    },
                )]),
            },
        );

        let markdown_rules = effective_rules(&config, "markdown").expect("markdown rules to build");
        let rust_rules = effective_rules(&config, "rust").expect("rust rules to build");

        assert!(!markdown_rules.contains_key(&0x00A0));
        assert!(rust_rules.contains_key(&0x00A0));
    }
}
