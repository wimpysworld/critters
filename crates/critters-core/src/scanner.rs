use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range as LspRange};
use std::collections::BTreeMap;

use crate::config::Severity;
use crate::rules::EffectiveRule;

#[derive(Clone, Debug)]
pub struct Finding {
    pub range: LspRange,
    pub severity: Severity,
    pub message: String,
    pub hover: String,
    pub fix_title: String,
    pub replacement: String,
}

#[derive(Clone, Debug)]
struct RunItem {
    code_point: u32,
    description: String,
    severity: Severity,
    class_name: String,
    zero_width: bool,
    replacement: String,
}

#[derive(Clone, Debug)]
struct PendingRun {
    start: Position,
    end: Position,
    end_byte: usize,
    items: Vec<RunItem>,
}

pub fn scan(
    text: &str,
    rules: &BTreeMap<u32, EffectiveRule>,
    max_diagnostics: usize,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut pending: Option<PendingRun> = None;
    let mut line = 0u32;
    let mut character = 0u32;

    for (byte_index, ch) in text.char_indices() {
        let start = Position::new(line, character);
        let next_position = advance_position(start, ch);
        let next_byte = byte_index + ch.len_utf8();

        if let Some(rule) = rules.get(&(ch as u32)) {
            let item = RunItem {
                code_point: rule.code_point,
                description: rule.description.clone(),
                severity: rule.severity.clone(),
                class_name: rule.class_name.clone(),
                zero_width: rule.zero_width,
                replacement: replacement_for(ch).to_string(),
            };

            match pending.as_mut() {
                Some(run) if run.end_byte == byte_index => {
                    run.end = next_position;
                    run.end_byte = next_byte;
                    run.items.push(item);
                }
                _ => {
                    flush_pending(&mut pending, &mut findings, max_diagnostics);
                    pending = Some(PendingRun {
                        start,
                        end: next_position,
                        end_byte: next_byte,
                        items: vec![item],
                    });
                }
            }
        } else {
            flush_pending(&mut pending, &mut findings, max_diagnostics);
        }

        line = next_position.line;
        character = next_position.character;

        if findings.len() >= max_diagnostics {
            return findings;
        }
    }

    flush_pending(&mut pending, &mut findings, max_diagnostics);
    findings
}

pub fn to_diagnostics(findings: &[Finding]) -> Vec<Diagnostic> {
    findings
        .iter()
        .map(|finding| Diagnostic {
            range: finding.range,
            severity: Some(to_lsp_severity(&finding.severity)),
            source: Some("critters".to_string()),
            message: finding.message.clone(),
            ..Diagnostic::default()
        })
        .collect()
}

pub fn contains(range: &LspRange, position: Position) -> bool {
    compare_position(position, range.start) >= 0 && compare_position(position, range.end) < 0
}

fn flush_pending(
    pending: &mut Option<PendingRun>,
    findings: &mut Vec<Finding>,
    max_diagnostics: usize,
) {
    if findings.len() >= max_diagnostics {
        *pending = None;
        return;
    }

    if let Some(run) = pending.take() {
        findings.push(build_finding(run));
    }
}

fn build_finding(run: PendingRun) -> Finding {
    let range = LspRange {
        start: run.start,
        end: run.end,
    };

    let severity = run
        .items
        .iter()
        .max_by_key(|item| item.severity.rank())
        .map(|item| item.severity.clone())
        .unwrap_or(Severity::Warning);

    let mut groups: Vec<(RunItem, usize)> = Vec::new();
    for item in run.items {
        if let Some((last, count)) = groups.last_mut() {
            if last.code_point == item.code_point {
                *count += 1;
                continue;
            }
        }
        groups.push((item, 1));
    }

    let message = if groups.len() == 1 {
        let (item, count) = &groups[0];
        if *count == 1 {
            format!("{} (U+{:04X})", item.description, item.code_point)
        } else {
            format!(
                "{} × {} (U+{:04X})",
                count, item.description, item.code_point
            )
        }
    } else {
        let details = groups
            .iter()
            .map(group_label)
            .collect::<Vec<_>>()
            .join(", ");
        format!("Suspicious Unicode run: {details}")
    };

    let mut classes = groups
        .iter()
        .map(|(item, _)| item.class_name.clone())
        .collect::<Vec<_>>();
    classes.sort();
    classes.dedup();

    let zero_width = groups.iter().any(|(item, _)| item.zero_width);
    let replacement = groups
        .iter()
        .map(|(item, count)| item.replacement.repeat(*count))
        .collect::<Vec<_>>()
        .join("");
    let fix_title = if replacement.is_empty() {
        "Remove suspicious Unicode characters".to_string()
    } else {
        "Replace suspicious Unicode characters with safe ASCII".to_string()
    };
    let mut hover_lines = vec![
        "**Critters**".to_string(),
        format!("- Severity: `{}`", severity.as_str()),
        format!("- Classes: `{}`", classes.join(", ")),
        format!(
            "- Zero-width content: `{}`",
            if zero_width { "yes" } else { "no" }
        ),
        "- Characters:".to_string(),
    ];
    hover_lines.extend(
        groups
            .iter()
            .map(|group| format!("  - {}", group_label(group))),
    );

    Finding {
        range,
        severity,
        message,
        hover: hover_lines.join("\n"),
        fix_title,
        replacement,
    }
}

fn replacement_for(ch: char) -> &'static str {
    match ch {
        '\u{00A0}' => " ",
        '\u{2013}' => "-",
        '\u{2018}' | '\u{2019}' => "'",
        '\u{201C}' | '\u{201D}' => "\"",
        _ => "",
    }
}

fn group_label((item, count): &(RunItem, usize)) -> String {
    if *count == 1 {
        format!("{} (U+{:04X})", item.description, item.code_point)
    } else {
        format!(
            "{} × {} (U+{:04X})",
            count, item.description, item.code_point
        )
    }
}

fn advance_position(position: Position, ch: char) -> Position {
    if ch == '\n' {
        Position::new(position.line + 1, 0)
    } else {
        Position::new(position.line, position.character + ch.len_utf16() as u32)
    }
}

fn compare_position(left: Position, right: Position) -> i8 {
    match (
        left.line.cmp(&right.line),
        left.character.cmp(&right.character),
    ) {
        (std::cmp::Ordering::Less, _) => -1,
        (std::cmp::Ordering::Greater, _) => 1,
        (_, std::cmp::Ordering::Less) => -1,
        (_, std::cmp::Ordering::Greater) => 1,
        _ => 0,
    }
}

fn to_lsp_severity(severity: &Severity) -> DiagnosticSeverity {
    match severity {
        Severity::None => DiagnosticSeverity::HINT,
        Severity::Info => DiagnosticSeverity::INFORMATION,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Error => DiagnosticSeverity::ERROR,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::config::Severity;
    use crate::rules::EffectiveRule;

    use super::{contains, scan};
    use lsp_types::Position;

    #[test]
    fn contiguous_gremlins_are_grouped_into_one_finding() {
        let rules = BTreeMap::from([
            (
                0x200B,
                EffectiveRule {
                    code_point: 0x200B,
                    description: "ZERO WIDTH SPACE".to_string(),
                    severity: Severity::Error,
                    class_name: "zero-width".to_string(),
                    zero_width: true,
                },
            ),
            (
                0x00A0,
                EffectiveRule {
                    code_point: 0x00A0,
                    description: "NO-BREAK SPACE".to_string(),
                    severity: Severity::Info,
                    class_name: "spacing".to_string(),
                    zero_width: false,
                },
            ),
        ]);

        let findings = scan("a\u{200B}\u{00A0}b", &rules, 50);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("Suspicious Unicode run"));
    }

    #[test]
    fn findings_carry_safe_quick_fix_replacements() {
        let rules = BTreeMap::from([
            (
                0x00A0,
                EffectiveRule {
                    code_point: 0x00A0,
                    description: "NO-BREAK SPACE".to_string(),
                    severity: Severity::Info,
                    class_name: "spacing".to_string(),
                    zero_width: false,
                },
            ),
            (
                0x200B,
                EffectiveRule {
                    code_point: 0x200B,
                    description: "ZERO WIDTH SPACE".to_string(),
                    severity: Severity::Error,
                    class_name: "zero-width".to_string(),
                    zero_width: true,
                },
            ),
        ]);

        let findings = scan("a\u{00A0}\u{200B}b", &rules, 50);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].replacement, " ");
        assert_eq!(
            findings[0].fix_title,
            "Replace suspicious Unicode characters with safe ASCII"
        );
    }

    #[test]
    fn ranges_are_half_open_for_hover_lookups() {
        let rules = BTreeMap::from([(
            0x200B,
            EffectiveRule {
                code_point: 0x200B,
                description: "ZERO WIDTH SPACE".to_string(),
                severity: Severity::Error,
                class_name: "zero-width".to_string(),
                zero_width: true,
            },
        )]);

        let findings = scan("\u{200B}", &rules, 50);
        let finding = &findings[0];
        assert!(contains(&finding.range, Position::new(0, 0)));
        assert!(!contains(&finding.range, Position::new(0, 1)));
    }
}
