//! Projects (PLAN P4): named folders that group sessions, with a colour and a place in the rail.
//! A session belongs to the project whose folder holds its working folder, unless chosen
//! otherwise. Chorus 1's projects can be imported once, read only (D33).

use serde::{Deserialize, Serialize};
use specta::Type;

pub const MAX_NAME_CHARS: usize = 60;

/// Colours offered for a new project, in the order they are handed out: the mockup's four tag
/// colours, then more in the same family.
pub const PALETTE: [&str; 10] =
    ["#f472b6", "#e0a96d", "#a5b4fc", "#c4e05a", "#3bcfae", "#5ea2e8", "#e2796b", "#c77be0", "#4fbfd1", "#8a97a6"];

/// Where a project is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectState {
    Active,
    /// Out of the rail, but its sessions still show.
    Hidden,
    /// Finished with: out of the rail and the Overview's chips.
    Archived,
}

impl ProjectState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Hidden => "hidden",
            Self::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Active, Self::Hidden, Self::Archived].into_iter().find(|v| v.as_str() == s)
    }
}

pub fn clean_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("A project needs a name.".into());
    }
    if n.chars().count() > MAX_NAME_CHARS || n.chars().any(char::is_control) {
        return Err(format!("A project's name is up to {MAX_NAME_CHARS} characters, without control characters."));
    }
    Ok(n.to_owned())
}

/// `#rrggbb`, lowercased.
pub fn clean_colour(colour: &str) -> Result<String, String> {
    let c = colour.trim().to_ascii_lowercase();
    if c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(c)
    } else {
        Err(format!("{colour:?} isn't a colour (#rrggbb)."))
    }
}

/// The next colour to hand out, given how many projects there are.
pub fn next_colour(existing: usize) -> &'static str {
    PALETTE[existing % PALETTE.len()]
}

fn norm(path: &str) -> String {
    path.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// The project a working folder belongs to: the one whose folder holds it most closely.
/// `projects` are `(id, folder)`; archived projects should be left out by the caller.
pub fn project_for_folder<'a>(projects: impl IntoIterator<Item = (u32, &'a str)>, cwd: &str) -> Option<u32> {
    let cwd = norm(cwd);
    projects
        .into_iter()
        .map(|(id, folder)| (id, norm(folder)))
        .filter(|(_, f)| !f.is_empty() && (cwd == *f || cwd.starts_with(&format!("{f}\\"))))
        .max_by_key(|(_, f)| f.len())
        .map(|(id, _)| id)
}

/// Chorus 1's `projects.status` as a 2.0 state (`active`, `hidden` or `archived`).
pub fn chorus1_state(status: &str) -> ProjectState {
    ProjectState::parse(status).unwrap_or(ProjectState::Active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_colours_are_checked() {
        assert_eq!(clean_name("  Ledger ").unwrap(), "Ledger");
        assert!(clean_name(" ").is_err() && clean_name(&"x".repeat(61)).is_err());
        assert_eq!(clean_colour("#F472B6").unwrap(), "#f472b6");
        for bad in ["f472b6", "#f472b", "#gggggg", "red"] {
            assert!(clean_colour(bad).is_err(), "{bad}");
        }
        assert_eq!(next_colour(0), "#f472b6");
        assert_eq!(next_colour(PALETTE.len() + 1), PALETTE[1]);
    }

    #[test]
    fn a_folder_belongs_to_the_closest_project() {
        let projects = [(1, r"C:\work"), (2, r"C:\work\ledger"), (3, r"D:\other"), (4, "")];
        assert_eq!(project_for_folder(projects, r"c:\Work\Ledger\web"), Some(2));
        assert_eq!(project_for_folder(projects, r"C:\work\auth"), Some(1));
        assert_eq!(project_for_folder(projects, r"C:\work"), Some(1));
        assert_eq!(project_for_folder(projects, r"C:\workshop"), None, "a longer sibling name isn't inside");
        assert_eq!(project_for_folder(projects, r"E:\x"), None);
    }

    #[test]
    fn chorus_1_states_map_across() {
        assert_eq!(chorus1_state("hidden"), ProjectState::Hidden);
        assert_eq!(chorus1_state("archived"), ProjectState::Archived);
        assert_eq!(chorus1_state("odd"), ProjectState::Active);
    }
}
