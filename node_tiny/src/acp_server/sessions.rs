use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use common::SessionContext;

use crate::praxis::{Agent, AgentSession};

pub struct NodeSession {
    pub session_id: Uuid,
    pub agent: Arc<dyn Agent>,
    pub session: Arc<dyn AgentSession>,
    pub context: SessionContext,
    pub cancel_flag: Arc<AtomicBool>,
}

impl NodeSession {
    pub fn short_name(&self) -> &str {
        self.agent.short_name()
    }
}

pub struct SessionStore {
    inner: RwLock<HashMap<Uuid, Arc<NodeSession>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn insert(&self, session: Arc<NodeSession>) {
        self.inner
            .write()
            .unwrap()
            .insert(session.session_id, session);
    }

    pub fn get(&self, session_id: &Uuid) -> Option<Arc<NodeSession>> {
        self.inner.read().unwrap().get(session_id).cloned()
    }

    pub fn remove(&self, session_id: &Uuid) -> Option<Arc<NodeSession>> {
        self.inner.write().unwrap().remove(session_id)
    }

    pub fn list(&self) -> Vec<Arc<NodeSession>> {
        self.inner.read().unwrap().values().cloned().collect()
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
