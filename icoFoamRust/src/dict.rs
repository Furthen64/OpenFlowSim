//! Minimal OpenFOAM dictionary parser.
//!
//! Handles the subset of the format needed for system/ files:
//! comments, key-value pairs, nested sub-dictionaries, and lists.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum DictError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path} at token {pos}: {detail}")]
    Parse {
        path: PathBuf,
        pos: usize,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Tokens(Vec<String>),
    Block(Dict),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Tokens(v) if v.len() == 1 => Some(&v[0]),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        self.as_str().and_then(|s| s.parse().ok())
    }

    pub fn as_usize(&self) -> Option<usize> {
        self.as_str().and_then(|s| s.parse().ok())
    }

    pub fn as_block(&self) -> Option<&Dict> {
        match self {
            Value::Block(d) => Some(d),
            _ => None,
        }
    }

    pub fn tokens(&self) -> &[String] {
        match self {
            Value::Tokens(v) => v,
            Value::Block(_) => &[],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dict {
    pub entries: Vec<(String, Value)>,
}

impl Dict {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(Value::as_usize)
    }

    pub fn get_block(&self, key: &str) -> Option<&Dict> {
        self.get(key).and_then(Value::as_block)
    }
}

pub fn read_file(path: &Path) -> Result<Dict, DictError> {
    let text = fs::read_to_string(path).map_err(|source| DictError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text, path)
}

fn parse(text: &str, path: &Path) -> Result<Dict, DictError> {
    let tokens = tokenize(&strip_comments(text));
    let mut parser = Parser { tokens: &tokens, pos: 0, path };
    // Skip the FoamFile header block if present
    if parser.peek() == Some("FoamFile") {
        parser.advance();
        parser.parse_block()?;
    }
    parser.parse_dict_body()
}

struct Parser<'a> {
    tokens: &'a [String],
    pos: usize,
    path: &'a Path,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn advance(&mut self) -> Option<&'a String> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &str) -> Result<(), DictError> {
        match self.peek() {
            Some(t) if t == expected => {
                self.pos += 1;
                Ok(())
            }
            other => Err(DictError::Parse {
                path: self.path.to_path_buf(),
                pos: self.pos,
                detail: format!("expected '{expected}', found '{other:?}'"),
            }),
        }
    }

    fn parse_dict_body(&mut self) -> Result<Dict, DictError> {
        let mut entries = Vec::new();
        while self.pos < self.tokens.len() {
            if self.peek() == Some("}") {
                break;
            }
            let key = self
                .advance()
                .ok_or_else(|| self.err("unexpected end of input"))?
                .clone();
            let value = self.parse_value()?;
            entries.push((key, value));
        }
        Ok(Dict { entries })
    }

    fn parse_block(&mut self) -> Result<Value, DictError> {
        self.expect("{")?;
        let dict = self.parse_dict_body()?;
        self.expect("}")?;
        // Consume trailing ';' if present (sub-dictionaries often end with ;)
        if self.peek() == Some(";") {
            self.advance();
        }
        Ok(Value::Block(dict))
    }

    fn parse_value(&mut self) -> Result<Value, DictError> {
        match self.peek() {
            Some("{") => self.parse_block(),
            Some("(") => {
                self.advance();
                let mut items = Vec::new();
                while self.peek() != Some(")") {
                    match self.peek() {
                        Some("{") => items.push(self.parse_block()?),
                        _ => {
                            let tok = self
                                .advance()
                                .ok_or_else(|| self.err("unexpected end in list"))?;
                            items.push(Value::Tokens(vec![tok.clone()]));
                        }
                    }
                }
                self.expect(")")?;
                if self.peek() == Some(";") {
                    self.advance();
                }
                Ok(Value::Tokens(
                    items.into_iter().filter_map(|v| match v {
                        Value::Tokens(t) => t.into_iter().next(),
                        _ => None,
                    }).collect(),
                ))
            }
            _ => {
                let mut tokens = Vec::new();
                while self.pos < self.tokens.len()
                    && !matches!(self.peek(), Some(";") | Some("}") | Some(")"))
                {
                    let tok = self
                        .advance()
                        .ok_or_else(|| self.err("unexpected end of input"))?;
                    tokens.push(tok.clone());
                }
                if self.peek() == Some(";") {
                    self.advance();
                }
                // Empty values occur with dictionary references like `$p;`
                Ok(Value::Tokens(tokens))
            }
        }
    }

    fn err(&self, detail: &str) -> DictError {
        DictError::Parse {
            path: self.path.to_path_buf(),
            pos: self.pos,
            detail: detail.to_string(),
        }
    }
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(next) = chars.next() {
                if next == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Tokenize OpenFOAM text. Parentheses directly attached to a preceding word
/// (no whitespace) are kept as part of that token, e.g. `div(phi,U)` stays
/// as one token while `cells (1 2 3)` produces separate tokens.
fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            i += 1;
        } else if "(){}[];".contains(c) {
            // If this is '(' and we're in the middle of a word with no space
            // before it, keep it attached (function-style key like div(phi,U))
            if c == '(' && !word.is_empty() {
                word.push(c);
                i += 1;
                // Consume until matching ')'
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    word.push(chars[i]);
                    i += 1;
                }
            } else {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
                out.push(c.to_string());
                i += 1;
            }
        } else {
            word.push(c);
            i += 1;
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Dict {
        parse(s, Path::new("test")).unwrap()
    }

    #[test]
    fn simple_key_value() {
        let d = parse_str("endTime 0.5;\ndeltaT 0.01;\n");
        assert_eq!(d.get_f64("endTime"), Some(0.5));
        assert_eq!(d.get_f64("deltaT"), Some(0.01));
    }

    #[test]
    fn string_value() {
        let d = parse_str("writeControl timeStep;\n");
        assert_eq!(d.get_str("writeControl"), Some("timeStep"));
    }

    #[test]
    fn nested_block() {
        let d = parse_str("PISO\n{\n    nCorrectors 2;\n    pRefCell 0;\n}\n");
        let piso = d.get_block("PISO").unwrap();
        assert_eq!(piso.get_usize("nCorrectors"), Some(2));
        assert_eq!(piso.get_usize("pRefCell"), Some(0));
    }

    #[test]
    fn foamfile_header_skipped() {
        let text = "FoamFile\n{\n    format ascii;\n    object controlDict;\n}\nstartTime 0;\nendTime 1.0;\n";
        let d = parse_str(text);
        assert!(d.get("format").is_none());
        assert_eq!(d.get_f64("endTime"), Some(1.0));
    }

    #[test]
    fn list_value() {
        let d = parse_str("cells (1 2 3);\n");
        let v = d.get("cells").unwrap();
        assert_eq!(v.tokens(), &vec!["1".to_string(), "2".to_string(), "3".to_string()]);
    }

    #[test]
    fn multi_token_value() {
        let d = parse_str("default Gauss linear orthogonal;\n");
        let v = d.get("default").unwrap();
        assert_eq!(
            v.tokens(),
            &vec![
                "Gauss".to_string(),
                "linear".to_string(),
                "orthogonal".to_string()
            ]
        );
    }

    #[test]
    fn function_style_key() {
        let d = parse_str("divSchemes\n{\n    div(phi,U) Gauss linear;\n}\n");
        let div = d.get_block("divSchemes").unwrap();
        let v = div.get("div(phi,U)").unwrap();
        assert_eq!(v.tokens(), &vec!["Gauss".to_string(), "linear".to_string()]);
    }

    #[test]
    fn dict_reference_empty_value() {
        let d = parse_str("pFinal\n{\n    $p;\n    relTol 0;\n}\n");
        let pf = d.get_block("pFinal").unwrap();
        assert!(pf.get("$p").is_some());
        assert_eq!(pf.get_f64("relTol"), Some(0.0));
    }
}
