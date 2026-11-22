//! Parser for Magic Set Editor's indented package language.

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar(String),
    Map(Vec<(String, Node)>),
    Script(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub fields: Vec<(String, Node)>,
}

impl Document {
    pub fn parse(text: &str) -> Result<Self, String> {
        let lines = physical_lines(text);
        let (fields, next) = parse_block(&lines, 0, 0)?;
        if next != lines.len() {
            return Err(format!("unexpected content at line {}", lines[next].number));
        }
        Ok(Self { fields })
    }

    pub fn all<'a>(&'a self, key: &str) -> Vec<&'a Node> {
        self.fields
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, node)| node)
            .collect()
    }

    pub fn first<'a>(&'a self, key: &str) -> Option<&'a Node> {
        self.all(key).into_iter().next()
    }

    pub fn scalar(&self, key: &str) -> Option<&str> {
        match self.first(key)? {
            Node::Scalar(value) => Some(value.as_str()),
            _ => None,
        }
    }
}

impl Node {
    pub fn map_all<'a>(&'a self, key: &str) -> Vec<&'a Node> {
        match self {
            Node::Map(fields) => fields
                .iter()
                .filter(|(name, _)| name == key)
                .map(|(_, node)| node)
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn map_first<'a>(&'a self, key: &str) -> Option<&'a Node> {
        self.map_all(key).into_iter().next()
    }

    pub fn scalar(&self, key: &str) -> Option<&str> {
        match self.map_first(key)? {
            Node::Scalar(value) => Some(value.as_str()),
            _ => None,
        }
    }
}

struct Phys {
    number: usize,
    indent: usize,
    text: String,
}

fn physical_lines(text: &str) -> Vec<Phys> {
    let mut lines = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let trimmed_end = raw.trim_end_matches(['\r', ' ']);
        if trimmed_end.trim().is_empty() {
            continue;
        }
        let indent = trimmed_end.chars().take_while(|c| *c == '\t').count();
        let text = trimmed_end[indent..].to_string();
        if text.trim_start().starts_with('#') {
            continue;
        }
        lines.push(Phys {
            number: idx + 1,
            indent,
            text,
        });
    }
    lines
}

fn parse_block(lines: &[Phys], mut index: usize, indent: usize) -> Result<(Vec<(String, Node)>, usize), String> {
    let mut fields = Vec::new();
    while index < lines.len() && lines[index].indent >= indent {
        if lines[index].indent > indent {
            return Err(format!("unexpected indent at line {}", lines[index].number));
        }
        let line = &lines[index];
        let Some((key, value)) = split_key(&line.text) else {
            return Err(format!("expected a key at line {}", line.number));
        };
        index += 1;
        let node = if !value.is_empty() {
            Node::Scalar(value.to_string())
        } else if index < lines.len() && lines[index].indent > indent {
            let (node, next) = parse_children(lines, index, indent)?;
            index = next;
            node
        } else {
            Node::Scalar(String::new())
        };
        fields.push((key.to_string(), node));
    }
    Ok((fields, index))
}

fn parse_children(
    lines: &[Phys],
    index: usize,
    parent_indent: usize,
) -> Result<(Node, usize), String> {
    let child_indent = lines[index].indent;
    if block_is_map(lines, index, child_indent) {
        let (children, next) = parse_block(lines, index, child_indent)?;
        Ok((Node::Map(children), next))
    } else {
        let mut script = Vec::new();
        let mut next = index;
        while next < lines.len() && lines[next].indent > parent_indent {
            script.push(lines[next].text.clone());
            next += 1;
        }
        Ok((Node::Script(script.join("\n")), next))
    }
}

/// A data block is a run of fields. A script may start with `include file:`
/// and later assign with `:=`, so one non-field line makes the whole block a script.
fn block_is_map(lines: &[Phys], mut index: usize, indent: usize) -> bool {
    let mut saw_line = false;
    while index < lines.len() && lines[index].indent >= indent {
        if lines[index].indent == indent {
            saw_line = true;
            if !looks_like_key(&lines[index].text) {
                return false;
            }
        }
        index += 1;
    }
    saw_line
}

fn split_key(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    // `:=` is a script assignment. The colon comes first.
    if text.as_bytes().get(colon + 1) == Some(&b'=') {
        return None;
    }
    let key = text[..colon].trim();
    if key.is_empty() || !is_key(key) {
        return None;
    }
    Some((key, text[colon + 1..].trim()))
}

fn looks_like_key(text: &str) -> bool {
    split_key(text).is_some()
}

fn is_key(key: &str) -> bool {
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '/' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_file_keeps_every_card_block() {
        let text = "\
mse version: 2.0.1
stylesheet: magic-m15-altered.mse-style
set info:
\tname: Brass Owl
\tset code: STM
card:
\tname: Brass Owl
\tcasting cost: 2U
\trule text: Flying
\tstyling:
\t\tpromo: no
card:
\tname: Zap
\ttype: Instant
";
        let doc = Document::parse(text).unwrap();
        assert_eq!(doc.scalar("stylesheet"), Some("magic-m15-altered.mse-style"));
        let info = doc.first("set info").unwrap();
        assert_eq!(info.scalar("name"), Some("Brass Owl"));
        assert_eq!(info.scalar("set code"), Some("STM"));
        let cards = doc.all("card");
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].scalar("casting cost"), Some("2U"));
        assert_eq!(cards[0].scalar("rule text"), Some("Flying"));
        assert_eq!(cards[0].map_first("styling").unwrap().scalar("promo"), Some("no"));
        assert_eq!(cards[1].scalar("name"), Some("Zap"));
    }

    #[test]
    fn a_script_block_stays_text() {
        let text = "\
init script:
\twidth := { if input == \"wide\" then 10 else 3 }
card width: 375
";
        let doc = Document::parse(text).unwrap();
        match doc.first("init script").unwrap() {
            Node::Script(body) => assert!(body.contains("width :=")),
            other => panic!("expected a script, got {other:?}"),
        }
        assert_eq!(doc.scalar("card width"), Some("375"));
    }
}
