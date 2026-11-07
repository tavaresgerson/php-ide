//! Tipos comuns de valor — `T-DB-001-02` (DB-001).
//!
//! Preserva NULL, binário, datas e decimais sem converter indiscriminadamente
//! para String: decimais vêm como texto canônico (precisão arbitrária), datas
//! como ISO 8601, binários como bytes. Display só na camada de UI.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Decimal de precisão arbitrária, representação canônica (ex.: "3.14").
    Decimal(String),
    Text(String),
    Bytes(Vec<u8>),
    Date(String),
    Time(String),
    Timestamp(String),
}

impl Value {
    /// Renderização de exibição (grade/Text view) — binário em hex resumido.
    pub fn display(&self) -> String {
        match self {
            Value::Null => "NULL".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Decimal(d) => d.clone(),
            Value::Text(t) => t.clone(),
            Value::Bytes(b) => {
                let head: String = b.iter().take(8).map(|x| format!("{x:02X}")).collect();
                if b.len() > 8 {
                    format!("{head}… ({} B)", b.len())
                } else {
                    head
                }
            }
            Value::Date(d) | Value::Time(d) | Value::Timestamp(d) => d.clone(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Valor para bind no driver SQLite (datas/timestamps como ISO texto).
    pub fn to_sql(&self) -> rusqlite::types::Value {
        match self {
            Value::Null => rusqlite::types::Value::Null,
            Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
            Value::Int(i) => rusqlite::types::Value::Integer(*i),
            Value::Float(f) => rusqlite::types::Value::Real(*f),
            Value::Decimal(d) => rusqlite::types::Value::Text(d.clone()),
            Value::Text(t) => rusqlite::types::Value::Text(t.clone()),
            Value::Bytes(b) => rusqlite::types::Value::Blob(b.clone()),
            Value::Date(d) | Value::Time(d) | Value::Timestamp(d) => {
                rusqlite::types::Value::Text(d.clone())
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_null_binary_date_decimal() {
        assert_eq!(Value::Null.display(), "NULL");
        assert_eq!(Value::Bytes(vec![0xDE, 0xAD]).display(), "DEAD");
        let big = Value::Bytes(vec![1u8; 32]);
        assert!(big.display().contains("… (32 B)"));
        assert_eq!(Value::Date("2024-01-02".into()).display(), "2024-01-02");
        assert_eq!(
            Value::Decimal("12345678901234567890.123".into()).display(),
            "12345678901234567890.123"
        );
        // Não colapsa tudo para String no modelo.
        assert!(matches!(Value::Decimal("1.0".into()), Value::Decimal(_)));
        assert!(matches!(Value::Bytes(vec![]), Value::Bytes(_)));
    }

    #[test]
    fn to_sql_bindings() {
        assert!(matches!(Value::Null.to_sql(), rusqlite::types::Value::Null));
        assert!(matches!(
            Value::Int(7).to_sql(),
            rusqlite::types::Value::Integer(7)
        ));
        assert!(matches!(
            Value::Bytes(vec![1]).to_sql(),
            rusqlite::types::Value::Blob(_)
        ));
        assert!(matches!(
            Value::Timestamp("2024-01-01T00:00:00Z".into()).to_sql(),
            rusqlite::types::Value::Text(_)
        ));
    }
}
