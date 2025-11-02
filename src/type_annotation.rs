use crate::ClassFileResult;
use derive_more::{Display, IsVariant, TryFrom};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Write};
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::num::ParseIntError;
use std::ops::{Index, IndexMut};
use std::str::FromStr;
use thiserror::Error;

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

#[derive(Clone, Eq, PartialOrd, Default)]
pub struct TypePath<'class> {
    // Invariant: path len must always be a multiple of 2
    path: Cow<'class, [u8]>,
}

impl<'class> TypePath<'class> {
    pub(crate) fn from_bytes(bytes: &'class [u8]) -> Self {
        #[inline(never)]
        #[cold]
        fn invalid_length(len: usize) -> ! {
            panic!("input has invalid length {len}, must be a multiple of 2");
        }

        if !bytes.len().is_multiple_of(2) {
            invalid_length(bytes.len());
        }

        TypePath { path: bytes.into() }
    }

    pub fn len(&self) -> usize {
        self.path.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.path.len() == 0
    }

    pub fn try_get(&self, index: usize) -> Result<Option<TypePathElement>, TypePathError> {
        match self.path.get(index * 2) {
            Some(0) => Ok(Some(TypePathElement::ArrayElement)),
            Some(1) => Ok(Some(TypePathElement::InnerType)),
            Some(2) => Ok(Some(TypePathElement::WildcardBound)),
            Some(3) => {
                // SAFETY: by the invariant, self.path.len() is a multiple of 2. If
                // self.path.get(index * 2) returned Some(3), then index * 2 is a valid index,
                // therefore index * 2 + 1 must also be a valid index.
                let argument_index = unsafe { *self.path.get_unchecked(index * 2 + 1) };
                Ok(Some(TypePathElement::TypeArgument(argument_index)))
            }
            Some(&kind) => Err(TypePathError { invalid_kind: kind }),
            None => Ok(None),
        }
    }

    pub fn get(&self, index: usize) -> Result<TypePathElement, TypePathError> {
        self.try_get(index)
            .map(|value| value.unwrap_or_else(|| self.out_of_bounds(index)))
    }

    pub fn set(&mut self, index: usize, value: TypePathElement) {
        if index * 2 >= self.path.len() {
            self.out_of_bounds(index);
        }

        let (kind, meta_value) = match value {
            TypePathElement::ArrayElement => (0, 0),
            TypePathElement::InnerType => (1, 0),
            TypePathElement::WildcardBound => (2, 0),
            TypePathElement::TypeArgument(argument_index) => (3, argument_index),
        };

        let path = self.path.to_mut();
        path[index * 2] = kind;
        // SAFETY: by the invariant, self.path.len() is a multiple of 2. index * 2 is a valid index,
        // therefore index * 2 + 1 is a valid index
        unsafe {
            *path.get_unchecked_mut(index * 2 + 1) = meta_value;
        }
    }

    pub fn push(&mut self, value: TypePathElement) {
        let len = self.len();
        self.path.to_mut().extend([0, 0]);
        self.set(len, value);
    }

    #[inline(never)]
    #[cold]
    fn out_of_bounds(&self, index: usize) -> ! {
        panic!(
            "type path index out of bounds, index {index}, len {}",
            self.len()
        );
    }
}

impl PartialEq for TypePath<'_> {
    fn eq(&self, other: &Self) -> bool {
        if self.path.len() != other.path.len() {
            return false;
        }

        for i in (0..self.path.len()).step_by(2) {
            if self.path[i] != other.path[i] {
                return false;
            }

            // type argument
            if self.path[i] == 3 {
                // SAFETY: since both self.path.len() and i are multiples of 2, and i is a valid
                // index, i + 1 must also be a valid index
                let argument_index_matches =
                    unsafe { self.path.get_unchecked(i + 1) == other.path.get_unchecked(i + 1) };
                if !argument_index_matches {
                    return false;
                }
            }
        }

        true
    }
}

impl Ord for TypePath<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        for i in (0..usize::min(self.path.len(), other.path.len())).step_by(2) {
            match self.path[i].cmp(&other.path[i]) {
                Ordering::Less => return Ordering::Less,
                Ordering::Greater => return Ordering::Greater,
                Ordering::Equal => {}
            }

            // type argument
            if self.path[i] == 3 {
                let argument_index_cmp = unsafe {
                    self.path
                        .get_unchecked(i + 1)
                        .cmp(other.path.get_unchecked(i + 1))
                };
                match argument_index_cmp {
                    Ordering::Less => return Ordering::Less,
                    Ordering::Greater => return Ordering::Greater,
                    Ordering::Equal => {}
                }
            }
        }

        self.path.len().cmp(&other.path.len())
    }
}

impl Hash for TypePath<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // TODO: replace with write_length_prefix once it's stabilized
        state.write_usize(self.path.len());

        for i in (0..self.path.len()).step_by(2) {
            state.write_u8(self.path[i]);
            // type argument
            if self.path[i] == 3 {
                // SAFETY: since both self.path.len() and i are multiples of 2, and i is a valid
                // index, i + 1 must also be a valid index
                let type_argument = unsafe { *self.path.get_unchecked(i + 1) };
                state.write_u8(type_argument);
            }
        }
    }
}

impl Debug for TypePath<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self).finish()
    }
}

impl Display for TypePath<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for element in self {
            match element {
                Ok(element) => Display::fmt(&element, f)?,
                Err(_) => f.write_char('?')?,
            }
        }

        Ok(())
    }
}

impl FromStr for TypePath<'_> {
    type Err = ParseTypePathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut path = TypePath::default();

        let mut number_start = None;
        for (i, ch) in s.char_indices() {
            match ch {
                '[' => {
                    if number_start.is_some() {
                        return Err(ParseTypePathError {
                            index: i,
                            kind: ParseTypePathErrorKind::ExpectedNumberTerminator,
                        });
                    }

                    path.push(TypePathElement::ArrayElement);
                }
                '.' => {
                    if number_start.is_some() {
                        return Err(ParseTypePathError {
                            index: i,
                            kind: ParseTypePathErrorKind::ExpectedNumberTerminator,
                        });
                    }

                    path.push(TypePathElement::InnerType);
                }
                '*' => {
                    if number_start.is_some() {
                        return Err(ParseTypePathError {
                            index: i,
                            kind: ParseTypePathErrorKind::ExpectedNumberTerminator,
                        });
                    }

                    path.push(TypePathElement::WildcardBound)
                }
                '0'..='9' => {
                    number_start.get_or_insert(i);
                }
                ';' => {
                    let Some(number_start) = number_start.take() else {
                        return Err(ParseTypePathError {
                            index: i,
                            kind: ParseTypePathErrorKind::UnexpectedChar(';'),
                        });
                    };
                    let argument_index = match s[number_start..i].parse() {
                        Ok(index) => index,
                        Err(err) => {
                            return Err(ParseTypePathError {
                                index: number_start,
                                kind: ParseTypePathErrorKind::IntParseError(err),
                            })
                        }
                    };
                    path.push(TypePathElement::TypeArgument(argument_index));
                }
                _ => {
                    return Err(ParseTypePathError {
                        index: i,
                        kind: ParseTypePathErrorKind::UnexpectedChar(ch),
                    })
                }
            };
        }

        if number_start.is_some() {
            return Err(ParseTypePathError {
                index: s.len(),
                kind: ParseTypePathErrorKind::ExpectedNumberTerminator,
            });
        }

        Ok(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display, Error)]
#[display("at {index}, {kind}")]
pub struct ParseTypePathError {
    pub index: usize,
    pub kind: ParseTypePathErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum ParseTypePathErrorKind {
    #[display("unexpected char '{_0}'")]
    UnexpectedChar(char),
    IntParseError(ParseIntError),
    #[display("expected ';' to terminate number")]
    ExpectedNumberTerminator,
}

impl<'path, 'class> IntoIterator for &'path TypePath<'class> {
    type Item = Result<TypePathElement, TypePathError>;
    type IntoIter = TypePathIterator<'path, 'class>;

    fn into_iter(self) -> Self::IntoIter {
        TypePathIterator {
            path: self,
            index: 0,
        }
    }
}

#[derive(Debug)]
pub struct TypePathIterator<'path, 'class> {
    path: &'path TypePath<'class>,
    index: usize,
}

impl Iterator for TypePathIterator<'_, '_> {
    type Item = Result<TypePathElement, TypePathError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.path.len() {
            return None;
        }

        let index = self.index;
        self.index += 1;
        Some(self.path.get(index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.path.len(), Some(self.path.len()))
    }
}

impl FusedIterator for TypePathIterator<'_, '_> {}

impl ExactSizeIterator for TypePathIterator<'_, '_> {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, IsVariant)]
pub enum TypePathElement {
    #[display("[")]
    ArrayElement,
    #[display(".")]
    InnerType,
    #[display("*")]
    WildcardBound,
    #[display("{_0};")]
    TypeArgument(u8),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Error)]
#[display("invalid type path kind: {invalid_kind}")]
pub struct TypePathError {
    pub invalid_kind: u8,
}
