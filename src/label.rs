use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(u32);

pub trait LabelCreator {
    fn create_label(&self) -> Label;
}

#[derive(Debug, Default)]
pub struct DefaultLabelCreator {
    next_id: AtomicU32,
}

impl LabelCreator for DefaultLabelCreator {
    fn create_label(&self) -> Label {
        Label(self.next_id.fetch_add(1, Ordering::Relaxed))
    }
}
