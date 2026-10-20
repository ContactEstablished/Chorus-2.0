//! The names New session suggests (ported from Chorus 1's `agentNames.ts`): short, plain first names,
//! so "Claude Code · Bob" and "Claude Code · Ruth" are told apart at a glance. A suggestion only;
//! the field is the user's to change or clear.

pub const NAMES: [&str; 40] = [
    "Bob", "Mae", "Frank", "Ruth", "Hank", "June", "Walt", "Pearl", "Gus", "Nell", "Ray", "Hazel", "Earl", "Faye", "Vince",
    "Lois", "Otis", "Vera", "Dale", "Rita", "Roy", "Wanda", "Chuck", "Marge", "Stan", "Gail", "Marty", "Joan", "Gene", "Sue",
    "Duke", "Edna", "Wade", "Ida", "Cal", "Dot", "Rex", "Peg", "Buck", "Bea",
];

/// A name not in `taken` (compared trimmed and case-insensitively), picked by `roll`. Once every
/// name is taken, any name: a repeat the user can type over beats a machine-made "Bob 2".
pub fn suggest<S: AsRef<str>>(taken: &[S], roll: u32) -> &'static str {
    let used: Vec<String> = taken.iter().map(|n| n.as_ref().trim().to_lowercase()).collect();
    let free: Vec<&'static str> = NAMES.iter().copied().filter(|n| !used.contains(&n.to_lowercase())).collect();
    let pool: &[&'static str] = if free.is_empty() { &NAMES } else { &free };
    pool[roll as usize % pool.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suggestion_skips_names_in_use() {
        let taken: Vec<&str> = NAMES.iter().copied().filter(|n| *n != "Ida").collect();
        assert_eq!(suggest(&taken, 7), "Ida");
        assert_eq!(suggest(&[" bob "], 0), "Mae", "taken names are compared loosely");
        assert!(NAMES.contains(&suggest(&NAMES, 3)), "a full house still gets a name");
    }
}
