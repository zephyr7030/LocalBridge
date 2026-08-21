use super::policy::shell_invocation_requires_review;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPolicyDecision {
    Allow,
    Review,
}

pub struct ShellExecutionPolicy;

impl ShellExecutionPolicy {
    pub fn evaluate(shell: &str, command: &str) -> ShellPolicyDecision {
        if shell_invocation_requires_review(shell, command) {
            ShellPolicyDecision::Review
        } else {
            ShellPolicyDecision::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_policy_has_no_filesystem_authorization_result() {
        assert_eq!(
            ShellExecutionPolicy::evaluate("cmd", "echo hello"),
            ShellPolicyDecision::Allow
        );
        assert_eq!(
            ShellExecutionPolicy::evaluate("cmd", "sc.exe query"),
            ShellPolicyDecision::Review
        );
    }
}
