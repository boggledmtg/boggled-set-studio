//! Split a Magic type line into supertypes, types, and subtypes.

const SUPERTYPES: &[&str] = &["Basic", "Legendary", "Ongoing", "Snow", "Token", "World"];

const CARD_TYPES: &[&str] = &[
    "Artifact",
    "Battle",
    "Conspiracy",
    "Creature",
    "Dungeon",
    "Enchantment",
    "Instant",
    "Kindred",
    "Land",
    "Phenomenon",
    "Plane",
    "Planeswalker",
    "Scheme",
    "Sorcery",
    "Tribal",
    "Vanguard",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParts {
    pub supertypes: Vec<String>,
    pub types: Vec<String>,
    pub subtypes: Vec<String>,
    pub line: String,
}

pub fn parse_type_line(input: &str) -> TypeParts {
    let normalized = input
        .trim()
        .replace(" — ", " — ")
        .replace(" - ", " — ")
        .replace(" -- ", " — ");
    let (front, back) = match normalized.split_once(" — ") {
        Some((front, back)) => (front.trim(), back.trim()),
        None => (normalized.as_str(), ""),
    };
    let mut supertypes = Vec::new();
    let mut types = Vec::new();
    for word in front.split_whitespace() {
        if SUPERTYPES.iter().any(|s| s.eq_ignore_ascii_case(word)) {
            supertypes.push(canonical_word(word, SUPERTYPES));
        } else if CARD_TYPES.iter().any(|s| s.eq_ignore_ascii_case(word)) {
            types.push(canonical_word(word, CARD_TYPES));
        } else if types.is_empty() {
            types.push(word.to_string());
        } else {
            // A word after a card type and before the dash is still a card type
            // only when it is in the known list. Anything else is kept as a type
            // so a custom card type is not dropped.
            types.push(word.to_string());
        }
    }
    let subtypes = back
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect::<Vec<_>>();
    let mut line = String::new();
    line.push_str(&supertypes.join(" "));
    if !supertypes.is_empty() && !types.is_empty() {
        line.push(' ');
    }
    line.push_str(&types.join(" "));
    if !subtypes.is_empty() {
        if !line.is_empty() {
            line.push_str(" — ");
        }
        line.push_str(&subtypes.join(" "));
    }
    TypeParts {
        supertypes,
        types,
        subtypes,
        line,
    }
}

fn canonical_word(word: &str, known: &[&str]) -> String {
    known
        .iter()
        .find(|s| s.eq_ignore_ascii_case(word))
        .map(|s| (*s).to_string())
        .unwrap_or_else(|| word.to_string())
}

pub fn is_creature(types: &[String]) -> bool {
    types.iter().any(|t| t == "Creature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legendary_artifact_creature_splits() {
        let parts = parse_type_line("Legendary Artifact Creature — Bird");
        assert_eq!(parts.supertypes, vec!["Legendary".to_string()]);
        assert_eq!(
            parts.types,
            vec!["Artifact".to_string(), "Creature".to_string()]
        );
        assert_eq!(parts.subtypes, vec!["Bird".to_string()]);
        assert!(is_creature(&parts.types));
    }

    #[test]
    fn ascii_dash_is_normalized() {
        let parts = parse_type_line("Creature - Goblin");
        assert_eq!(parts.line, "Creature — Goblin");
        assert_eq!(parts.subtypes, vec!["Goblin".to_string()]);
    }

    #[test]
    fn instant_is_not_a_creature() {
        let parts = parse_type_line("Instant");
        assert!(!is_creature(&parts.types));
    }
}
