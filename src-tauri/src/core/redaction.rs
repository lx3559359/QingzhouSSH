use serde_json::{Map, Value};

pub const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    runtime_secrets: Vec<String>,
}

impl Redactor {
    pub fn new<I, S>(runtime_secrets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut runtime_secrets = runtime_secrets
            .into_iter()
            .map(Into::into)
            .filter(|secret| !secret.is_empty())
            .collect::<Vec<_>>();
        runtime_secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        runtime_secrets.dedup();
        Self { runtime_secrets }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut output = redact_private_key_blocks(input);
        for secret in &self.runtime_secrets {
            output = output.replace(secret, REDACTED);
        }
        redact_named_assignments(&output)
    }

    pub fn redact_json(&self, value: &Value) -> Value {
        self.redact_json_with_key(None, value)
    }

    fn redact_json_with_key(&self, key: Option<&str>, value: &Value) -> Value {
        if key.is_some_and(is_sensitive_key) {
            return Value::String(REDACTED.into());
        }
        match value {
            Value::String(value) => Value::String(self.redact(value)),
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| self.redact_json_with_key(None, value))
                    .collect(),
            ),
            Value::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), self.redact_json_with_key(Some(key), value)))
                    .collect::<Map<_, _>>(),
            ),
            other => other.clone(),
        }
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    normalized.contains("password")
        || normalized.contains("passphrase")
        || normalized.contains("private_key")
        || normalized.contains("credential")
        || normalized == "token"
        || normalized.ends_with("_token")
        || normalized == "secret"
        || normalized.ends_with("_secret")
}

fn redact_private_key_blocks(input: &str) -> String {
    let mut remaining = input;
    let mut output = String::with_capacity(input.len());
    while let Some(begin) = remaining.find("-----BEGIN ") {
        output.push_str(&remaining[..begin]);
        let block = &remaining[begin..];
        let header_prefix = "-----BEGIN ".len();
        let Some(header_end) = block[header_prefix..].find("-----") else {
            output.push_str(block);
            return output;
        };
        let header_end = header_prefix + header_end + 5;
        let header = &block[..header_end];
        if !header.contains("PRIVATE KEY") {
            output.push_str(&block[..header_end]);
            remaining = &block[header_end..];
            continue;
        }
        let after_header = &block[header_end..];
        let Some(end_start) = after_header.find("-----END ") else {
            output.push_str(REDACTED);
            return output;
        };
        let end_block = &after_header[end_start..];
        let Some(end_marker) = end_block[5..].find("-----") else {
            output.push_str(REDACTED);
            return output;
        };
        output.push_str(REDACTED);
        remaining = &end_block[5 + end_marker + 5..];
    }
    output.push_str(remaining);
    output
}

fn redact_named_assignments(input: &str) -> String {
    const MARKERS: [&str; 6] = [
        "password=",
        "password:",
        "passphrase=",
        "passphrase:",
        "token=",
        "token:",
    ];
    let mut output = input.to_string();
    for marker in MARKERS {
        let mut search_from = 0;
        loop {
            let lowered = output[search_from..].to_ascii_lowercase();
            let Some(relative) = lowered.find(marker) else {
                break;
            };
            let value_start = search_from + relative + marker.len();
            let trimmed = output[value_start..]
                .char_indices()
                .find(|(_, character)| !character.is_whitespace())
                .map(|(offset, _)| value_start + offset)
                .unwrap_or(output.len());
            let value_end = output[trimmed..]
                .char_indices()
                .find(|(_, character)| {
                    character.is_whitespace() || *character == ',' || *character == ';'
                })
                .map(|(offset, _)| trimmed + offset)
                .unwrap_or(output.len());
            if trimmed < value_end && &output[trimmed..value_end] != REDACTED {
                output.replace_range(trimmed..value_end, REDACTED);
                search_from = trimmed + REDACTED.len();
            } else {
                search_from = value_end.max(value_start);
            }
            if search_from >= output.len() {
                break;
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_non_secret_pem_blocks() {
        let input = "-----BEGIN CERTIFICATE-----\npublic\n-----END CERTIFICATE-----";
        assert_eq!(Redactor::default().redact(input), input);
    }
}
