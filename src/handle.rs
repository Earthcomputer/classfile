use crate::{ClassFileError, ClassFileResult};
use java_string::JavaStr;
use std::borrow::Cow;
use strum::{Display, FromRepr};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromRepr)]
#[repr(u8)]
#[non_exhaustive]
pub enum HandleKind {
    GetField = 1,
    GetStatic = 2,
    PutField = 3,
    PutStatic = 4,
    InvokeVirtual = 5,
    InvokeStatic = 6,
    InvokeSpecial = 7,
    NewInvokeSpecial = 8,
    InvokeInterface = 9,
}

impl HandleKind {
    pub fn from_u8(tag: u8) -> ClassFileResult<HandleKind> {
        Self::from_repr(tag).ok_or(ClassFileError::BadHandleKind(tag))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)] // TODO: Display
pub struct Handle<'class> {
    pub kind: HandleKind,
    pub owner: Cow<'class, JavaStr>,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub is_interface: bool,
}
