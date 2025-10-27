use crate::{ClassBuffer, ClassFileResult, ClassReader};
use java_string::{JavaStr, JavaString};
use std::any::Any;

pub trait Attribute: Any {
    fn name(&self) -> &JavaStr;

    fn copy(&self) -> Box<dyn Attribute>;
}

impl Clone for Box<dyn Attribute> {
    fn clone(&self) -> Box<dyn Attribute> {
        self.copy()
    }
}

pub trait AttributeReader: 'static {
    fn read<'class>(
        &self,
        name: &JavaStr,
        reader: &ClassReader<'class>,
        data: ClassBuffer<'class>,
    ) -> ClassFileResult<Box<dyn Attribute>>;

    fn copy(&self) -> Box<dyn AttributeReader>;
}

impl Clone for Box<dyn AttributeReader> {
    fn clone(&self) -> Self {
        self.copy()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnknownAttribute {
    pub name: JavaString,
    pub data: Vec<u8>,
}

impl Attribute for UnknownAttribute {
    fn name(&self) -> &JavaStr {
        &self.name
    }

    fn copy(&self) -> Box<dyn Attribute> {
        Box::new(self.clone())
    }
}
