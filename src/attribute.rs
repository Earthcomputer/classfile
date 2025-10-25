use crate::{ClassBuffer, ClassFileResult};
use java_string::JavaStr;
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
        data: ClassBuffer<'class>,
    ) -> ClassFileResult<Box<dyn Attribute>>;

    fn copy(&self) -> Box<dyn AttributeReader>;
}

impl Clone for Box<dyn AttributeReader> {
    fn clone(&self) -> Self {
        self.copy()
    }
}
