use crate::session::{SessionHandle, SessionId};
use dashmap::DashMap;

pub struct SessionManager {
    pub sessions: DashMap<SessionId, SessionHandle>, // Example key-value pair, adjust as needed
}
