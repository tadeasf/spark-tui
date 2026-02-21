use super::super::types::Suspect;

/// Aggregate all suspects and sort by severity (Critical first), then by estimated savings descending.
pub fn aggregate_suspects(mut suspects: Vec<Suspect>) -> Vec<Suspect> {
    suspects.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| b.estimated_savings_ms.cmp(&a.estimated_savings_ms))
    });
    suspects
}
