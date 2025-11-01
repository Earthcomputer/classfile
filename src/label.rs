use derive_more::Display;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
#[display("L{_0}")]
pub struct Label(u32);

#[derive(Debug, Clone, Default)]
pub struct LabelCreator {
    next_id: Arc<AtomicU32>,
}

impl LabelCreator {
    pub fn create_label(&self) -> Label {
        Label(self.next_id.fetch_add(1, Ordering::Relaxed))
    }
}
