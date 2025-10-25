use java_string::JavaStr;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum FieldValue<'class> {
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(Cow<'class, JavaStr>),
}
