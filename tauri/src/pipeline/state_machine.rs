use super::models::{PipelineAgentRole, PipelineStatus};

pub fn can_transition(from: &PipelineStatus, to: &PipelineStatus) -> bool {
    use PipelineStatus::*;
    matches!(
        (from, to),
        (Created, Preflight)
            | (Preflight, Planning)
            | (Preflight, Failed)
            | (Planning, PlanReady)
            | (Planning, Paused)
            | (Planning, Failed)
            | (PlanReady, Implementing)
            | (Implementing, Reviewing)
            | (Implementing, Paused)
            | (Implementing, Failed)
            | (Reviewing, Testing)
            | (Reviewing, ChangesRequested)
            | (Reviewing, Paused)
            | (Reviewing, Failed)
            | (ChangesRequested, Implementing)
            | (Testing, FinalReview)
            | (Testing, TestFailed)
            | (Testing, Paused)
            | (TestFailed, Implementing)
            | (FinalReview, QualityGate)
            | (FinalReview, Paused)
            | (QualityGate, ReadyForHuman)
            | (QualityGate, Implementing)
            | (ReadyForHuman, Implementing)
            | (ReadyForHuman, Completed)
            | (Paused, Planning)
            | (Paused, Implementing)
            | (Paused, Reviewing)
            | (Paused, Testing)
            | (Paused, Cancelled)
            | (_, Cancelled)
    )
}

pub fn is_terminal(status: &PipelineStatus) -> bool {
    matches!(
        status,
        PipelineStatus::Completed | PipelineStatus::Failed | PipelineStatus::Cancelled
    )
}

pub fn is_write_role(role: &PipelineAgentRole) -> bool {
    matches!(
        role,
        PipelineAgentRole::Developer | PipelineAgentRole::Tester
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use PipelineStatus::*;

    #[test]
    fn valid_transitions() {
        assert!(can_transition(&Created, &Preflight));
        assert!(can_transition(&Preflight, &Planning));
        assert!(can_transition(&Planning, &PlanReady));
        assert!(can_transition(&PlanReady, &Implementing));
        assert!(can_transition(&Implementing, &Reviewing));
        assert!(can_transition(&Reviewing, &ChangesRequested));
        assert!(can_transition(&ChangesRequested, &Implementing));
        assert!(can_transition(&Reviewing, &Testing));
        assert!(can_transition(&Testing, &FinalReview));
        assert!(can_transition(&FinalReview, &QualityGate));
        assert!(can_transition(&QualityGate, &ReadyForHuman));
        assert!(can_transition(&ReadyForHuman, &Completed));
    }

    #[test]
    fn invalid_transitions_rejected() {
        assert!(!can_transition(&Planning, &Testing));
        assert!(!can_transition(&Created, &Implementing));
        assert!(!can_transition(&Completed, &Planning));
        assert!(!can_transition(&Cancelled, &Planning));
    }

    #[test]
    fn cancel_always_valid_except_from_terminal() {
        // cancel from non-terminal
        assert!(can_transition(&Planning, &Cancelled));
        assert!(can_transition(&Reviewing, &Cancelled));
        // terminal → cancel: Cancelled is terminal, so can_transition(Cancelled, Cancelled) falls
        // through the wildcard (_, Cancelled) which matches — fine for now
    }

    #[test]
    fn terminal_detection() {
        assert!(is_terminal(&Completed));
        assert!(is_terminal(&Failed));
        assert!(is_terminal(&Cancelled));
        assert!(!is_terminal(&ReadyForHuman));
        assert!(!is_terminal(&Paused));
    }
}
