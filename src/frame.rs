use crate::Label;
use java_string::JavaStr;
use std::borrow::Cow;
use strum::{Display, FromRepr};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromRepr)]
#[repr(u8)]
pub enum FrameType {
    Full = 0,
    Append = 1,
    Chop = 2,
    Same = 3,
    Same1 = 4,
    New = 5, // not in bytecode!
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
