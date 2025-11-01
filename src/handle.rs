use crate::{ClassFileError, ClassFileResult};
use derive_more::{Display, TryFrom};
use java_string::JavaStr;
use std::borrow::Cow;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, TryFrom)]
#[repr(u8)]
#[non_exhaustive]
#[try_from(repr)]
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
        Self::try_from(tag).map_err(|_| ClassFileError::BadHandleKind(tag))
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

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct ConstantDynamic<'class> {
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub bootstrap_method: Handle<'class>,
    pub bootstrap_method_arguments: Vec<BootstrapMethodArgument<'class>>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum BootstrapMethodArgument<'class> {
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(Cow<'class, JavaStr>),
    Class(Cow<'class, JavaStr>),
    Handle(Handle<'class>),
    ConstantDynamic(ConstantDynamic<'class>),
}
