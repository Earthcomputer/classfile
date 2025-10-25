use crate::{ClassBuffer, ClassFileError, ClassFileResult, Handle, HandleKind};
use java_string::JavaStr;
use std::borrow::Cow;
use strum::{Display, FromRepr};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromRepr)]
#[repr(u8)]
#[non_exhaustive]
pub enum ConstantPoolTag {
    Utf8 = 1,
    Integer = 3,
    Float = 4,
    Long = 5,
    Double = 6,
    Class = 7,
    String = 8,
    FieldRef = 9,
    MethodRef = 10,
    InterfaceMethodRef = 11,
    NameAndType = 12,
    MethodHandle = 15,
    MethodType = 16,
    Dynamic = 17,
    InvokeDynamic = 18,
    Module = 19,
    Package = 20,
}

impl ConstantPoolTag {
    pub fn from_u8(tag: u8) -> ClassFileResult<ConstantPoolTag> {
        Self::from_repr(tag).ok_or(ClassFileError::BadConstantPoolTag(tag))
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum ConstantPoolEntry<'class> {
    Utf8(Cow<'class, JavaStr>),
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class(Cow<'class, JavaStr>),
    String(Cow<'class, JavaStr>),
    FieldRef(MemberRef<'class>),
    MethodRef(MemberRef<'class>),
    InterfaceMethodRef(MemberRef<'class>),
    NameAndType(NameAndType<'class>),
    MethodHandle(Handle<'class>),
    MethodType(Cow<'class, JavaStr>),
    Dynamic(DynamicEntry<'class>),
    InvokeDynamic(DynamicEntry<'class>),
    Module(Cow<'class, JavaStr>),
    Package(Cow<'class, JavaStr>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameAndType<'class> {
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemberRef<'class> {
    pub owner: Cow<'class, JavaStr>,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DynamicEntry<'class> {
    pub bootstrap_method_attr_index: u16,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
}

#[derive(Clone)]
pub struct ConstantPool<'class> {
    buffer: ClassBuffer<'class>,
    offset: Box<[usize]>,
}

impl<'class> ConstantPool<'class> {
    pub(crate) fn new(
        buffer: ClassBuffer<'class>,
    ) -> ClassFileResult<(ConstantPool<'class>, usize)> {
        let constant_pool_count = buffer.read_u16(8)? as usize;
        let mut cp_offset = vec![0; constant_pool_count].into_boxed_slice();
        let mut current_offset = 10;
        let mut i = 1;
        while i < constant_pool_count {
            cp_offset[i] = current_offset;
            let tag = ConstantPoolTag::from_u8(buffer.read_u8(current_offset)?)?;
            current_offset += 1;
            match tag {
                ConstantPoolTag::Class
                | ConstantPoolTag::MethodType
                | ConstantPoolTag::Module
                | ConstantPoolTag::String
                | ConstantPoolTag::Package => current_offset += 2,
                ConstantPoolTag::MethodHandle => current_offset += 3,
                ConstantPoolTag::Dynamic
                | ConstantPoolTag::FieldRef
                | ConstantPoolTag::Float
                | ConstantPoolTag::Integer
                | ConstantPoolTag::InterfaceMethodRef
                | ConstantPoolTag::InvokeDynamic
                | ConstantPoolTag::MethodRef
                | ConstantPoolTag::NameAndType => current_offset += 4,
                ConstantPoolTag::Double | ConstantPoolTag::Long => {
                    current_offset += 8;
                    i += 1;
                }
                ConstantPoolTag::Utf8 => {
                    current_offset += 2 + buffer.read_u16(current_offset)? as usize
                }
            }
            i += 1;
        }

        let constant_pool = ConstantPool {
            buffer,
            offset: cp_offset,
        };
        Ok((constant_pool, current_offset))
    }

    fn index_to_offset(&self, index: u16) -> ClassFileResult<usize> {
        match self.offset.get(index as usize) {
            Some(&0) => Err(ClassFileError::BadConstantPoolIndexNoEntry(index)),
            Some(&offset) => Ok(offset),
            None => Err(ClassFileError::BadConstantPoolIndex {
                index,
                len: self.offset.len(),
            }),
        }
    }

    pub fn get_type(&self, index: u16) -> ClassFileResult<ConstantPoolTag> {
        let offset = self.index_to_offset(index)?;
        ConstantPoolTag::from_u8(self.buffer.read_u8(offset)?)
    }

    pub fn get_optional(&self, index: u16) -> ClassFileResult<Option<ConstantPoolEntry<'class>>> {
        if index == 0 {
            return Ok(None);
        }

        self.get(index).map(Some)
    }

    pub fn get_utf8_as_bytes(&self, index: u16) -> ClassFileResult<&[u8]> {
        let offset = self.index_to_offset(index)?;
        let tag = ConstantPoolTag::from_u8(self.buffer.read_u8(offset)?)?;

        if tag != ConstantPoolTag::Utf8 {
            return Err(ClassFileError::BadConstantPoolType {
                expected: ConstantPoolTag::Utf8,
                actual: tag,
            });
        }

        let len = self.buffer.read_u16(offset + 1)?;
        self.buffer.read_bytes(offset + 3, len as usize)
    }
}

macro_rules! generate_getters {
    ($($tag:ident, $getter:ident, $opt_getter:ident: $ty:ty => $read:expr;)*) => {
        impl<'class> ConstantPool<'class> {
            pub fn get(&self, index: u16) -> ClassFileResult<ConstantPoolEntry<'class>> {
                let offset = self.index_to_offset(index)?;
                let tag = ConstantPoolTag::from_u8(self.buffer.read_u8(offset)?)?;

                match tag {
                    $(
                    ConstantPoolTag::$tag => Ok(ConstantPoolEntry::$tag($read(self, offset)?)),
                    )*
                }
            }

            $(
            pub fn $getter(&self, index: u16) -> ClassFileResult<$ty> {
                let offset = self.index_to_offset(index)?;
                let tag = ConstantPoolTag::from_u8(self.buffer.read_u8(offset)?)?;

                if tag != ConstantPoolTag::$tag {
                    return Err(ClassFileError::BadConstantPoolType { expected: ConstantPoolTag::$tag, actual: tag });
                }

                $read(self, offset)
            }

            pub fn $opt_getter(&self, index: u16) -> ClassFileResult<Option<$ty>> {
                if index == 0 {
                    return Ok(None);
                }
                self.$getter(index).map(Some)
            }
            )*
        }
    }
}

generate_getters! {
    Utf8, get_utf8, get_optional_utf8: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        let len = this.buffer.read_u16(offset + 1)?;
        Ok(JavaStr::from_modified_utf8(this.buffer.read_bytes(offset + 3, len as usize)?)?)
    };
    Integer, get_i32, get_optional_i32: i32 => |this: &ConstantPool<'class>, offset| -> ClassFileResult<i32> {
        this.buffer.read_i32(offset + 1)
    };
    Float, get_f32, get_optional_f32: f32 => |this: &ConstantPool<'class>, offset| -> ClassFileResult<f32> {
        this.buffer.read_f32(offset + 1)
    };
    Long, get_i64, get_optional_i64: i64 => |this: &ConstantPool<'class>, offset| -> ClassFileResult<i64> {
        this.buffer.read_i64(offset + 1)
    };
    Double, get_f64, get_optional_f64: f64 => |this: &ConstantPool<'class>, offset| -> ClassFileResult<f64> {
        this.buffer.read_f64(offset + 1)
    };
    Class, get_class, get_optional_class: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        this.get_utf8(this.buffer.read_u16(offset + 1)?)
    };
    String, get_string, get_optional_string: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        this.get_utf8(this.buffer.read_u16(offset + 1)?)
    };
    FieldRef, get_field_ref, get_optional_field_ref: MemberRef<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<MemberRef<'class>> {
        let owner = this.get_class(this.buffer.read_u16(offset + 1)?)?;
        let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
        Ok(MemberRef { owner, name: name_and_type.name, desc: name_and_type.desc })
    };
    MethodRef, get_method_ref, get_optional_method_ref: MemberRef<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<MemberRef<'class>> {
        let owner = this.get_class(this.buffer.read_u16(offset + 1)?)?;
        let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
        Ok(MemberRef { owner, name: name_and_type.name, desc: name_and_type.desc })
    };
    InterfaceMethodRef, get_interface_method_ref, get_optional_interface_method_ref: MemberRef<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<MemberRef<'class>> {
        let owner = this.get_class(this.buffer.read_u16(offset + 1)?)?;
        let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
        Ok(MemberRef { owner, name: name_and_type.name, desc: name_and_type.desc })
    };
    NameAndType, get_name_and_type, get_optional_name_and_type: NameAndType<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<NameAndType<'class>> {
        let name = this.get_utf8(this.buffer.read_u16(offset + 1)?)?;
        let desc = this.get_utf8(this.buffer.read_u16(offset + 3)?)?;
        Ok(NameAndType { name, desc })
    };
    MethodHandle, get_method_handle, get_optional_method_handle: Handle<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Handle<'class>> {
        let kind = HandleKind::from_u8(this.buffer.read_u8(offset + 1)?)?;
        let ref_index = this.buffer.read_u16(offset + 2)?;
        let (member_ref, is_interface) = match kind {
            HandleKind::GetField | HandleKind::GetStatic | HandleKind::PutField | HandleKind::PutStatic => (this.get_field_ref(ref_index)?, false),
            HandleKind::InvokeVirtual | HandleKind::NewInvokeSpecial => (this.get_method_ref(ref_index)?, false),
            HandleKind::InvokeInterface => (this.get_interface_method_ref(ref_index)?, true),
            HandleKind::InvokeStatic | HandleKind::InvokeSpecial => {
                let offset = this.index_to_offset(ref_index)?;
                let tag = ConstantPoolTag::from_u8(this.buffer.read_u8(offset)?)?;

                if tag != ConstantPoolTag::MethodRef && tag != ConstantPoolTag::InterfaceMethodRef {
                    return Err(ClassFileError::BadConstantPoolType { expected: ConstantPoolTag::MethodRef, actual: tag });
                }

                let owner = this.get_class(this.buffer.read_u16(offset + 1)?)?;
                let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
                (MemberRef { owner, name: name_and_type.name, desc: name_and_type.desc }, tag == ConstantPoolTag::InterfaceMethodRef)
            }
        };
        Ok(Handle { kind, owner: member_ref.owner, name: member_ref.name, desc: member_ref.desc, is_interface })
    };
    MethodType, get_method_type, get_optional_method_type: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        this.get_utf8(this.buffer.read_u16(offset + 1)?)
    };
    Dynamic, get_dynamic, get_optional_dynamic: DynamicEntry<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<DynamicEntry<'class>> {
        let bootstrap_method_attr_index = this.buffer.read_u16(offset + 1)?;
        let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
        Ok(DynamicEntry { bootstrap_method_attr_index, name: name_and_type.name, desc: name_and_type.desc })
    };
    InvokeDynamic, get_invoke_dynamic, get_optional_invoke_dynamic: DynamicEntry<'class> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<DynamicEntry<'class>> {
        let bootstrap_method_attr_index = this.buffer.read_u16(offset + 1)?;
        let name_and_type = this.get_name_and_type(this.buffer.read_u16(offset + 3)?)?;
        Ok(DynamicEntry { bootstrap_method_attr_index, name: name_and_type.name, desc: name_and_type.desc })
    };
    Module, get_module, get_optional_module: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        this.get_utf8(this.buffer.read_u16(offset + 1)?)
    };
    Package, get_package, get_optional_package: Cow<'class, JavaStr> => |this: &ConstantPool<'class>, offset| -> ClassFileResult<Cow<'class, JavaStr>> {
        this.get_utf8(this.buffer.read_u16(offset + 1)?)
    };
}

impl<'a, 'class> IntoIterator for &'a ConstantPool<'class> {
    type Item = ClassFileResult<ConstantPoolEntry<'class>>;
    type IntoIter = ConstantPoolIntoIter<'a, 'class>;

    fn into_iter(self) -> Self::IntoIter {
        ConstantPoolIntoIter {
            constant_pool: self,
            index: 0,
        }
    }
}

#[derive(Copy, Clone)]
pub struct ConstantPoolIntoIter<'a, 'class> {
    constant_pool: &'a ConstantPool<'class>,
    index: u16,
}

impl<'class> Iterator for ConstantPoolIntoIter<'_, 'class> {
    type Item = ClassFileResult<ConstantPoolEntry<'class>>;

    fn next(&mut self) -> Option<Self::Item> {
        let cp_max = (self.constant_pool.offset.len() - 1) as u16;

        if self.index == cp_max {
            return None;
        }

        self.index += 1;

        if self.constant_pool.offset[self.index as usize] == 0 && self.index < cp_max {
            self.index += 1;
        }

        if self.constant_pool.offset[self.index as usize] == 0 {
            return None;
        }

        Some(self.constant_pool.get(self.index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // lowest case: every entry takes 2 slots, (len - 1) / 2
        // highest case: no entry takes 2 slots, len - 1
        (
            (self.constant_pool.offset.len() - 1) / 2,
            Some(self.constant_pool.offset.len() - 1),
        )
    }
}
