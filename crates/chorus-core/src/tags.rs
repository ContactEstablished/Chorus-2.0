//! Session tags (D52): short free-form labels beside the project, several per session, created by
//! typing and offered again from the ones used before. Kept lowercase, so "Bugfix" and "bugfix"
//! are one tag.

pub const MAX_TAG_CHARS: usize = 24;
pub const MAX_TAGS: usize = 8;

/// One tag as typed: trimmed, a leading `#` dropped, inner spaces collapsed, lowercased.
pub fn clean_tag(tag: &str) -> Result<String, String> {
    let t = tag.trim().trim_start_matches('#').split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if t.is_empty() {
        return Err("A tag can't be empty.".into());
    }
    if t.chars().count() > MAX_TAG_CHARS || t.chars().any(|c| c.is_control() || c == ',') {
        return Err(format!("A tag is up to {MAX_TAG_CHARS} characters, without commas."));
    }
    Ok(t)
}

/// A session's tags: each cleaned, repeats dropped, in the order given.
pub fn clean_tags<S: AsRef<str>>(tags: &[S]) -> Result<Vec<String>, String> {
    // Repeats are dropped below, but a list this long is no session's.
    if tags.len() > MAX_TAGS * 4 {
        return Err(format!("A session takes up to {MAX_TAGS} tags."));
    }
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = clean_tag(t.as_ref())?;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(format!("A session takes up to {MAX_TAGS} tags."));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_cleaned_and_deduplicated() {
        assert_eq!(clean_tag("  #Bug   Fix ").unwrap(), "bug fix");
        assert!(clean_tag(" # ").is_err());
        assert!(clean_tag("a,b").is_err());
        assert!(clean_tag(&"x".repeat(MAX_TAG_CHARS + 1)).is_err());
        assert_eq!(clean_tags(&["Spike", "spike", "review"]).unwrap(), ["spike", "review"]);
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert!(clean_tags(&many).is_err());
    }
}
