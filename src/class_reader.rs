use crate::constants::MAX_ANNOTATION_NESTING;
use crate::tree::{AnnotationNode, AnnotationValue, TypeAnnotationNode};
use crate::type_annotation::TypeReferenceTargetType;
use crate::{
    AnnotationEvent, Attribute, AttributeReader, ClassAccess, ClassClassEvent, ClassEvent,
    ClassEventProviders, ClassEventSource, ClassFieldEvent, ClassFileError, ClassFileResult,
    ClassInnerClassEvent, ClassMethodEvent, ClassModuleEvent, ClassOuterClassEvent,
    ClassRecordComponentEvent, ClassSourceEvent, ConstantPool, DefaultLabelCreator, FieldEvent,
    FieldEventProviders, InnerClassAccess, MethodEvent, MethodEventProviders,
    MethodParameterAnnotationEvent, MethodParameterEvent, ModuleAccess, ModuleEvent,
    ModuleEventProviders, ModuleProvidesEvent, ModuleRelationAccess, ModuleRelationEvent,
    ModuleRequireAccess, ModuleRequireEvent, RecordComponentEvent, RecordComponentEventProviders,
    TypePath, TypeReference, LATEST_MAJOR_VERSION,
};
use bitflags::bitflags;
use java_string::{JavaStr, JavaString};
use std::borrow::Cow;
use std::collections::HashMap;
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem;
use std::slice::SliceIndex;

macro_rules! define_simple_iterator {
    ($name:ident, $item_type:ty, $read_func:expr) => {
        pub struct $name<'reader, 'class> {
            reader: &'reader ClassReader<'class>,
            count: u16,
            remaining: u16,
            offset: usize,
        }

        impl<'reader, 'class> $name<'reader, 'class> {
            fn new(reader: &'reader ClassReader<'class>, count: u16, offset: usize) -> Self {
                Self {
                    reader,
                    count,
                    remaining: count,
                    offset,
                }
            }
        }

        impl<'reader, 'class> Iterator for $name<'reader, 'class> {
            type Item = ClassFileResult<$item_type>;

            fn next(&mut self) -> Option<Self::Item> {
                if self.remaining == 0 {
                    return None;
                }

                self.remaining -= 1;
                Some($read_func(self.reader, &mut self.offset))
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                (self.count as usize, Some(self.count as usize))
            }
        }

        impl FusedIterator for $name<'_, '_> {}

        impl ExactSizeIterator for $name<'_, '_> {}
    };
}

bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
    pub struct ClassReaderFlags: u8 {
        const None = 0;
        const SkipCode = 1;
        const SkipDebug = 2;
        const SkipFrames = 4;
        const ExpandFrames = 8;
    }
}

#[derive(Clone)]
pub struct ClassReader<'class> {
    buffer: ClassBuffer<'class>,
    pub constant_pool: ConstantPool<'class>,
    metadata_start: usize,
    reader_flags: ClassReaderFlags,
    attribute_readers: HashMap<JavaString, Box<dyn AttributeReader>>,
}

impl<'class> ClassReader<'class> {
    pub fn new(
        data: &'class [u8],
        reader_flags: ClassReaderFlags,
    ) -> ClassFileResult<ClassReader<'class>> {
        let buffer = ClassBuffer { data };

        if buffer.read_u32(0)? != 0xcafebabe {
            return Err(ClassFileError::BadMagic);
        }
        if buffer.read_u16(6)? > LATEST_MAJOR_VERSION {
            return Err(ClassFileError::UnsupportedVersion(buffer.read_u16(6)?));
        }

        let (constant_pool, metadata_start) = ConstantPool::new(buffer)?;

        Ok(ClassReader {
            buffer,
            constant_pool,
            metadata_start,
            reader_flags,
            attribute_readers: HashMap::new(),
        })
    }

    pub fn add_attribute_reader<R>(&mut self, attribute_name: impl Into<JavaString>, reader: R)
    where
        R: AttributeReader,
    {
        self.attribute_readers
            .insert(attribute_name.into(), Box::new(reader));
    }

    pub fn major_version(&self) -> u16 {
        self.buffer
            .read_u16(6)
            .expect("couldn't read value before constant pool")
    }

    pub fn minor_version(&self) -> u16 {
        self.buffer
            .read_u16(8)
            .expect("couldn't read value before constant pool")
    }

    pub fn access(&self) -> ClassFileResult<ClassAccess> {
        Ok(ClassAccess::from_bits_retain(
            self.buffer.read_u16(self.metadata_start)?,
        ))
    }

    pub fn name(&self) -> ClassFileResult<Cow<'class, JavaStr>> {
        self.constant_pool
            .get_class(self.buffer.read_u16(self.metadata_start + 2)?)
    }

    pub fn super_name(&self) -> ClassFileResult<Option<Cow<'class, JavaStr>>> {
        self.constant_pool
            .get_optional_class(self.buffer.read_u16(self.metadata_start + 4)?)
    }

    pub fn interfaces(&self) -> ClassFileResult<InterfacesIterator<'_, 'class>> {
        let interface_count = self.buffer.read_u16(self.metadata_start + 6)? as usize;
        Ok(InterfacesIterator {
            reader: self,
            interface_count,
            index: 0,
        })
    }
}

#[derive(Copy, Clone)]
pub struct InterfacesIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    interface_count: usize,
    index: usize,
}

impl<'class> Iterator for InterfacesIterator<'_, 'class> {
    type Item = ClassFileResult<Cow<'class, JavaStr>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.interface_count {
            return None;
        }

        let index = self.index;
        self.index += 1;

        Some(
            self.reader
                .buffer
                .read_u16(self.reader.metadata_start + 8 + index * 2)
                .and_then(|itf_index| self.reader.constant_pool.get_class(itf_index)),
        )
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.interface_count, Some(self.interface_count))
    }
}

#[derive(Copy, Clone)]
pub struct ClassBuffer<'class> {
    data: &'class [u8],
}

impl<'class> ClassBuffer<'class> {
    fn read_array<const N: usize>(&self, index: usize) -> ClassFileResult<[u8; N]> {
        let slice = self.read_bytes(index, N)?;
        // SAFETY: just read the correct amount of bytes so the conversion to array should succeed
        let array = unsafe { slice.try_into().unwrap_unchecked() };
        Ok(array)
    }

    pub fn read_u8(&self, index: usize) -> ClassFileResult<u8> {
        self.read_array::<1>(index).map(|arr| arr[0])
    }

    pub fn read_u16(&self, index: usize) -> ClassFileResult<u16> {
        self.read_array::<2>(index).map(u16::from_be_bytes)
    }

    pub fn read_u32(&self, index: usize) -> ClassFileResult<u32> {
        self.read_array::<4>(index).map(u32::from_be_bytes)
    }

    pub fn read_u64(&self, index: usize) -> ClassFileResult<u64> {
        self.read_array::<8>(index).map(u64::from_be_bytes)
    }

    pub fn read_i8(&self, index: usize) -> ClassFileResult<i8> {
        self.read_u8(index).map(|u| u as i8)
    }

    pub fn read_i16(&self, index: usize) -> ClassFileResult<i16> {
        self.read_u16(index).map(|u| u as i16)
    }

    pub fn read_i32(&self, index: usize) -> ClassFileResult<i32> {
        self.read_u32(index).map(|u| u as i32)
    }

    pub fn read_i64(&self, index: usize) -> ClassFileResult<i64> {
        self.read_u64(index).map(|u| u as i64)
    }

    pub fn read_f32(&self, index: usize) -> ClassFileResult<f32> {
        self.read_u32(index).map(f32::from_bits)
    }

    pub fn read_f64(&self, index: usize) -> ClassFileResult<f64> {
        self.read_u64(index).map(f64::from_bits)
    }

    pub fn read_bytes(&self, index: usize, len: usize) -> ClassFileResult<&'class [u8]> {
        self.data
            .get(index..index + len)
            .ok_or_else(|| ClassFileError::OutOfBounds {
                index: index + len - 1,
                len: self.data.len(),
            })
    }

    pub fn slice<R>(&self, range: R) -> ClassBuffer<'class>
    where
        R: SliceIndex<[u8], Output = [u8]>,
    {
        ClassBuffer {
            data: &self.data[range],
        }
    }
}

impl<'reader, 'class> ClassEventSource<'class> for &'reader ClassReader<'class> {
    type Providers = ClassReaderEventProviders<'reader, 'class>;
    type Iterator = ClassReaderEvents<'reader, 'class>;

    fn events(self) -> ClassFileResult<Self::Iterator> {
        let interfaces: ClassFileResult<Vec<_>> = self.interfaces()?.collect();
        let interfaces = interfaces?;
        let mut signature_offset = 0;
        let mut source_offset = 0;
        let mut source_debug_offset = 0;
        let mut module_offset = 0;
        let mut module_packages_offset = 0;
        let mut module_main_offset = 0;
        let mut nest_host_offset = 0;
        let mut enclosing_method_offset = 0;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut nest_members_count = 0;
        let mut nest_members_offset = 0;
        let mut permitted_subclasses_count = 0;
        let mut permitted_subclasses_offset = 0;
        let mut inner_classes_count = 0;
        let mut inner_classes_offset = 0;
        let mut record_components_count = 0;
        let mut record_components_offset = 0;
        let mut custom_attributes_offsets = Vec::new();

        let mut pos = self.metadata_start + 8 + interfaces.len() * 2;

        let fields_count = self.buffer.read_u16(pos)?;
        pos += 2;
        let fields_offset = pos;

        for _ in 0..fields_count {
            pos += 6;
            let attributes_count = self.buffer.read_u16(pos)?;
            pos += 2;
            for _ in 0..attributes_count {
                pos += 2;
                let attribute_length = self.buffer.read_u32(pos)?;
                pos += 4 + attribute_length as usize;
            }
        }

        let methods_count = self.buffer.read_u16(pos)?;
        pos += 2;
        let methods_offset = pos;

        for _ in 0..methods_count {
            pos += 6;
            let attributes_count = self.buffer.read_u16(pos)?;
            pos += 2;
            for _ in 0..attributes_count {
                pos += 2;
                let attribute_length = self.buffer.read_u32(pos)?;
                pos += 4 + attribute_length as usize;
            }
        }

        let attributes_count = self.buffer.read_u16(pos)?;
        pos += 2;

        for _ in 0..attributes_count {
            let attribute_name = self
                .constant_pool
                .get_utf8_as_bytes(self.buffer.read_u16(pos)?)?;
            pos += 2;
            let attribute_length = self.buffer.read_u32(pos)?;
            pos += 4;

            match attribute_name {
                b"Signature" => signature_offset = pos,
                b"SourceFile" => source_offset = pos,
                b"SourceDebugExtension" => source_debug_offset = pos - 4,
                b"Module" => module_offset = pos,
                b"ModulePackages" => module_packages_offset = pos,
                b"ModuleMainClass" => module_main_offset = pos,
                b"NestHost" => nest_host_offset = pos,
                b"EnclosingMethod" => enclosing_method_offset = pos,
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = self.buffer.read_u16(pos)?;
                    visible_annotations_offset = pos + 2;
                }
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = self.buffer.read_u16(pos)?;
                    invisible_annotations_offset = pos + 2;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = self.buffer.read_u16(pos)?;
                    visible_type_annotations_offset = pos + 2;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = self.buffer.read_u16(pos)?;
                    invisible_type_annotations_offset = pos + 2;
                }
                b"NestMembers" => {
                    nest_members_count = self.buffer.read_u16(pos)?;
                    nest_members_offset = pos + 2;
                }
                b"PermittedSubclasses" => {
                    permitted_subclasses_count = self.buffer.read_u16(pos)?;
                    permitted_subclasses_offset = pos + 2;
                }
                b"InnerClasses" => {
                    inner_classes_count = self.buffer.read_u16(pos)?;
                    inner_classes_offset = pos + 2;
                }
                b"Record" => {
                    record_components_count = self.buffer.read_u16(pos)?;
                    record_components_offset = pos + 2;
                }
                _ => custom_attributes_offsets.push(pos - 6),
            }

            pos += attribute_length as usize;
        }

        Ok(ClassReaderEvents {
            reader: self,
            signature_offset,
            interfaces,
            fields_offset,
            fields_count,
            methods_offset,
            methods_count,
            source_offset,
            source_debug_offset,
            module_offset,
            module_packages_offset,
            module_main_offset,
            nest_host_offset,
            enclosing_method_offset,
            visible_annotations_count,
            visible_annotations_offset,
            invisible_annotations_count,
            invisible_annotations_offset,
            visible_type_annotations_count,
            visible_type_annotations_offset,
            invisible_type_annotations_count,
            invisible_type_annotations_offset,
            custom_attributes_offsets,
            nest_members_count,
            nest_members_offset,
            permitted_subclasses_count,
            permitted_subclasses_offset,
            inner_classes_count,
            inner_classes_offset,
            record_components_count,
            record_components_offset,
            state: 0,
        })
    }
}

pub struct ClassReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    signature_offset: usize,
    interfaces: Vec<Cow<'class, JavaStr>>,
    fields_offset: usize,
    fields_count: u16,
    methods_offset: usize,
    methods_count: u16,
    source_offset: usize,
    source_debug_offset: usize,
    module_offset: usize,
    module_packages_offset: usize,
    module_main_offset: usize,
    nest_host_offset: usize,
    enclosing_method_offset: usize,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    custom_attributes_offsets: Vec<usize>,
    nest_members_count: u16,
    nest_members_offset: usize,
    permitted_subclasses_count: u16,
    permitted_subclasses_offset: usize,
    inner_classes_count: u16,
    inner_classes_offset: usize,
    record_components_count: u16,
    record_components_offset: usize,
    state: u8,
}

impl<'reader, 'class> ClassReaderEvents<'reader, 'class> {
    fn class_internal(&mut self) -> ClassFileResult<ClassClassEvent<'class>> {
        Ok(ClassClassEvent {
            major_version: self.reader.major_version(),
            minor_version: self.reader.minor_version(),
            name: self.reader.name()?,
            super_name: self.reader.super_name()?,
            signature: self.signature()?,
            interfaces: mem::take(&mut self.interfaces),
        })
    }

    pub fn signature(&self) -> ClassFileResult<Option<Cow<'class, JavaStr>>> {
        if self.signature_offset == 0 {
            return Ok(None);
        }

        Ok(Some(self.reader.constant_pool.get_utf8(
            self.reader.buffer.read_u16(self.signature_offset)?,
        )?))
    }

    pub fn source(&self) -> ClassFileResult<Option<ClassSourceEvent<'class>>> {
        if self
            .reader
            .reader_flags
            .contains(ClassReaderFlags::SkipDebug)
        {
            return Ok(None);
        }

        if self.source_offset == 0 && self.source_debug_offset == 0 {
            return Ok(None);
        }

        let source = if self.source_offset == 0 {
            None
        } else {
            Some(
                self.reader
                    .constant_pool
                    .get_utf8(self.reader.buffer.read_u16(self.source_offset)?)?,
            )
        };
        let debug = if self.source_debug_offset == 0 {
            None
        } else {
            let len = self.reader.buffer.read_u32(self.source_debug_offset)?;
            Some(JavaStr::from_modified_utf8(
                self.reader
                    .buffer
                    .read_bytes(self.source_debug_offset + 4, len as usize)?,
            )?)
        };
        Ok(Some(ClassSourceEvent { source, debug }))
    }

    fn module(
        &self,
    ) -> ClassFileResult<Option<ClassModuleEvent<'class, ModuleReaderEvents<'reader, 'class>>>>
    {
        if self.module_offset == 0 {
            return Ok(None);
        }

        let name = self
            .reader
            .constant_pool
            .get_module(self.reader.buffer.read_u16(self.module_offset)?)?;
        let access =
            ModuleAccess::from_bits_retain(self.reader.buffer.read_u16(self.module_offset + 2)?);
        let version = self
            .reader
            .constant_pool
            .get_optional_utf8(self.reader.buffer.read_u16(self.module_offset + 4)?)?;

        Ok(Some(ClassModuleEvent {
            name,
            access,
            version,
            events: ModuleReaderEvents {
                reader: self.reader,
                offset: self.module_offset + 6,
                packages_offset: self.module_packages_offset,
                main_offset: self.module_main_offset,
                state: 0,
            },
        }))
    }

    fn nest_host(&self) -> ClassFileResult<Option<Cow<'class, JavaStr>>> {
        if self.nest_host_offset == 0 {
            return Ok(None);
        }

        Ok(Some(self.reader.constant_pool.get_class(
            self.reader.buffer.read_u16(self.nest_host_offset)?,
        )?))
    }

    fn outer_class(&self) -> ClassFileResult<Option<ClassOuterClassEvent<'class>>> {
        if self.enclosing_method_offset == 0 {
            return Ok(None);
        }

        let owner = self
            .reader
            .constant_pool
            .get_class(self.reader.buffer.read_u16(self.enclosing_method_offset)?)?;
        let method = self.reader.constant_pool.get_optional_name_and_type(
            self.reader
                .buffer
                .read_u16(self.enclosing_method_offset + 2)?,
        )?;
        if let Some(method) = method {
            Ok(Some(ClassOuterClassEvent {
                owner,
                method_name: Some(method.name),
                method_desc: Some(method.desc),
            }))
        } else {
            Ok(Some(ClassOuterClassEvent {
                owner,
                method_name: None,
                method_desc: None,
            }))
        }
    }

    fn annotations(&self) -> AnnotationReaderIterator<'reader, 'class> {
        AnnotationReaderIterator::new(
            self.reader,
            self.visible_annotations_count,
            self.visible_annotations_offset,
            self.invisible_annotations_count,
            self.invisible_annotations_offset,
        )
    }

    fn type_annotations(&self) -> TypeAnnotationReaderIterator<'reader, 'class> {
        TypeAnnotationReaderIterator::new(
            self.reader,
            self.visible_type_annotations_count,
            self.visible_type_annotations_offset,
            self.invisible_type_annotations_count,
            self.invisible_type_annotations_offset,
        )
    }

    fn attributes(&self) -> CustomAttributeReaderIterator<'reader, 'class> {
        CustomAttributeReaderIterator::new(self.reader, self.custom_attributes_offsets.clone())
    }

    fn nest_members(&self) -> ClassesReaderIterator<'reader, 'class> {
        ClassesReaderIterator::new(
            self.reader,
            self.nest_members_count,
            self.nest_members_offset,
        )
    }

    fn permitted_subclasses(&self) -> ClassesReaderIterator<'reader, 'class> {
        ClassesReaderIterator::new(
            self.reader,
            self.permitted_subclasses_count,
            self.permitted_subclasses_offset,
        )
    }

    fn inner_classes(&self) -> ClassInnerClassesReaderIterator<'reader, 'class> {
        ClassInnerClassesReaderIterator::new(
            self.reader,
            self.inner_classes_count,
            self.inner_classes_offset,
        )
    }

    fn record_components(&self) -> ClassRecordComponentsReaderIterator<'reader, 'class> {
        ClassRecordComponentsReaderIterator::new(
            self.reader,
            self.record_components_count,
            self.record_components_offset,
        )
    }

    fn fields(&self) -> ClassFieldsIterator<'reader, 'class> {
        ClassFieldsIterator::new(self.reader, self.fields_count, self.fields_offset)
    }

    fn methods(&self) -> ClassMethodsIterator<'reader, 'class> {
        ClassMethodsIterator::new(self.reader, self.methods_count, self.methods_offset)
    }
}

impl<'reader, 'class> Iterator for ClassReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<ClassEvent<'class, ClassReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let state = self.state;
            self.state += 1;
            match state {
                0 => {
                    return Some(self.class_internal().map(ClassEvent::Class));
                }
                1 => {
                    if let Some(source) = self.source().transpose() {
                        return Some(source.map(ClassEvent::Source));
                    }
                }
                2 => {
                    if let Some(module) = self.module().transpose() {
                        return Some(module.map(ClassEvent::Module));
                    }
                }
                3 => {
                    if let Some(nest_host) = self.nest_host().transpose() {
                        return Some(nest_host.map(ClassEvent::NestHost));
                    }
                }
                4 => {
                    if let Some(outer_class) = self.outer_class().transpose() {
                        return Some(outer_class.map(ClassEvent::OuterClass));
                    }
                }
                5 => {
                    if self.visible_annotations_offset != 0
                        || self.invisible_annotations_offset != 0
                    {
                        return Some(Ok(ClassEvent::Annotations(self.annotations())));
                    }
                }
                6 => {
                    if self.visible_type_annotations_offset != 0
                        || self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(ClassEvent::TypeAnnotations(self.type_annotations())));
                    }
                }
                7 => {
                    if !self.custom_attributes_offsets.is_empty() {
                        return Some(Ok(ClassEvent::Attributes(self.attributes())));
                    }
                }
                8 => {
                    if self.nest_members_offset != 0 {
                        return Some(Ok(ClassEvent::NestMembers(self.nest_members())));
                    }
                }
                9 => {
                    if self.permitted_subclasses_offset != 0 {
                        return Some(Ok(ClassEvent::PermittedSubclasses(
                            self.permitted_subclasses(),
                        )));
                    }
                }
                10 => {
                    if self.inner_classes_offset != 0 {
                        return Some(Ok(ClassEvent::InnerClasses(self.inner_classes())));
                    }
                }
                11 => {
                    if self.record_components_offset != 0 {
                        return Some(Ok(ClassEvent::Record(self.record_components())));
                    }
                }
                12 => {
                    if self.fields_count != 0 {
                        return Some(Ok(ClassEvent::Fields(self.fields())));
                    }
                }
                13 => {
                    if self.methods_count != 0 {
                        return Some(Ok(ClassEvent::Methods(self.methods())));
                    }
                }
                _ => return None,
            }
        }
    }
}

pub struct ClassReaderEventProviders<'reader, 'class>(
    PhantomData<&'reader ()>,
    PhantomData<&'class ()>,
);

impl<'reader, 'class> ClassEventProviders<'class> for ClassReaderEventProviders<'reader, 'class>
where
    'class: 'reader,
{
    type ModuleSubProviders = ModuleReaderEventProviders<'reader, 'class>;
    type ModuleEvents = ModuleReaderEvents<'reader, 'class>;

    type Annotations = AnnotationReaderIterator<'reader, 'class>;

    type TypeAnnotations = TypeAnnotationReaderIterator<'reader, 'class>;

    type Attributes = CustomAttributeReaderIterator<'reader, 'class>;

    type NestMembers = ClassesReaderIterator<'reader, 'class>;

    type PermittedSubclasses = ClassesReaderIterator<'reader, 'class>;

    type InnerClasses = ClassInnerClassesReaderIterator<'reader, 'class>;

    type RecordComponentSubProviders = RecordComponentReaderEventProviders<'reader, 'class>;
    type RecordComponentEvents = RecordComponentReaderEvents<'reader, 'class>;
    type RecordComponents = ClassRecordComponentsReaderIterator<'reader, 'class>;

    type FieldSubProviders = FieldReaderEventProviders<'reader, 'class>;
    type FieldEvents = FieldReaderEvents<'reader, 'class>;
    type Fields = ClassFieldsIterator<'reader, 'class>;

    type MethodSubProviders = MethodReaderEventProviders<'reader, 'class>;
    type MethodEvents = MethodReaderEvents<'reader, 'class>;
    type Methods = ClassMethodsIterator<'reader, 'class>;
}

define_simple_iterator!(
    ClassInnerClassesReaderIterator,
    ClassInnerClassEvent<'class>,
    |reader: &ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let name = reader
            .constant_pool
            .get_class(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let outer_name = reader
            .constant_pool
            .get_optional_class(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let inner_name = reader
            .constant_pool
            .get_optional_class(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let access = InnerClassAccess::from_bits_retain(reader.buffer.read_u16(*offset)?);
        *offset += 2;
        Ok(ClassInnerClassEvent {
            name,
            outer_name,
            inner_name,
            access,
        })
    }
);

define_simple_iterator!(
    ClassRecordComponentsReaderIterator,
    ClassRecordComponentEvent<'class, RecordComponentReaderEvents<'reader, 'class>>,
    |reader: &'reader ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let name = reader
            .constant_pool
            .get_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let desc = reader
            .constant_pool
            .get_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let attribute_count = reader.buffer.read_u16(*offset)?;
        *offset += 2;

        let mut signature = None;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut custom_attributes_offsets = Vec::new();

        for _ in 0..attribute_count {
            let attribute_name = reader
                .constant_pool
                .get_utf8_as_bytes(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            let attribute_length = reader.buffer.read_u32(*offset)?;
            *offset += 4;

            match attribute_name {
                b"Signature" => {
                    signature = Some(
                        reader
                            .constant_pool
                            .get_utf8(reader.buffer.read_u16(*offset)?)?,
                    )
                }
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_annotations_offset = *offset + 2;
                }
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_annotations_offset = *offset + 2;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_type_annotations_offset = *offset + 2;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_type_annotations_offset = *offset + 2;
                }
                _ => custom_attributes_offsets.push(*offset - 6),
            }

            *offset += attribute_length as usize;
        }

        Ok(ClassRecordComponentEvent {
            name,
            desc,
            signature,
            events: RecordComponentReaderEvents {
                reader,
                visible_annotations_count,
                visible_annotations_offset,
                invisible_annotations_count,
                invisible_annotations_offset,
                visible_type_annotations_count,
                visible_type_annotations_offset,
                invisible_type_annotations_count,
                invisible_type_annotations_offset,
                custom_attributes_offsets,
            },
        })
    }
);

define_simple_iterator!(
    ClassFieldsIterator,
    ClassFieldEvent<'class, FieldReaderEvents<'reader, 'class>>,
    |reader, offset| { todo!() }
);

define_simple_iterator!(
    ClassMethodsIterator,
    ClassMethodEvent<'class, MethodReaderEvents<'reader, 'class>>,
    |reader, offset| { todo!() }
);

pub struct FieldReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
}

impl<'reader, 'class> Iterator for FieldReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<FieldEvent<'class, FieldReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct FieldReaderEventProviders<'reader, 'class>(
    PhantomData<&'reader ()>,
    PhantomData<&'class ()>,
);

impl<'reader, 'class> FieldEventProviders<'class> for FieldReaderEventProviders<'reader, 'class>
where
    'class: 'reader,
{
    type Annotations = AnnotationReaderIterator<'reader, 'class>;

    type TypeAnnotations = TypeAnnotationReaderIterator<'reader, 'class>;

    type Attributes = CustomAttributeReaderIterator<'reader, 'class>;
}

pub struct MethodReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
}

impl<'reader, 'class> Iterator for MethodReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<MethodEvent<'class, MethodReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct MethodReaderEventProviders<'reader, 'class>(
    PhantomData<&'reader ()>,
    PhantomData<&'class ()>,
);

impl<'reader, 'class> MethodEventProviders<'class> for MethodReaderEventProviders<'reader, 'class>
where
    'class: 'reader,
{
    type Parameters = MethodParameterReaderIterator<'reader, 'class>;

    type Annotations = AnnotationReaderIterator<'reader, 'class>;

    type TypeAnnotations = TypeAnnotationReaderIterator<'reader, 'class>;

    type ParameterAnnotations = MethodParameterAnnotationsReaderIterator<'reader, 'class>;

    type Attributes = CustomAttributeReaderIterator<'reader, 'class>;

    type InsnAnnotationEvents = TypeAnnotationReaderIterator<'reader, 'class>;
    type TryCatchAnnotationEvents = TypeAnnotationReaderIterator<'reader, 'class>;
    type LocalVariableAnnotationEvents = TypeAnnotationReaderIterator<'reader, 'class>;

    type CodeAttributes = CustomAttributeReaderIterator<'reader, 'class>;

    type LabelCreator = DefaultLabelCreator;
}

define_simple_iterator!(
    MethodParameterReaderIterator,
    MethodParameterEvent<'class>,
    |reader, offset| { todo!() }
);

pub struct MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    visible_remaining: u16,
    visible_offset: usize,
    invisible_remaining: u16,
    invisible_offset: usize,
}

impl<'reader, 'class> MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    fn new(
        reader: &'reader ClassReader<'class>,
        visible_count: u16,
        visible_offset: usize,
        invisible_count: u16,
        invisible_offset: usize,
    ) -> Self {
        MethodParameterAnnotationsReaderIterator {
            reader,
            visible_remaining: visible_count,
            visible_offset,
            invisible_remaining: invisible_count,
            invisible_offset,
        }
    }
}

impl<'reader, 'class> Iterator for MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    type Item = ClassFileResult<MethodParameterAnnotationEvent<'class>>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

fn read_annotation<'class>(
    reader: &ClassReader<'class>,
    offset: &mut usize,
    depth: u16,
) -> ClassFileResult<AnnotationNode<'class>> {
    if depth > MAX_ANNOTATION_NESTING {
        return Err(ClassFileError::TooDeepAnnotationNesting);
    }

    let desc = reader
        .constant_pool
        .get_utf8(reader.buffer.read_u16(*offset)?)?;
    *offset += 2;

    let values = read_annotation_values(reader, offset, depth)?;

    Ok(AnnotationNode { desc, values })
}

enum TypeAnnotationCodeLocation {
    None,
    LocalVariable(Vec<TypeAnnotationLocalVariableEntry>),
    Insn(u16),
}

impl TypeAnnotationCodeLocation {
    fn read_local_variable(
        reader: &ClassReader<'_>,
        offset: &mut usize,
    ) -> ClassFileResult<Vec<TypeAnnotationLocalVariableEntry>> {
        let table_length = reader.buffer.read_u16(*offset)?;
        *offset += 2;
        let mut table = Vec::with_capacity(table_length as usize);
        for _ in 0..table_length {
            let start_pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let length = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let index = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            table.push(TypeAnnotationLocalVariableEntry {
                start_pc,
                length,
                index,
            });
        }
        Ok(table)
    }
}

struct TypeAnnotationLocalVariableEntry {
    start_pc: u16,
    length: u16,
    index: u16,
}

fn read_type_annotation<'class>(
    reader: &ClassReader<'class>,
    offset: &mut usize,
) -> ClassFileResult<(TypeAnnotationNode<'class>, TypeAnnotationCodeLocation)> {
    let target_type = reader.buffer.read_u8(*offset)?;
    *offset += 1;
    let target_type = TypeReferenceTargetType::from_repr(target_type)
        .ok_or(ClassFileError::BadTypeAnnotationTarget(target_type))?;
    let mut code_location = TypeAnnotationCodeLocation::None;

    let type_ref = match target_type {
        TypeReferenceTargetType::ClassTypeParameter => {
            let param_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            TypeReference::ClassTypeParameter { param_index }
        }
        TypeReferenceTargetType::MethodTypeParameter => {
            let param_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            TypeReference::MethodTypeParameter { param_index }
        }
        TypeReferenceTargetType::ClassExtends => {
            let supertype_index = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            TypeReference::ClassExtends {
                interface_index: if supertype_index == u16::MAX {
                    None
                } else {
                    Some(supertype_index)
                },
            }
        }
        TypeReferenceTargetType::ClassTypeParameterBound => {
            let param_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            let bound_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            TypeReference::ClassTypeParameterBound {
                param_index,
                bound_index,
            }
        }
        TypeReferenceTargetType::MethodTypeParameterBound => {
            let param_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            let bound_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            TypeReference::MethodTypeParameterBound {
                param_index,
                bound_index,
            }
        }
        TypeReferenceTargetType::Field => TypeReference::Field,
        TypeReferenceTargetType::MethodReturn => TypeReference::MethodReturn,
        TypeReferenceTargetType::MethodReceiver => TypeReference::MethodReceiver,
        TypeReferenceTargetType::MethodFormalParameter => {
            let param_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            TypeReference::MethodFormalParameter { param_index }
        }
        TypeReferenceTargetType::Throws => {
            let exception_index = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            TypeReference::Throws { exception_index }
        }
        TypeReferenceTargetType::LocalVariable => {
            code_location = TypeAnnotationCodeLocation::LocalVariable(
                TypeAnnotationCodeLocation::read_local_variable(reader, offset)?,
            );
            TypeReference::LocalVariable
        }
        TypeReferenceTargetType::ResourceVariable => {
            code_location = TypeAnnotationCodeLocation::LocalVariable(
                TypeAnnotationCodeLocation::read_local_variable(reader, offset)?,
            );
            TypeReference::ResourceVariable
        }
        TypeReferenceTargetType::ExceptionParameter => {
            let try_catch_block_index = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            TypeReference::ExceptionParameter {
                try_catch_block_index,
            }
        }
        TypeReferenceTargetType::Instanceof => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::Instanceof
        }
        TypeReferenceTargetType::New => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::New
        }
        TypeReferenceTargetType::ConstructorReference => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::ConstructorReference
        }
        TypeReferenceTargetType::MethodReference => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::MethodReference
        }
        TypeReferenceTargetType::Cast => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let arg_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::Cast { arg_index }
        }
        TypeReferenceTargetType::ConstructorInvocationTypeArgument => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let arg_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::ConstructorInvocationTypeArgument { arg_index }
        }
        TypeReferenceTargetType::MethodInvocationTypeArgument => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let arg_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::MethodInvocationTypeArgument { arg_index }
        }
        TypeReferenceTargetType::ConstructorReferenceTypeArgument => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let arg_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::ConstructorReferenceTypeArgument { arg_index }
        }
        TypeReferenceTargetType::MethodReferenceTypeArgument => {
            let pc = reader.buffer.read_u16(*offset)?;
            *offset += 2;
            let arg_index = reader.buffer.read_u8(*offset)?;
            *offset += 1;
            code_location = TypeAnnotationCodeLocation::Insn(pc);
            TypeReference::MethodReferenceTypeArgument { arg_index }
        }
    };

    let type_path_length = reader.buffer.read_u8(*offset)?;
    *offset += 1;
    let type_path = reader
        .buffer
        .read_bytes(*offset, type_path_length as usize * 2)?;
    *offset += type_path_length as usize * 2;
    let type_path = TypePath::from_bytes(type_path);

    let desc = reader
        .constant_pool
        .get_utf8(reader.buffer.read_u16(*offset)?)?;
    *offset += 2;

    let values = read_annotation_values(reader, offset, 0)?;

    Ok((
        TypeAnnotationNode {
            type_ref,
            type_path,
            desc,
            values,
        },
        code_location,
    ))
}

fn read_annotation_values<'class>(
    reader: &ClassReader<'class>,
    offset: &mut usize,
    depth: u16,
) -> ClassFileResult<Vec<(Cow<'class, JavaStr>, AnnotationValue<'class>)>> {
    let num_values = reader.buffer.read_u16(*offset)?;
    *offset += 2;

    let mut values = Vec::with_capacity(num_values as usize);

    for _ in 0..num_values {
        let name = reader
            .constant_pool
            .get_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let value = read_annotation_value(reader, offset, depth)?;
        values.push((name, value));
    }

    Ok(values)
}

fn read_annotation_array<'class>(
    reader: &ClassReader<'class>,
    offset: &mut usize,
    depth: u16,
) -> ClassFileResult<Vec<AnnotationValue<'class>>> {
    if depth > MAX_ANNOTATION_NESTING {
        return Err(ClassFileError::TooDeepAnnotationNesting);
    }

    let num_values = reader.buffer.read_u16(*offset)?;
    *offset += 2;

    let mut values = Vec::with_capacity(num_values as usize);

    for _ in 0..num_values {
        values.push(read_annotation_value(reader, offset, depth)?);
    }

    Ok(values)
}

fn read_annotation_value<'class>(
    reader: &ClassReader<'class>,
    offset: &mut usize,
    depth: u16,
) -> ClassFileResult<AnnotationValue<'class>> {
    let tag = reader.buffer.read_u8(*offset)?;
    *offset += 1;

    let value = match tag {
        b'B' => {
            let value = reader
                .constant_pool
                .get_i32(reader.buffer.read_u16(*offset)?)? as i8;
            *offset += 2;
            AnnotationValue::Byte(value)
        }
        b'C' => {
            let value = reader
                .constant_pool
                .get_i32(reader.buffer.read_u16(*offset)?)? as u16;
            *offset += 2;
            AnnotationValue::Char(value)
        }
        b'D' => {
            let value = reader
                .constant_pool
                .get_f64(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Double(value)
        }
        b'F' => {
            let value = reader
                .constant_pool
                .get_f32(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Float(value)
        }
        b'I' => {
            let value = reader
                .constant_pool
                .get_i32(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Int(value)
        }
        b'J' => {
            let value = reader
                .constant_pool
                .get_i64(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Long(value)
        }
        b'S' => {
            let value = reader
                .constant_pool
                .get_i32(reader.buffer.read_u16(*offset)?)? as i16;
            *offset += 2;
            AnnotationValue::Short(value)
        }
        b'Z' => {
            let value = reader
                .constant_pool
                .get_i32(reader.buffer.read_u16(*offset)?)?
                != 0;
            *offset += 2;
            AnnotationValue::Boolean(value)
        }
        b's' => {
            let value = reader
                .constant_pool
                .get_utf8(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::String(value)
        }
        b'e' => {
            let desc = reader
                .constant_pool
                .get_utf8(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            let name = reader
                .constant_pool
                .get_utf8(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Enum { desc, name }
        }
        b'c' => {
            let value = reader
                .constant_pool
                .get_utf8(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            AnnotationValue::Class(value)
        }
        b'@' => AnnotationValue::Annotation(read_annotation(reader, offset, depth + 1)?),
        b'[' => AnnotationValue::Array(read_annotation_array(reader, offset, depth + 1)?),
        _ => return Err(ClassFileError::BadAnnotationTag(tag)),
    };

    Ok(value)
}

pub struct AnnotationReaderIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    count: usize,
    visible_remaining: u16,
    visible_offset: usize,
    invisible_remaining: u16,
    invisible_offset: usize,
}

impl<'reader, 'class> AnnotationReaderIterator<'reader, 'class> {
    fn new(
        reader: &'reader ClassReader<'class>,
        visible_count: u16,
        visible_offset: usize,
        invisible_count: u16,
        invisible_offset: usize,
    ) -> Self {
        AnnotationReaderIterator {
            reader,
            count: visible_count as usize + invisible_count as usize,
            visible_remaining: visible_count,
            visible_offset,
            invisible_remaining: invisible_count,
            invisible_offset,
        }
    }

    fn event(
        reader: &'reader ClassReader<'class>,
        visible: bool,
        offset: &mut usize,
    ) -> ClassFileResult<AnnotationEvent<AnnotationNode<'class>>> {
        Ok(AnnotationEvent {
            visible,
            annotation: read_annotation(reader, offset, 0)?,
        })
    }
}

impl<'reader, 'class> Iterator for AnnotationReaderIterator<'reader, 'class> {
    type Item = ClassFileResult<AnnotationEvent<AnnotationNode<'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.visible_remaining != 0 {
            self.visible_remaining -= 1;
            Some(Self::event(self.reader, true, &mut self.visible_offset))
        } else if self.invisible_remaining != 0 {
            self.invisible_remaining -= 1;
            Some(Self::event(self.reader, false, &mut self.invisible_offset))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}

impl FusedIterator for AnnotationReaderIterator<'_, '_> {}

impl ExactSizeIterator for AnnotationReaderIterator<'_, '_> {}

pub struct TypeAnnotationReaderIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    count: usize,
    visible_remaining: u16,
    visible_offset: usize,
    invisible_remaining: u16,
    invisible_offset: usize,
}

impl<'reader, 'class> TypeAnnotationReaderIterator<'reader, 'class> {
    fn new(
        reader: &'reader ClassReader<'class>,
        visible_count: u16,
        visible_offset: usize,
        invisible_count: u16,
        invisible_offset: usize,
    ) -> Self {
        TypeAnnotationReaderIterator {
            reader,
            count: visible_count as usize + invisible_count as usize,
            visible_remaining: visible_count,
            visible_offset,
            invisible_remaining: invisible_count,
            invisible_offset,
        }
    }

    fn event(
        reader: &'reader ClassReader<'class>,
        visible: bool,
        offset: &mut usize,
    ) -> ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>> {
        let (annotation, _) = read_type_annotation(reader, offset)?;
        Ok(AnnotationEvent {
            visible,
            annotation,
        })
    }
}

impl<'reader, 'class> Iterator for TypeAnnotationReaderIterator<'reader, 'class> {
    type Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.visible_remaining != 0 {
            self.visible_remaining -= 1;
            Some(Self::event(self.reader, true, &mut self.visible_offset))
        } else if self.invisible_remaining != 0 {
            self.invisible_remaining -= 1;
            Some(Self::event(self.reader, false, &mut self.invisible_offset))
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}

impl FusedIterator for TypeAnnotationReaderIterator<'_, '_> {}

impl ExactSizeIterator for TypeAnnotationReaderIterator<'_, '_> {}

pub struct ModuleReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    offset: usize,
    packages_offset: usize,
    main_offset: usize,
    state: u8,
}

impl<'reader, 'class> ModuleReaderEvents<'reader, 'class> {
    pub fn main_class(&self) -> ClassFileResult<Option<Cow<'class, JavaStr>>> {
        if self.main_offset == 0 {
            return Ok(None);
        }

        Ok(Some(self.reader.constant_pool.get_utf8(
            self.reader.buffer.read_u16(self.main_offset)?,
        )?))
    }

    pub fn packages(&self) -> ClassFileResult<PackagesReaderIterator<'reader, 'class>> {
        let packages_count = if self.packages_offset == 0 {
            0
        } else {
            self.reader.buffer.read_u16(self.packages_offset)?
        };
        Ok(PackagesReaderIterator::new(
            self.reader,
            packages_count,
            self.packages_offset + 2,
        ))
    }

    fn requires_internal(
        &mut self,
    ) -> ClassFileResult<Option<ModuleRequireReaderIterator<'reader, 'class>>> {
        let requires_count = self.reader.buffer.read_u16(self.offset)?;
        self.offset += 2;

        if requires_count == 0 {
            return Ok(None);
        }

        let start_offset = self.offset;
        self.offset += requires_count as usize * 6;

        Ok(Some(ModuleRequireReaderIterator::new(
            self.reader,
            requires_count,
            start_offset,
        )))
    }

    fn relations_internal(
        &mut self,
    ) -> ClassFileResult<Option<ModuleRelationReaderIterator<'reader, 'class>>> {
        let relation_count = self.reader.buffer.read_u16(self.offset)?;
        self.offset += 2;

        if relation_count == 0 {
            return Ok(None);
        }

        let start_offset = self.offset;
        for _ in 0..relation_count {
            self.offset += 4;
            let module_count = self.reader.buffer.read_u16(self.offset)?;
            self.offset += 2 + module_count as usize * 2;
        }

        Ok(Some(ModuleRelationReaderIterator::new(
            self.reader,
            relation_count,
            start_offset,
        )))
    }

    fn uses_internal(&mut self) -> ClassFileResult<Option<ClassesReaderIterator<'reader, 'class>>> {
        let uses_count = self.reader.buffer.read_u16(self.offset)?;
        self.offset += 2;

        if uses_count == 0 {
            return Ok(None);
        }

        let start_offset = self.offset;
        self.offset += uses_count as usize * 2;
        Ok(Some(ClassesReaderIterator::new(
            self.reader,
            uses_count,
            start_offset,
        )))
    }
}

impl<'reader, 'class> Iterator for ModuleReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<ModuleEvent<'class, ModuleReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let state = self.state;
            self.state += 1;
            match state {
                0 => {
                    if let Some(main_class) = self.main_class().transpose() {
                        return Some(main_class.map(ModuleEvent::MainClass));
                    }
                }
                1 => {
                    if self.packages_offset != 0 {
                        return Some(self.packages().map(ModuleEvent::Packages));
                    }
                }
                2 => {
                    if let Some(requires) = self.requires_internal().transpose() {
                        return Some(requires.map(ModuleEvent::Requires));
                    }
                }
                3 => {
                    if let Some(exports) = self.relations_internal().transpose() {
                        return Some(exports.map(ModuleEvent::Exports));
                    }
                }
                4 => {
                    if let Some(opens) = self.relations_internal().transpose() {
                        return Some(opens.map(ModuleEvent::Opens));
                    }
                }
                5 => {
                    if let Some(uses) = self.uses_internal().transpose() {
                        return Some(uses.map(ModuleEvent::Uses));
                    }
                }
                6 => {
                    // no need to increment the offset here as this is the last thing visited
                    let provides_count = match self.reader.buffer.read_u16(self.offset) {
                        Ok(count) => count,
                        Err(err) => return Some(Err(err)),
                    };
                    if provides_count != 0 {
                        return Some(Ok(ModuleEvent::Provides(
                            ModuleProvidesReaderIterator::new(
                                self.reader,
                                provides_count,
                                self.offset,
                            ),
                        )));
                    }
                }
                _ => return None,
            }
        }
    }
}

define_simple_iterator!(
    ModuleRequireReaderIterator,
    ModuleRequireEvent<'class>,
    |reader: &ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let module = reader
            .constant_pool
            .get_module(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let access = ModuleRequireAccess::from_bits_retain(reader.buffer.read_u16(*offset)?);
        *offset += 2;
        let version = reader
            .constant_pool
            .get_optional_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        Ok(ModuleRequireEvent {
            module,
            access,
            version,
        })
    }
);

define_simple_iterator!(
    ModuleRelationReaderIterator,
    ModuleRelationEvent<'class>,
    |reader: &ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let package = reader
            .constant_pool
            .get_package(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let access = ModuleRelationAccess::from_bits_retain(reader.buffer.read_u16(*offset)?);
        *offset += 2;
        let module_count = reader.buffer.read_u16(*offset)?;
        *offset += 2;
        let mut modules = Vec::with_capacity(module_count as usize);
        for _ in 0..module_count {
            modules.push(
                reader
                    .constant_pool
                    .get_module(reader.buffer.read_u16(*offset)?)?,
            );
            *offset += 2;
        }
        Ok(ModuleRelationEvent {
            package,
            access,
            modules,
        })
    }
);

define_simple_iterator!(
    ModuleProvidesReaderIterator,
    ModuleProvidesEvent<'class>,
    |reader: &ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let service = reader
            .constant_pool
            .get_class(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let provider_count = reader.buffer.read_u16(*offset)?;
        *offset += 2;
        let mut providers = Vec::with_capacity(provider_count as usize);
        for _ in 0..provider_count {
            providers.push(
                reader
                    .constant_pool
                    .get_class(reader.buffer.read_u16(*offset)?)?,
            );
            *offset += 2;
        }
        Ok(ModuleProvidesEvent { service, providers })
    }
);

pub struct ModuleReaderEventProviders<'reader, 'class>(
    PhantomData<&'reader ()>,
    PhantomData<&'class ()>,
);

impl<'reader, 'class> ModuleEventProviders<'class> for ModuleReaderEventProviders<'reader, 'class>
where
    'class: 'reader,
{
    type Packages = PackagesReaderIterator<'reader, 'class>;
    type Requires = ModuleRequireReaderIterator<'reader, 'class>;
    type Exports = ModuleRelationReaderIterator<'reader, 'class>;
    type Opens = ModuleRelationReaderIterator<'reader, 'class>;
    type Uses = ClassesReaderIterator<'reader, 'class>;
    type Provides = ModuleProvidesReaderIterator<'reader, 'class>;
}

pub struct RecordComponentReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    custom_attributes_offsets: Vec<usize>,
}

impl<'reader, 'class> Iterator for RecordComponentReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<
        RecordComponentEvent<'class, RecordComponentReaderEventProviders<'reader, 'class>>,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct RecordComponentReaderEventProviders<'reader, 'class>(
    PhantomData<&'reader ()>,
    PhantomData<&'class ()>,
);

impl<'reader, 'class> RecordComponentEventProviders<'class>
    for RecordComponentReaderEventProviders<'reader, 'class>
where
    'class: 'reader,
{
    type Annotations = AnnotationReaderIterator<'reader, 'class>;

    type TypeAnnotations = TypeAnnotationReaderIterator<'reader, 'class>;

    type Attributes = CustomAttributeReaderIterator<'reader, 'class>;
}

pub struct CustomAttributeReaderIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    index: usize,
    offsets: Vec<usize>,
}

impl<'reader, 'class> CustomAttributeReaderIterator<'reader, 'class> {
    fn new(reader: &'reader ClassReader<'class>, offsets: Vec<usize>) -> Self {
        CustomAttributeReaderIterator {
            reader,
            index: 0,
            offsets,
        }
    }
}

impl Iterator for CustomAttributeReaderIterator<'_, '_> {
    type Item = ClassFileResult<Box<dyn Attribute>>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.offsets.len(), Some(self.offsets.len()))
    }
}

impl FusedIterator for CustomAttributeReaderIterator<'_, '_> {}

impl ExactSizeIterator for CustomAttributeReaderIterator<'_, '_> {}

define_simple_iterator!(
    StringsReaderIterator,
    Cow<'class, JavaStr>,
    |reader: &ClassReader<'class>, offset: &mut usize| {
        let result = reader
            .constant_pool
            .get_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        Ok(result)
    }
);

define_simple_iterator!(
    ClassesReaderIterator,
    Cow<'class, JavaStr>,
    |reader: &ClassReader<'class>, offset: &mut usize| {
        let result = reader
            .constant_pool
            .get_class(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        Ok(result)
    }
);

define_simple_iterator!(
    PackagesReaderIterator,
    Cow<'class, JavaStr>,
    |reader: &ClassReader<'class>, offset: &mut usize| {
        let result = reader
            .constant_pool
            .get_package(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        Ok(result)
    }
);

#[cfg(test)]
mod test {
    use crate::{ClassAccess, ClassReader, ClassReaderFlags};
    use classfile_macros::include_class;
    use std::borrow::Cow;

    const HELLO_WORLD_BYTECODE: &[u8] = include_class!("test/HelloWorld.java")[0];

    #[test]
    fn test_hello_world() {
        let reader = ClassReader::new(HELLO_WORLD_BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            ClassAccess::Public | ClassAccess::Super,
            reader.access().unwrap()
        );
        assert_eq!(Cow::Borrowed("HelloWorld"), reader.name().unwrap());
        assert_eq!(
            Cow::Borrowed("java/lang/Object"),
            reader.super_name().unwrap().unwrap()
        );
    }
}
