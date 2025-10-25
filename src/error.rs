use crate::ConstantPoolTag;
use java_string::Utf8Error;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClassFileError {
    #[error("bad annotation tag: {0}")]
    BadAnnotationTag(u8),
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
    #[error("bad handle kind: {0}")]
    BadHandleKind(u8),
    #[error("bad magic number")]
    BadMagic,
    #[error("bad type annotation target: {0}")]
    BadTypeAnnotationTarget(u8),
    #[error("read past the end of the class file, index {index}, len {len}")]
    OutOfBounds { index: usize, len: usize },
    #[error("too deep annotation nesting")]
    TooDeepAnnotationNesting,
    #[error("unsupported class file version: {0}")]
    UnsupportedVersion(u16),
    #[error("utf8 error: {0}")]
    Utf8(#[from] Utf8Error),
}

pub type ClassFileResult<T> = Result<T, ClassFileError>;
