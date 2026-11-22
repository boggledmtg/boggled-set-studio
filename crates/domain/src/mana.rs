//! Mana cost parsing and the numbers derived from it.

#[derive(Debug, Clone, PartialEq)]
pub struct ManaInfo {
    pub canonical: String,
    pub mana_value: f64,
    pub colors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    Generic(u32),
    Color(char),
    Hybrid(Vec<AtomPart>),
    Special(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AtomPart {
    Generic(u32),
    Color(char),
    Phyrexian,
}

const COLOR_ORDER: [char; 5] = ['W', 'U', 'B', 'R', 'G'];

pub fn parse_mana(input: &str) -> ManaInfo {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ManaInfo {
            canonical: String::new(),
            mana_value: 0.0,
            colors: Vec::new(),
        };
    }
    let atoms = if trimmed.contains('{') {
        parse_braced(trimmed)
    } else {
        parse_compact(trimmed)
    };
    let mut colors = Vec::new();
    let mut value = 0.0;
    let mut canonical = String::new();
    for atom in &atoms {
        canonical.push_str(&atom_canonical(atom));
        value += atom_value(atom);
        collect_colors(atom, &mut colors);
    }
    colors.sort_by_key(|c| color_rank(c));
    colors.dedup();
    ManaInfo {
        canonical,
        mana_value: value,
        colors,
    }
}

/// Color identity is the mana cost's colors plus mana symbols written in rules text.
pub fn color_identity(cost: &ManaInfo, rules: &str) -> Vec<String> {
    let mut colors = cost.colors.clone();
    let extra = parse_mana(&extract_symbols(rules));
    colors.extend(extra.colors);
    colors.sort_by_key(|c| color_rank(c));
    colors.dedup();
    colors
}

fn extract_symbols(text: &str) -> String {
    let mut out = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = text[i..].find('}') {
                out.push_str(&text[i..i + end + 1]);
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn color_rank(color: &str) -> usize {
    COLOR_ORDER
        .iter()
        .position(|c| c.to_string() == color)
        .unwrap_or(9)
}

fn atom_value(atom: &Atom) -> f64 {
    match atom {
        Atom::Generic(n) => *n as f64,
        Atom::Color(_) => 1.0,
        Atom::Special('H') => 0.5,
        Atom::Special(_) => 0.0,
        Atom::Hybrid(parts) => {
            let generic = parts.iter().filter_map(|p| match p {
                AtomPart::Generic(n) => Some(*n),
                _ => None,
            });
            let phyrexian = parts.iter().any(|p| matches!(p, AtomPart::Phyrexian));
            let colors = parts.iter().any(|p| matches!(p, AtomPart::Color(_)));
            if phyrexian && colors && parts.len() == 2 {
                return 1.0;
            }
            generic.max().unwrap_or(if colors { 1 } else { 0 }) as f64
        }
    }
}

fn collect_colors(atom: &Atom, colors: &mut Vec<String>) {
    match atom {
        Atom::Color(c) if is_color(*c) => colors.push(c.to_string()),
        Atom::Hybrid(parts) => {
            for part in parts {
                if let AtomPart::Color(c) = part {
                    if is_color(*c) {
                        colors.push(c.to_string());
                    }
                }
            }
        }
        _ => {}
    }
}

fn is_color(c: char) -> bool {
    matches!(c, 'W' | 'U' | 'B' | 'R' | 'G')
}

fn atom_canonical(atom: &Atom) -> String {
    match atom {
        Atom::Generic(n) => format!("{{{n}}}"),
        Atom::Color(c) | Atom::Special(c) => format!("{{{c}}}"),
        Atom::Hybrid(parts) => {
            let body = parts
                .iter()
                .map(|p| match p {
                    AtomPart::Generic(n) => n.to_string(),
                    AtomPart::Color(c) => c.to_string(),
                    AtomPart::Phyrexian => "P".to_string(),
                })
                .collect::<Vec<_>>()
                .join("/");
            format!("{{{body}}}")
        }
    }
}

fn parse_braced(input: &str) -> Vec<Atom> {
    let mut atoms = Vec::new();
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        atoms.push(parse_group(&after[..end]));
        rest = &after[end + 1..];
    }
    atoms
}

fn parse_group(body: &str) -> Atom {
    let body = body.trim();
    if body.contains('/') {
        let parts = body
            .split('/')
            .map(|part| {
                if part.eq_ignore_ascii_case("P") {
                    AtomPart::Phyrexian
                } else if let Ok(n) = part.parse::<u32>() {
                    AtomPart::Generic(n)
                } else {
                    AtomPart::Color(part.chars().next().unwrap_or('C').to_ascii_uppercase())
                }
            })
            .collect();
        return Atom::Hybrid(parts);
    }
    if let Ok(n) = body.parse::<u32>() {
        return Atom::Generic(n);
    }
    let c = body.chars().next().unwrap_or('C').to_ascii_uppercase();
    if is_color(c) {
        Atom::Color(c)
    } else {
        Atom::Special(c)
    }
}

fn parse_compact(input: &str) -> Vec<Atom> {
    let chars: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).collect();
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let n: u32 = chars[start..i].iter().collect::<String>().parse().unwrap_or(0);
            if i < chars.len() && chars[i] == '/' {
                i += 1;
                let right = read_symbol(&chars, &mut i);
                atoms.push(Atom::Hybrid(vec![AtomPart::Generic(n), right]));
            } else {
                atoms.push(Atom::Generic(n));
            }
            continue;
        }
        let left = read_symbol(&chars, &mut i);
        if i < chars.len() && chars[i] == '/' {
            i += 1;
            let right = read_symbol(&chars, &mut i);
            atoms.push(Atom::Hybrid(vec![part_from_symbol(left), right]));
        } else {
            atoms.push(atom_from_part(part_from_symbol(left)));
        }
    }
    atoms
}

fn read_symbol(chars: &[char], i: &mut usize) -> AtomPart {
    if *i >= chars.len() {
        return AtomPart::Color('C');
    }
    let c = chars[*i].to_ascii_uppercase();
    *i += 1;
    if c == 'P' {
        AtomPart::Phyrexian
    } else if is_color(c) {
        AtomPart::Color(c)
    } else {
        AtomPart::Color(c)
    }
}

fn part_from_symbol(part: AtomPart) -> AtomPart {
    part
}

fn atom_from_part(part: AtomPart) -> Atom {
    match part {
        AtomPart::Generic(n) => Atom::Generic(n),
        AtomPart::Phyrexian => Atom::Special('P'),
        AtomPart::Color(c) if is_color(c) => Atom::Color(c),
        AtomPart::Color(c) => Atom::Special(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braced_cost_derives_value_and_colors() {
        let info = parse_mana("{2}{U}{R}");
        assert_eq!(info.canonical, "{2}{U}{R}");
        assert_eq!(info.mana_value, 4.0);
        assert_eq!(info.colors, vec!["U".to_string(), "R".to_string()]);
    }

    #[test]
    fn compact_mse_cost_is_canonicalized() {
        let info = parse_mana("2U");
        assert_eq!(info.canonical, "{2}{U}");
        assert_eq!(info.mana_value, 3.0);
        assert_eq!(info.colors, vec!["U".to_string()]);
    }

    #[test]
    fn hybrid_phyrexian_and_twobrid_symbols() {
        let hybrid = parse_mana("{U/R}");
        assert_eq!(hybrid.mana_value, 1.0);
        assert_eq!(hybrid.colors, vec!["U".to_string(), "R".to_string()]);

        let phy = parse_mana("{G/P}");
        assert_eq!(phy.mana_value, 1.0);
        assert_eq!(phy.colors, vec!["G".to_string()]);

        let two = parse_mana("{2/W}");
        assert_eq!(two.mana_value, 2.0);
        assert_eq!(two.colors, vec!["W".to_string()]);
    }

    #[test]
    fn tap_and_colorless_do_not_add_color() {
        let info = parse_mana("{T}{C}");
        assert_eq!(info.mana_value, 0.0);
        assert!(info.colors.is_empty());
    }

    #[test]
    fn identity_includes_symbols_in_rules_text() {
        let cost = parse_mana("{U}");
        let identity = color_identity(&cost, "Create a Treasure. {T}: Add {R}.");
        assert_eq!(identity, vec!["U".to_string(), "R".to_string()]);
    }
}
