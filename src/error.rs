use crate::{ConstantPoolTag, Opcode};
use java_string::Utf8Error;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum ClassFileError {
    #[error("bad annotation tag: {0}")]
    BadAnnotationTag(u8),
    #[error("bad code size: {0}, must be between 1-65535 inclusive")]
    BadCodeSize(u32),
    #[error("bad constant pool index: {index}, len {len}")]
    BadConstantPoolIndex { index: u16, len: usize },
    #[error("no entry at constant pool index: {0}")]
    BadConstantPoolIndexNoEntry(u16),
    #[error("bad constant pool tag: {0}")]
    BadConstantPoolTag(u8),
    #[error("bad constant pool tag: {actual}, expected {expected}")]
    BadConstantPoolType {
        expected: ConstantPoolTag,
        actual: ConstantPoolTag,
    },
    #[error("bad constant pool tag: {0}, expected bootstrap method argument")]
    BadConstantPoolTypeExpectedBootstrapMethodArgument(ConstantPoolTag),
    #[error("bad constant pool tag: {0}, expected field constant value")]
    BadConstantPoolTypeExpectedFieldConstantValue(ConstantPoolTag),
    #[error("bad constant pool tag: {0}, expected ldc operand")]
    BadConstantPoolTypeExpectedLdcOperand(ConstantPoolTag),
    #[error("bad frame type: {0}")]
    BadFrameType(u8),
    #[error("bad frame value tag: {0}")]
    BadFrameValueTag(u8),
    #[error("bad handle kind: {0}")]
    BadHandleKind(u8),
    #[error("bad magic number")]
    BadMagic,
    #[error("bad newarray type: {0}")]
    BadNewArrayType(u8),
    #[error("bad opcode: {0}")]
    BadOpcode(u8),
    #[error("bad type annotation target: {0}")]
    BadTypeAnnotationTarget(u8),
    #[error("bad wide opcode: {0}")]
    BadWideOpcode(Opcode),
    #[error("circular dependency in bootstrap methods")]
    BootstrapMethodCircularDependency,
    #[error("bootstrap method out of bounds, index {index}, len {len}")]
    BootstrapMethodOutOfBounds { index: u16, len: u16 },
    #[error("code offset out of bounds, index {index}, len {len}")]
    CodeOffsetOutOfBounds { index: usize, len: usize },
    #[error("read past the end of the class file, index {index}, len {len}")]
    OutOfBounds { index: usize, len: usize },
    #[error("tableswitch bounds in wrong order, low: {low}, high: {high}, expected low <= high")]
    TableSwitchBoundsWrongOrder { low: i32, high: i32 },
    #[error("too deep annotation nesting")]
    TooDeepAnnotationNesting,
    #[error("unsupported class file version: {0}")]
    UnsupportedVersion(u16),
    #[error("utf8 error: {0}")]
    Utf8(#[from] Utf8Error),
}

pub type ClassFileResult<T> = Result<T, ClassFileError>;
