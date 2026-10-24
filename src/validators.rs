use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Validator {
    Boolean,
    Integer {
        min: Option<i64>,
        max: Option<i64>,
    },
    Float {
        min: Option<f64>,
        max: Option<f64>,
    },
    String {
        min_len: Option<usize>,
        max_len: Option<usize>,
    },
    Choice {
        values: Vec<Value>,
    },
    MultiChoice {
        values: Vec<Value>,
    },
    Series {
        item: Box<Validator>,
        min_len: Option<usize>,
        max_len: Option<usize>,
    },
    Link,
    TelegramId,
    Hidden {
        inner: Box<Validator>,
    },
    Union {
        items: Vec<Validator>,
    },
    None,
}
impl Validator {
    pub fn validate(&self, value: Value) -> Result<Value> {
        self.validate_inner(value, 0)
    }
    fn validate_inner(&self, value: Value, depth: usize) -> Result<Value> {
        ensure!(depth < 16, "validator nesting too deep");
        let value = match self {
            Self::Boolean => match value {
                Value::Bool(_) => value,
                Value::Number(ref n) if n.as_i64() == Some(1) => Value::Bool(true),
                Value::Number(ref n) if n.as_i64() == Some(0) => Value::Bool(false),
                Value::String(ref s)
                    if matches!(s.to_lowercase().as_str(), "true" | "yes" | "on" | "1" | "y") =>
                {
                    Value::Bool(true)
                }
                Value::String(ref s)
                    if matches!(
                        s.to_lowercase().as_str(),
                        "false" | "no" | "off" | "0" | "n"
                    ) =>
                {
                    Value::Bool(false)
                }
                _ => anyhow::bail!("expected boolean"),
            },
            Self::Integer { min, max } => {
                let n = value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
                    .ok_or_else(|| anyhow::anyhow!("expected integer"))?;
                ensure!(
                    min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m),
                    "integer outside bounds"
                );
                Value::from(n)
            }
            Self::Float { min, max } => {
                let n = value
                    .as_f64()
                    .or_else(|| {
                        value
                            .as_str()
                            .and_then(|s| s.replace(',', ".").parse().ok())
                    })
                    .ok_or_else(|| anyhow::anyhow!("expected finite number"))?;
                ensure!(
                    n.is_finite() && min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m),
                    "number outside bounds"
                );
                serde_json::json!(n)
            }
            Self::String { min_len, max_len } => {
                let s = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                let n = s.chars().count();
                ensure!(
                    min_len.is_none_or(|m| n >= m) && max_len.is_none_or(|m| n <= m),
                    "string length outside bounds"
                );
                Value::String(s)
            }
            Self::Choice { values } => {
                ensure!(values.contains(&value), "unknown choice");
                value
            }
            Self::MultiChoice { values } => {
                let array = match value {
                    Value::Array(a) => a,
                    v => vec![v],
                };
                let mut out = vec![];
                for v in array {
                    ensure!(values.contains(&v), "unknown choice");
                    if !out.contains(&v) {
                        out.push(v);
                    }
                }
                Value::Array(out)
            }
            Self::Series {
                item,
                min_len,
                max_len,
            } => {
                let array = match value {
                    Value::Array(a) => a,
                    Value::String(s) => s
                        .split(',')
                        .map(|v| Value::String(v.trim().into()))
                        .collect(),
                    _ => anyhow::bail!("expected list"),
                };
                let array: Vec<_> = array
                    .into_iter()
                    .filter(|v| !v.is_null() && v.as_str() != Some(""))
                    .collect();
                ensure!(
                    min_len.is_none_or(|m| array.len() >= m)
                        && max_len.is_none_or(|m| array.len() <= m),
                    "list length outside bounds"
                );
                Value::Array(
                    array
                        .into_iter()
                        .map(|v| item.validate_inner(v, depth + 1))
                        .collect::<Result<_>>()?,
                )
            }
            Self::Link => {
                let raw = value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("expected URL"))?;
                let url = reqwest::Url::parse(raw)?;
                ensure!(
                    matches!(url.scheme(), "https" | "http"),
                    "expected HTTP(S) URL"
                );
                value
            }
            Self::TelegramId => {
                let n = value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| anyhow::anyhow!("expected Telegram ID"))?;
                ensure!(n != 0, "Telegram ID cannot be zero");
                Value::from(n)
            }
            Self::Hidden { inner } => inner.validate_inner(value, depth + 1)?,
            Self::Union { items } => {
                let mut matched = None;
                for item in items {
                    if let Ok(v) = item.validate_inner(value.clone(), depth + 1) {
                        matched = Some(v);
                        break;
                    }
                }
                matched.ok_or_else(|| anyhow::anyhow!("no matching validator"))?
            }
            Self::None => {
                ensure!(
                    value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty()),
                    "expected null"
                );
                Value::Null
            }
        };
        Ok(value)
    }
    pub fn hidden(&self) -> bool {
        matches!(self, Self::Hidden { .. })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bounds_and_false_values() {
        assert_eq!(
            Validator::Integer {
                min: Some(0),
                max: Some(5)
            }
            .validate(json!("0"))
            .unwrap(),
            0
        );
        assert!(
            Validator::Integer {
                min: Some(0),
                max: None
            }
            .validate(json!(-1))
            .is_err()
        );
        assert_eq!(Validator::Boolean.validate(json!("false")).unwrap(), false);
        assert!(
            Validator::Float {
                min: None,
                max: None
            }
            .validate(json!("NaN"))
            .is_err()
        );
    }
    #[test]
    fn series_retains_zero_and_false() {
        let v = Validator::Series {
            item: Box::new(Validator::Boolean),
            min_len: None,
            max_len: Some(4),
        };
        assert_eq!(
            v.validate(json!([false, 0, true])).unwrap(),
            json!([false, false, true])
        );
    }
    #[test]
    fn choices_hidden_and_union() {
        assert!(
            Validator::Hidden {
                inner: Box::new(Validator::Boolean)
            }
            .hidden()
        );
        assert_eq!(
            Validator::MultiChoice {
                values: vec![json!(1), json!(2)]
            }
            .validate(json!([2, 1, 2]))
            .unwrap(),
            json!([2, 1])
        );
        assert!(
            Validator::Choice {
                values: vec![json!(1)]
            }
            .validate(json!(2))
            .is_err()
        );
    }
}
