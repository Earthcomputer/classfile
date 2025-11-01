use derive_more::TryFrom;
use std::borrow::Cow;
use std::fmt::{Debug, Formatter};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, TryFrom)]
#[repr(u8)]
#[try_from(repr)]
pub(crate) enum TypeReferenceTargetType {
    ClassTypeParameter = 0x00,
    MethodTypeParameter = 0x01,
    ClassExtends = 0x10,
    ClassTypeParameterBound = 0x11,
    MethodTypeParameterBound = 0x12,
    Field = 0x13,
    MethodReturn = 0x14,
    MethodReceiver = 0x15,
    MethodFormalParameter = 0x16,
    Throws = 0x17,
    LocalVariable = 0x40,
    ResourceVariable = 0x41,
    ExceptionParameter = 0x42,
    Instanceof = 0x43,
    New = 0x44,
    ConstructorReference = 0x45,
    MethodReference = 0x46,
    Cast = 0x47,
    ConstructorInvocationTypeArgument = 0x48,
    MethodInvocationTypeArgument = 0x49,
    ConstructorReferenceTypeArgument = 0x4A,
    MethodReferenceTypeArgument = 0x4B,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum TypeReference {
    ClassTypeParameter { param_index: u8 } = 0x00,
    MethodTypeParameter { param_index: u8 } = 0x01,
    ClassExtends { interface_index: Option<u16> } = 0x10,
    ClassTypeParameterBound { param_index: u8, bound_index: u8 } = 0x11,
    MethodTypeParameterBound { param_index: u8, bound_index: u8 } = 0x12,
    Field = 0x13,
    MethodReturn = 0x14,
    MethodReceiver = 0x15,
    MethodFormalParameter { param_index: u8 } = 0x16,
    Throws { exception_index: u16 } = 0x17,
    LocalVariable = 0x40,
    ResourceVariable = 0x41,
    ExceptionParameter = 0x42,
    Instanceof = 0x43,
    New = 0x44,
    ConstructorReference = 0x45,
    MethodReference = 0x46,
    Cast { arg_index: u8 } = 0x47,
    ConstructorInvocationTypeArgument { arg_index: u8 } = 0x48,
    MethodInvocationTypeArgument { arg_index: u8 } = 0x49,
    ConstructorReferenceTypeArgument { arg_index: u8 } = 0x4A,
    MethodReferenceTypeArgument { arg_index: u8 } = 0x4B,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypePath<'class> {
    path: Cow<'class, [u8]>,
}

impl<'class> TypePath<'class> {
    pub(crate) fn from_bytes(bytes: &'class [u8]) -> Self {
        TypePath { path: bytes.into() }
    }
}

impl Debug for TypePath<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: this is wrong
        Debug::fmt(&String::from_utf8_lossy(&self.path), f)
    }
}
