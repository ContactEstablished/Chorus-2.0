//! Which build is running, and whether its app identifier matches (LESSONS L15, D14).
//!
//! Dev and release builds use different identifiers, so they never share a data directory. A
//! debug binary carrying the release identifier would write into the installed app's data, so the
//! mismatch is an error, not a warning.

use serde::Serialize;

/// The release identifier. The dev build appends [`DEV_SUFFIX`].
pub const RELEASE_IDENTIFIER: &str = "com.contactestablished.chorus2";
pub const DEV_SUFFIX: &str = ".dev";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildFlavor {
    Dev,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlavorError {
    /// The identifier is neither the release identifier nor its dev variant.
    UnknownIdentifier(String),
    /// A debug binary with the release identifier, or a release binary with the dev one.
    Mismatch { identifier: String, debug_build: bool },
}

impl std::fmt::Display for FlavorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownIdentifier(id) => write!(f, "unknown app identifier {id:?}"),
            Self::Mismatch { identifier, debug_build: true } => write!(
                f,
                "this debug build has identifier {identifier:?}, which belongs to release builds; \
                 run it with `npm run dev` so it uses the dev identifier and its own data"
            ),
            Self::Mismatch { identifier, debug_build: false } => write!(
                f,
                "this release build has the dev identifier {identifier:?}; build it with `npm run build`"
            ),
        }
    }
}

impl std::error::Error for FlavorError {}

/// Classifies `identifier` and checks it against how the binary was compiled.
pub fn flavor_for(identifier: &str, debug_build: bool) -> Result<BuildFlavor, FlavorError> {
    let flavor = match identifier.strip_suffix(DEV_SUFFIX) {
        Some(RELEASE_IDENTIFIER) => BuildFlavor::Dev,
        Some(_) => return Err(FlavorError::UnknownIdentifier(identifier.to_owned())),
        None if identifier == RELEASE_IDENTIFIER => BuildFlavor::Release,
        None => return Err(FlavorError::UnknownIdentifier(identifier.to_owned())),
    };
    let expected = if debug_build { BuildFlavor::Dev } else { BuildFlavor::Release };
    if flavor == expected {
        Ok(flavor)
    } else {
        Err(FlavorError::Mismatch { identifier: identifier.to_owned(), debug_build })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_identifier_in_a_debug_build_is_dev() {
        assert_eq!(flavor_for("com.contactestablished.chorus2.dev", true), Ok(BuildFlavor::Dev));
    }

    #[test]
    fn release_identifier_in_a_release_build_is_release() {
        assert_eq!(flavor_for("com.contactestablished.chorus2", false), Ok(BuildFlavor::Release));
    }

    #[test]
    fn debug_build_with_the_release_identifier_is_refused() {
        let err = flavor_for(RELEASE_IDENTIFIER, true).unwrap_err();
        assert!(matches!(err, FlavorError::Mismatch { debug_build: true, .. }));
        assert!(err.to_string().contains("npm run dev"));
    }

    #[test]
    fn release_build_with_the_dev_identifier_is_refused() {
        let err = flavor_for("com.contactestablished.chorus2.dev", false).unwrap_err();
        assert!(matches!(err, FlavorError::Mismatch { debug_build: false, .. }));
    }

    #[test]
    fn chorus_1_identifier_is_unknown() {
        // Chorus 1 is com.contactestablished.chorus; 2.0 must never adopt it.
        assert!(matches!(
            flavor_for("com.contactestablished.chorus", false),
            Err(FlavorError::UnknownIdentifier(_))
        ));
        assert!(matches!(
            flavor_for("com.contactestablished.chorus.dev", true),
            Err(FlavorError::UnknownIdentifier(_))
        ));
    }
}
