#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McpSessionState {
    Created,
    Active,
    Closing,
    Closed,
}

impl McpSessionState {
    pub const fn accepts_requests(self) -> bool {
        matches!(self, Self::Created | Self::Active)
    }
}
