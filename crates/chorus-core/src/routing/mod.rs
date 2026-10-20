//! Routing tiers for opencode on OpenRouter (PLAN P12): Budget, Balanced and Fast pin a model's
//! best endpoints by measured speed, price and reliability; Nitro lets OpenRouter pick the
//! fastest. Ported from Chorus 1's pure routing cores (`src/main/routing/*Core.ts`) with their
//! tests and the 2026-10-02 golden fixture; constants and user-facing texts are unchanged.
//!
//! The app fetches, stores and launches; everything here is computed from its inputs. Times are
//! Unix milliseconds passed in (`now_ms`), never read from a clock, and nonces are parameters.
//!
//! The flow: [`endpoints::parse_endpoints_response`] → [`types::EndpointSnapshot`] and
//! [`endpoints::extract_observations`] (stored with [`store::merge_observations`]) →
//! [`registry::entry_for`] → [`tiers::compute_tiers`] → [`launch::resolve_launch_selection`] →
//! [`payload`] values for opencode's configuration. [`preflight`] reads the account's own
//! filters; [`cache_probe`] checks which endpoints really serve cached reads.

pub mod cache_probe;
pub mod eligibility;
pub mod endpoints;
mod js;
mod json;
pub mod launch;
pub mod payload;
pub mod preflight;
pub mod pricing;
pub mod ranker;
pub mod registry;
pub mod store;
pub mod tiers;
pub mod time;
pub mod types;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests {
    use specta::Type;

    /// specta's TypeScript exporter refuses `i64`, `u64` and the other integers JavaScript can't
    /// hold exactly, so every type the front end receives must be free of them (the `*_ms`
    /// fields export as `f64`).
    #[test]
    fn front_end_types_export_without_big_integers() {
        let mut types = specta::Types::default();
        let roots = [
            super::types::TierResult::definition(&mut types),
            super::types::RoutingSettings::definition(&mut types),
            super::types::ModelRegistryEntry::definition(&mut types),
            super::registry::RegistrySource::definition(&mut types),
            super::launch::RoutingLaunchSelection::definition(&mut types),
            super::launch::RoutingLaunchChoice::definition(&mut types),
            super::launch::LaunchRefusal::definition(&mut types),
            super::cache_probe::RoutingRefreshReport::definition(&mut types),
            super::cache_probe::ProbePlan::definition(&mut types),
            super::cache_probe::CacheProbeOutcome::definition(&mut types),
            super::preflight::PreflightIssue::definition(&mut types),
            super::preflight::PreflightStep::definition(&mut types),
        ];
        let text = format!("{types:?} {roots:?}");
        assert!(text.contains("Primitive(f64)") && text.contains("Primitive(u32)"), "the check sees the primitives");
        assert!(format!("{:?}", i64::definition(&mut specta::Types::default())).contains("Primitive(i64)"), "and would see an i64");
        for big in ["i64", "u64", "i128", "u128", "isize", "usize"] {
            assert!(!text.contains(&format!("Primitive({big})")), "a front-end type holds {big}");
        }
    }
}
