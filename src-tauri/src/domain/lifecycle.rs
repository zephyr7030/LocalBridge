use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Lost,
    Blocked,
}

impl TerminalOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Lost => "lost",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "state", content = "outcome", rename_all = "snake_case")]
pub enum LifecycleState {
    Queued,
    Running,
    Terminal(TerminalOutcome),
}

impl LifecycleState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_vocabulary_is_complete_and_shared() {
        let outcomes = [
            TerminalOutcome::Completed,
            TerminalOutcome::Failed,
            TerminalOutcome::Cancelled,
            TerminalOutcome::TimedOut,
            TerminalOutcome::Lost,
            TerminalOutcome::Blocked,
        ];
        assert_eq!(
            outcomes.map(TerminalOutcome::as_str),
            [
                "completed",
                "failed",
                "cancelled",
                "timed_out",
                "lost",
                "blocked",
            ]
        );
    }
}
