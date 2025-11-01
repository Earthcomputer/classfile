use crate::Label;
use java_string::JavaStr;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Frame<'class> {
    Full {
        locals: Vec<FrameValue<'class>>,
        stack: Vec<FrameValue<'class>>,
    },
    Append {
        locals: Vec<FrameValue<'class>>,
    },
    Chop {
        num_locals: u8,
    },
    Same,
    Same1 {
        stack_value: FrameValue<'class>,
    },
    // not in bytecode!
    New {
        locals: Vec<FrameValue<'class>>,
        stack: Vec<FrameValue<'class>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameValue<'class> {
    Top,
    Integer,
    Float,
    Long,
    Double,
    Null,
    UninitializedThis,
    Class(Cow<'class, JavaStr>),
    Uninitialized(Label),
}
