use crate::{TypePath, TypeReference};
use java_string::JavaStr;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct AnnotationNode<'class> {
    pub desc: Cow<'class, JavaStr>,
    pub values: Vec<(Cow<'class, JavaStr>, AnnotationValue<'class>)>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct TypeAnnotationNode<'class> {
    pub type_ref: TypeReference,
    pub type_path: TypePath<'class>,
    pub desc: Cow<'class, JavaStr>,
    pub values: Vec<(Cow<'class, JavaStr>, AnnotationValue<'class>)>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum AnnotationValue<'class> {
    Byte(i8),
    Char(u16),
    Double(f64),
    Float(f32),
    Int(i32),
    Long(i64),
    Short(i16),
    Boolean(bool),
    String(Cow<'class, JavaStr>),
    Enum {
        desc: Cow<'class, JavaStr>,
        name: Cow<'class, JavaStr>,
    },
    Class(Cow<'class, JavaStr>),
    Annotation(AnnotationNode<'class>),
    Array(Vec<AnnotationValue<'class>>),
}
