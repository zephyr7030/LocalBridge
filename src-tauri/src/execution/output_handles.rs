use std::collections::{HashMap, VecDeque};

use crate::domain::McpSessionId;
pub(crate) const MAX_LOCAL_RETAINED_OUTPUT_HANDLES: usize = 8;
pub(crate) const MAX_PRIVATE_RETAINED_OUTPUT_HANDLES: usize = 256;

#[derive(Debug, Clone)]
enum OutputHandle {
    Private {
        private_output_ref: String,
        owner_public_session_id: String,
        stream: String,
    },
    Local {
        owner_session: McpSessionId,
        stream: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OutputOwner {
    PublicSession(String),
    McpSession(McpSessionId),
}

#[derive(Debug, Default)]
pub(crate) struct OutputHandleRegistry {
    handles: HashMap<String, OutputHandle>,
    private_to_public: HashMap<String, String>,
    private_order: VecDeque<String>,
    local_order: VecDeque<String>,
}

impl OutputHandleRegistry {
    pub(crate) fn public_for_private(
        &mut self,
        private_output_ref: &str,
        owner_public_session_id: &str,
        stream: &str,
    ) -> String {
        if let Some(public) = self.private_to_public.get(private_output_ref) {
            return public.clone();
        }
        while self.private_order.len() >= MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            if let Some(expired) = self.private_order.pop_front() {
                self.remove(&expired);
            }
        }
        let public = next_output_handle();
        self.private_to_public
            .insert(private_output_ref.to_owned(), public.clone());
        self.handles.insert(
            public.clone(),
            OutputHandle::Private {
                private_output_ref: private_output_ref.to_owned(),
                owner_public_session_id: owner_public_session_id.to_owned(),
                stream: stream.to_owned(),
            },
        );
        self.private_order.push_back(public.clone());
        public
    }

    pub(crate) fn retain_local(
        &mut self,
        owner_session: McpSessionId,
        stream: &str,
        content: String,
    ) -> String {
        while self.local_order.len() >= MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
            if let Some(expired) = self.local_order.pop_front() {
                self.remove(&expired);
            }
        }
        let public = next_output_handle();
        self.handles.insert(
            public.clone(),
            OutputHandle::Local {
                owner_session,
                stream: stream.to_owned(),
                content,
            },
        );
        self.local_order.push_back(public.clone());
        public
    }

    pub(crate) fn private(&self, public_output_ref: &str) -> Option<String> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private {
                private_output_ref, ..
            } => Some(private_output_ref.clone()),
            OutputHandle::Local { .. } => None,
        }
    }

    pub(crate) fn local(&self, public_output_ref: &str) -> Option<(String, String)> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private { .. } => None,
            OutputHandle::Local {
                stream, content, ..
            } => Some((stream.clone(), content.clone())),
        }
    }

    pub(crate) fn stream(&self, public_output_ref: &str) -> Option<String> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private { stream, .. } | OutputHandle::Local { stream, .. } => {
                Some(stream.clone())
            }
        }
    }

    pub(crate) fn owner(&self, public_output_ref: &str) -> Option<OutputOwner> {
        match self.handles.get(public_output_ref)? {
            OutputHandle::Private {
                owner_public_session_id,
                ..
            } => Some(OutputOwner::PublicSession(owner_public_session_id.clone())),
            OutputHandle::Local { owner_session, .. } => {
                Some(OutputOwner::McpSession(owner_session.clone()))
            }
        }
    }

    pub(crate) fn reap_owned_by(&mut self, public_sessions: &[String]) {
        let expired = self
            .handles
            .iter()
            .filter(|(_, handle)| match handle {
                OutputHandle::Private {
                    owner_public_session_id,
                    ..
                } => public_sessions.contains(owner_public_session_id),
                OutputHandle::Local { .. } => false,
            })
            .map(|(public, _)| public.clone())
            .collect::<Vec<_>>();
        for public in &expired {
            self.remove(public);
        }
        self.private_order
            .retain(|public| !expired.contains(public));
    }

    fn remove(&mut self, public_output_ref: &str) {
        if let Some(OutputHandle::Private {
            private_output_ref, ..
        }) = self.handles.remove(public_output_ref)
        {
            self.private_to_public.remove(&private_output_ref);
        }
    }
}

fn next_output_handle() -> String {
    crate::security::random_prefixed_id("lb-output-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_private_handles_have_independent_bounded_fifo_retention() {
        let mut registry = OutputHandleRegistry::default();
        let private_first = registry.public_for_private("private-0", "session-a", "stdout");
        for index in 1..=MAX_PRIVATE_RETAINED_OUTPUT_HANDLES {
            registry.public_for_private(&format!("private-{index}"), "session-a", "stdout");
        }
        assert!(registry.private(&private_first).is_none());
        assert_eq!(
            registry.private_order.len(),
            MAX_PRIVATE_RETAINED_OUTPUT_HANDLES
        );

        let local_first =
            registry.retain_local(McpSessionId::new("owner"), "stdout", "first".into());
        for index in 1..=MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
            registry.retain_local(
                McpSessionId::new("owner"),
                "stderr",
                format!("local-{index}"),
            );
        }
        assert!(registry.local(&local_first).is_none());
        assert_eq!(
            registry.local_order.len(),
            MAX_LOCAL_RETAINED_OUTPUT_HANDLES
        );
    }

    #[test]
    fn private_handles_reap_with_their_public_session_owner() {
        let mut registry = OutputHandleRegistry::default();
        let a = registry.public_for_private("private-a", "session-a", "stdout");
        let b = registry.public_for_private("private-b", "session-b", "stderr");
        registry.reap_owned_by(&["session-a".into()]);
        assert!(registry.private(&a).is_none());
        assert_eq!(registry.private(&b).as_deref(), Some("private-b"));
    }
}
