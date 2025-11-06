use crate::opcodes::InternalOpcodes;
use crate::tree::{AnnotationNode, AnnotationValue, TypeAnnotationNode};
use crate::{
    AnnotationEvent, Attribute, AttributeReader, BootstrapMethodArgument, ClassAccess,
    ClassClassEvent, ClassEvent, ClassEventProviders, ClassEventSource, ClassFieldEvent,
    ClassFileError, ClassFileResult, ClassInnerClassEvent, ClassMethodEvent, ClassModuleEvent,
    ClassOuterClassEvent, ClassRecordComponentEvent, ClassSourceEvent, ConstantDynamic,
    ConstantPool, ConstantPoolEntry, ConstantPoolTag, DynamicEntry, FieldAccess, FieldEvent,
    FieldEventProviders, FieldValue, Frame, FrameValue, Handle, HandleKind, InnerClassAccess,
    Label, LabelCreator, LdcConstant, MethodAccess, MethodAnnotableParameterCountEvent,
    MethodEvent, MethodEventProviders, MethodLocalVariableAnnotationEvent,
    MethodLocalVariableEvent, MethodMaxsEvent, MethodParameterAnnotationEvent,
    MethodParameterEvent, MethodTryCatchBlockAnnotationEvent, MethodTryCatchBlockEvent,
    ModuleAccess, ModuleEvent, ModuleEventProviders, ModuleProvidesEvent, ModuleRelationAccess,
    ModuleRelationEvent, ModuleRequireAccess, ModuleRequireEvent, NewArrayType, Opcode,
    ParameterAccess, RecordComponentEvent, RecordComponentEventProviders, TypePath, TypeReference,
    TypeReferenceTargetType, UnknownAttribute, LATEST_MAJOR_VERSION, MAX_ANNOTATION_NESTING,
};
use bitflags::{bitflags, Flags};
use derive_more::Debug;
use java_string::{JavaStr, JavaString};
use std::borrow::Cow;
use std::collections::HashMap;
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem;
use std::slice::SliceIndex;
use std::sync::{Arc, OnceLock};

macro_rules! define_simple_iterator {
    ($name:ident, $item_type:ty, $read_func:expr) => {
        #[derive(Debug)]
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

#[derive(Debug, Clone)]
pub struct ClassReader<'class> {
    buffer: ClassBuffer<'class>,
    pub constant_pool: ConstantPool<'class>,
    metadata_start: usize,
    reader_flags: ClassReaderFlags,
    #[debug("{:?}", attribute_readers.keys())]
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

    /// Returns the access flags of the class. For classes before Java 1.5, this value won't reflect
    /// the [`ClassAccess::Synthetic`] flag. If you need to support parsing these old classes and
    /// need to check for synthetic classes, use [`ClassReaderEvents::is_synthetic`] or check for
    /// [`ClassEvent::Synthetic`].
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

#[derive(Debug, Copy, Clone)]
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

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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

    pub fn slice<R>(&self, range: R) -> ClassFileResult<ClassBuffer<'class>>
    where
        R: SliceIndex<[u8], Output = [u8]>,
    {
        Ok(ClassBuffer {
            data: self.data.get(range).ok_or(ClassFileError::OutOfBounds {
                index: self.data.len(),
                len: self.data.len(),
            })?,
        })
    }
}

impl std::fmt::Debug for ClassBuffer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClassBuffer {{ {} bytes }}", self.data.len())
    }
}

impl<'reader, 'class> ClassEventSource<'class> for &'reader ClassReader<'class> {
    type Providers = ClassReaderEventProviders<'reader, 'class>;
    type Iterator = ClassReaderEvents<'reader, 'class>;

    fn events(self) -> ClassFileResult<Self::Iterator> {
        let access = self.access()?;
        let interfaces: ClassFileResult<Vec<_>> = self.interfaces()?.collect();
        let interfaces = interfaces?;
        let mut signature_offset = 0;
        let mut bootstrap_methods_offset = 0;
        let mut enclosing_method_offset = 0;
        let mut has_synthetic_attribute = false;
        let mut inner_classes_count = 0;
        let mut inner_classes_offset = 0;
        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut is_deprecated = false;
        let mut module_main_offset = 0;
        let mut module_offset = 0;
        let mut module_packages_offset = 0;
        let mut nest_host_offset = 0;
        let mut nest_members_count = 0;
        let mut nest_members_offset = 0;
        let mut permitted_subclasses_count = 0;
        let mut permitted_subclasses_offset = 0;
        let mut record_components_count = 0;
        let mut record_components_offset = 0;
        let mut source_debug_offset = 0;
        let mut source_offset = 0;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
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
                b"BootstrapMethods" => bootstrap_methods_offset = pos,
                b"Deprecated" => is_deprecated = true,
                b"EnclosingMethod" => enclosing_method_offset = pos,
                b"InnerClasses" => {
                    inner_classes_count = self.buffer.read_u16(pos)?;
                    inner_classes_offset = pos + 2;
                }
                b"Module" => module_offset = pos,
                b"ModuleMainClass" => module_main_offset = pos,
                b"ModulePackages" => module_packages_offset = pos,
                b"NestHost" => nest_host_offset = pos,
                b"NestMembers" => {
                    nest_members_count = self.buffer.read_u16(pos)?;
                    nest_members_offset = pos + 2;
                }
                b"PermittedSubclasses" => {
                    permitted_subclasses_count = self.buffer.read_u16(pos)?;
                    permitted_subclasses_offset = pos + 2;
                }
                b"Signature" => signature_offset = pos,
                b"SourceDebugExtension" => source_debug_offset = pos - 4,
                b"SourceFile" => source_offset = pos,
                b"Record" => {
                    record_components_count = self.buffer.read_u16(pos)?;
                    record_components_offset = pos + 2;
                }
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = self.buffer.read_u16(pos)?;
                    invisible_annotations_offset = pos + 2;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = self.buffer.read_u16(pos)?;
                    invisible_type_annotations_offset = pos + 2;
                }
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = self.buffer.read_u16(pos)?;
                    visible_annotations_offset = pos + 2;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = self.buffer.read_u16(pos)?;
                    visible_type_annotations_offset = pos + 2;
                }
                b"Synthetic" => has_synthetic_attribute = true,
                _ => custom_attributes_offsets.push(pos - 6),
            }

            pos += attribute_length as usize;
        }

        Ok(ClassReaderEvents {
            reader: self,
            access,
            interfaces,
            fields_count,
            fields_offset,
            methods_count,
            methods_offset,
            enclosing_method_offset,
            has_synthetic_attribute,
            inner_classes_count,
            inner_classes_offset,
            invisible_annotations_count,
            invisible_annotations_offset,
            invisible_type_annotations_count,
            invisible_type_annotations_offset,
            is_deprecated,
            module_main_offset,
            module_offset,
            module_packages_offset,
            nest_host_offset,
            nest_members_count,
            nest_members_offset,
            permitted_subclasses_count,
            permitted_subclasses_offset,
            record_components_count,
            record_components_offset,
            signature_offset,
            source_debug_offset,
            source_offset,
            visible_annotations_count,
            visible_annotations_offset,
            visible_type_annotations_count,
            visible_type_annotations_offset,
            custom_attributes_offsets,
            bootstrap_methods: BootstrapMethods {
                reader: self,
                bootstrap_methods_offset,
                cache: Default::default(),
            },
            state: 0,
        })
    }
}

#[derive(Debug)]
pub struct ClassReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    access: ClassAccess,
    interfaces: Vec<Cow<'class, JavaStr>>,
    fields_count: u16,
    fields_offset: usize,
    methods_count: u16,
    methods_offset: usize,
    enclosing_method_offset: usize,
    has_synthetic_attribute: bool,
    inner_classes_count: u16,
    inner_classes_offset: usize,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    is_deprecated: bool,
    module_main_offset: usize,
    module_offset: usize,
    module_packages_offset: usize,
    nest_host_offset: usize,
    nest_members_count: u16,
    nest_members_offset: usize,
    permitted_subclasses_count: u16,
    permitted_subclasses_offset: usize,
    record_components_count: u16,
    record_components_offset: usize,
    signature_offset: usize,
    source_debug_offset: usize,
    source_offset: usize,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    custom_attributes_offsets: Vec<usize>,
    bootstrap_methods: BootstrapMethods<'reader, 'class>,
    state: u8,
}

impl<'reader, 'class> ClassReaderEvents<'reader, 'class> {
    fn class_internal(&mut self) -> ClassFileResult<ClassClassEvent<'class>> {
        Ok(ClassClassEvent {
            major_version: self.reader.major_version(),
            minor_version: self.reader.minor_version(),
            access: self.access,
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

    pub fn is_deprecated(&self) -> bool {
        self.is_deprecated
    }

    pub fn is_synthetic(&self) -> bool {
        self.access.contains(ClassAccess::Synthetic) || self.has_synthetic_attribute
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
        ClassMethodsIterator::new(
            self.reader,
            self.methods_count,
            self.methods_offset,
            self.bootstrap_methods.clone(),
        )
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
                    if self.is_synthetic() {
                        return Some(Ok(ClassEvent::Synthetic));
                    }
                }
                2 => {
                    if self.is_deprecated {
                        return Some(Ok(ClassEvent::Deprecated));
                    }
                }
                3 => {
                    if let Some(source) = self.source().transpose() {
                        return Some(source.map(ClassEvent::Source));
                    }
                }
                4 => {
                    if let Some(module) = self.module().transpose() {
                        return Some(module.map(ClassEvent::Module));
                    }
                }
                5 => {
                    if let Some(nest_host) = self.nest_host().transpose() {
                        return Some(nest_host.map(ClassEvent::NestHost));
                    }
                }
                6 => {
                    if let Some(outer_class) = self.outer_class().transpose() {
                        return Some(outer_class.map(ClassEvent::OuterClass));
                    }
                }
                7 => {
                    if self.visible_annotations_offset != 0
                        || self.invisible_annotations_offset != 0
                    {
                        return Some(Ok(ClassEvent::Annotations(self.annotations())));
                    }
                }
                8 => {
                    if self.visible_type_annotations_offset != 0
                        || self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(ClassEvent::TypeAnnotations(self.type_annotations())));
                    }
                }
                9 => {
                    if !self.custom_attributes_offsets.is_empty() {
                        return Some(Ok(ClassEvent::Attributes(self.attributes())));
                    }
                }
                10 => {
                    if self.nest_members_offset != 0 {
                        return Some(Ok(ClassEvent::NestMembers(self.nest_members())));
                    }
                }
                11 => {
                    if self.permitted_subclasses_offset != 0 {
                        return Some(Ok(ClassEvent::PermittedSubclasses(
                            self.permitted_subclasses(),
                        )));
                    }
                }
                12 => {
                    if self.inner_classes_offset != 0 {
                        return Some(Ok(ClassEvent::InnerClasses(self.inner_classes())));
                    }
                }
                13 => {
                    if self.record_components_offset != 0 {
                        return Some(Ok(ClassEvent::Record(self.record_components())));
                    }
                }
                14 => {
                    if self.fields_count != 0 {
                        return Some(Ok(ClassEvent::Fields(self.fields())));
                    }
                }
                15 => {
                    if self.methods_count != 0 {
                        return Some(Ok(ClassEvent::Methods(self.methods())));
                    }
                }
                _ => return None,
            }
        }
    }
}

#[derive(Debug)]
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

#[derive(Clone)]
struct BootstrapMethods<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    bootstrap_methods_offset: usize,
    cache: Arc<OnceLock<ClassFileResult<Vec<BootstrapMethod<'class>>>>>,
}

impl<'reader, 'class> BootstrapMethods<'reader, 'class> {
    fn get(&self, index: u16) -> ClassFileResult<&BootstrapMethod<'class>> {
        let all = self.get_all()?;
        all.get(index as usize)
            .ok_or(ClassFileError::BootstrapMethodOutOfBounds {
                index,
                len: all.len() as u16,
            })
    }

    fn get_all(&self) -> ClassFileResult<&[BootstrapMethod<'class>]> {
        match self.cache.get_or_init(|| self.compute()) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.clone()),
        }
    }

    fn compute(&self) -> ClassFileResult<Vec<BootstrapMethod<'class>>> {
        enum UnresolvedBsmArg<'class> {
            Integer(i32),
            Float(f32),
            Long(i64),
            Double(f64),
            String(Cow<'class, JavaStr>),
            Class(Cow<'class, JavaStr>),
            Handle(Handle<'class>),
            ConstantDynamic(DynamicEntry<'class>),
        }

        struct UnresolvedBsm<'class> {
            handle: Handle<'class>,
            args: Vec<UnresolvedBsmArg<'class>>,
        }

        #[derive(Copy, Clone, PartialEq)]
        enum ResolvedState {
            Unresolved,
            Resolving,
            Resolved,
        }

        if self.bootstrap_methods_offset == 0 {
            return Ok(Vec::new());
        }

        let mut offset = self.bootstrap_methods_offset;

        let bsm_count = self.reader.buffer.read_u16(offset)?;
        offset += 2;

        let mut unresolved_bsms = Vec::with_capacity(bsm_count as usize);
        for _ in 0..bsm_count {
            let handle = self
                .reader
                .constant_pool
                .get_method_handle(self.reader.buffer.read_u16(offset)?)?;
            offset += 2;
            let arg_count = self.reader.buffer.read_u16(offset)?;
            offset += 2;
            let mut args = Vec::with_capacity(arg_count as usize);
            for _ in 0..arg_count {
                let cp_index = self.reader.buffer.read_u16(offset)?;
                let arg = match self.reader.constant_pool.get(cp_index)? {
                    ConstantPoolEntry::Integer(i) => UnresolvedBsmArg::Integer(i),
                    ConstantPoolEntry::Float(f) => UnresolvedBsmArg::Float(f),
                    ConstantPoolEntry::Long(l) => UnresolvedBsmArg::Long(l),
                    ConstantPoolEntry::Double(d) => UnresolvedBsmArg::Double(d),
                    ConstantPoolEntry::String(s) => UnresolvedBsmArg::String(s),
                    ConstantPoolEntry::Class(c) => UnresolvedBsmArg::Class(c),
                    ConstantPoolEntry::MethodHandle(h) => UnresolvedBsmArg::Handle(h),
                    ConstantPoolEntry::Dynamic(d) => UnresolvedBsmArg::ConstantDynamic(d),
                    _ => {
                        return Err(
                            ClassFileError::BadConstantPoolTypeExpectedBootstrapMethodArgument(
                                self.reader.constant_pool.get_type(cp_index)?,
                            ),
                        )
                    }
                };
                offset += 2;
                args.push(arg);
            }

            unresolved_bsms.push(UnresolvedBsm { handle, args });
        }

        let mut resolved_states = vec![ResolvedState::Unresolved; bsm_count as usize];
        // create resolved bsms list pre-filled with dummy values
        let mut resolved_bsms: Vec<_> = (0..bsm_count)
            .map(|_| BootstrapMethod {
                handle: Handle {
                    kind: HandleKind::GetField,
                    owner: Default::default(),
                    name: Default::default(),
                    desc: Default::default(),
                    is_interface: false,
                },
                args: Vec::new(),
            })
            .collect();

        fn resolve<'class>(
            i: usize,
            unresolved_bsms: &[UnresolvedBsm<'class>],
            resolved_states: &mut [ResolvedState],
            resolved_bsms: &mut [BootstrapMethod<'class>],
        ) -> ClassFileResult<()> {
            if resolved_states[i] == ResolvedState::Resolved {
                return Ok(());
            }

            if resolved_states[i] == ResolvedState::Resolving {
                return Err(ClassFileError::BootstrapMethodCircularDependency);
            }

            resolved_states[i] = ResolvedState::Resolving;

            let unresolved = &unresolved_bsms[i];
            let mut resolved_args = unresolved
                .args
                .iter()
                .map(|unresolved_arg| -> ClassFileResult<_> {
                    Ok(match unresolved_arg {
                        UnresolvedBsmArg::Integer(i) => BootstrapMethodArgument::Integer(*i),
                        UnresolvedBsmArg::Float(f) => BootstrapMethodArgument::Float(*f),
                        UnresolvedBsmArg::Long(l) => BootstrapMethodArgument::Long(*l),
                        UnresolvedBsmArg::Double(d) => BootstrapMethodArgument::Double(*d),
                        UnresolvedBsmArg::String(s) => BootstrapMethodArgument::String(s.clone()),
                        UnresolvedBsmArg::Class(c) => BootstrapMethodArgument::Class(c.clone()),
                        UnresolvedBsmArg::Handle(h) => BootstrapMethodArgument::Handle(h.clone()),
                        UnresolvedBsmArg::ConstantDynamic(d) => {
                            if d.bootstrap_method_attr_index as usize >= unresolved_bsms.len() {
                                return Err(ClassFileError::BootstrapMethodOutOfBounds {
                                    index: d.bootstrap_method_attr_index,
                                    len: unresolved_bsms.len() as u16,
                                });
                            }
                            resolve(
                                d.bootstrap_method_attr_index as usize,
                                unresolved_bsms,
                                resolved_states,
                                resolved_bsms,
                            )?;
                            let resolved =
                                resolved_bsms[d.bootstrap_method_attr_index as usize].clone();
                            BootstrapMethodArgument::ConstantDynamic(ConstantDynamic {
                                name: d.name.clone(),
                                desc: d.desc.clone(),
                                bootstrap_method: resolved.handle,
                                bootstrap_method_arguments: resolved.args,
                            })
                        }
                    })
                })
                .collect::<ClassFileResult<Vec<_>>>()?;

            resolved_bsms[i] = BootstrapMethod {
                handle: unresolved.handle.clone(),
                args: resolved_args,
            };

            resolved_states[i] = ResolvedState::Resolved;
            Ok(())
        }

        for i in 0..bsm_count as usize {
            resolve(
                i,
                &unresolved_bsms,
                &mut resolved_states,
                &mut resolved_bsms,
            )?;
        }

        Ok(resolved_bsms)
    }
}

impl std::fmt::Debug for BootstrapMethods<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.get_all(), f)
    }
}

#[derive(Debug, Clone)]
struct BootstrapMethod<'class> {
    handle: Handle<'class>,
    args: Vec<BootstrapMethodArgument<'class>>,
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
            .get_optional_utf8(reader.buffer.read_u16(*offset)?)?;
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

        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut signature = None;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
        let mut custom_attributes_offsets = Vec::new();

        for _ in 0..attribute_count {
            let attribute_name = reader
                .constant_pool
                .get_utf8_as_bytes(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            let attribute_length = reader.buffer.read_u32(*offset)?;
            *offset += 4;

            match attribute_name {
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_annotations_offset = *offset + 2;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_type_annotations_offset = *offset + 2;
                }
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_annotations_offset = *offset + 2;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_type_annotations_offset = *offset + 2;
                }
                b"Signature" => {
                    signature = Some(
                        reader
                            .constant_pool
                            .get_utf8(reader.buffer.read_u16(*offset)?)?,
                    )
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
                invisible_annotations_count,
                invisible_annotations_offset,
                invisible_type_annotations_count,
                invisible_type_annotations_offset,
                visible_annotations_count,
                visible_annotations_offset,
                visible_type_annotations_count,
                visible_type_annotations_offset,
                custom_attributes_offsets,
                state: 0,
            },
        })
    }
);

define_simple_iterator!(
    ClassFieldsIterator,
    ClassFieldEvent<'class, FieldReaderEvents<'reader, 'class>>,
    |reader: &'reader ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let mut access = FieldAccess::from_bits_retain(reader.buffer.read_u16(*offset)?);
        *offset += 2;
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

        let mut constant_value = None;
        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut is_deprecated = false;
        let mut signature = None;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
        let mut custom_attributes_offsets = Vec::new();

        for _ in 0..attribute_count {
            let attribute_name = reader
                .constant_pool
                .get_utf8_as_bytes(reader.buffer.read_u16(*offset)?)?;
            *offset += 2;
            let attribute_length = reader.buffer.read_u32(*offset)?;
            *offset += 4;

            match attribute_name {
                b"ConstantValue" => {
                    let cp_index = reader.buffer.read_u16(*offset)?;
                    let constant = match reader.constant_pool.get(cp_index)? {
                        ConstantPoolEntry::Integer(i) => FieldValue::Integer(i),
                        ConstantPoolEntry::Float(f) => FieldValue::Float(f),
                        ConstantPoolEntry::Long(l) => FieldValue::Long(l),
                        ConstantPoolEntry::Double(d) => FieldValue::Double(d),
                        ConstantPoolEntry::String(s) => FieldValue::String(s),
                        _ => {
                            return Err(
                                ClassFileError::BadConstantPoolTypeExpectedFieldConstantValue(
                                    reader.constant_pool.get_type(cp_index)?,
                                ),
                            )
                        }
                    };
                    constant_value = Some(constant);
                }
                b"Deprecated" => is_deprecated = true,
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_annotations_offset = *offset + 2;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    invisible_type_annotations_offset = *offset + 2;
                }
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_annotations_offset = *offset + 2;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = reader.buffer.read_u16(*offset)?;
                    visible_type_annotations_offset = *offset + 2;
                }
                b"Signature" => {
                    signature = Some(
                        reader
                            .constant_pool
                            .get_utf8(reader.buffer.read_u16(*offset)?)?,
                    )
                }
                b"Synthetic" => access.insert(FieldAccess::Synthetic),
                _ => custom_attributes_offsets.push(*offset - 6),
            }

            *offset += attribute_length as usize;
        }

        Ok(ClassFieldEvent {
            access,
            name,
            desc,
            signature,
            value: constant_value,
            events: FieldReaderEvents {
                reader,
                invisible_annotations_count,
                invisible_annotations_offset,
                invisible_type_annotations_count,
                invisible_type_annotations_offset,
                is_deprecated,
                visible_annotations_count,
                visible_annotations_offset,
                visible_type_annotations_count,
                visible_type_annotations_offset,
                custom_attributes_offsets,
                state: 0,
            },
        })
    }
);

#[derive(Debug)]
pub struct ClassMethodsIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    count: u16,
    remaining: u16,
    offset: usize,
    bootstrap_methods: BootstrapMethods<'reader, 'class>,
}
impl<'reader, 'class> ClassMethodsIterator<'reader, 'class> {
    fn new(
        reader: &'reader ClassReader<'class>,
        count: u16,
        offset: usize,
        bootstrap_methods: BootstrapMethods<'reader, 'class>,
    ) -> Self {
        ClassMethodsIterator {
            reader,
            count,
            remaining: count,
            offset,
            bootstrap_methods,
        }
    }

    fn event(
        &mut self,
    ) -> ClassFileResult<ClassMethodEvent<'class, MethodReaderEvents<'reader, 'class>>> {
        let mut access = MethodAccess::from_bits_retain(self.reader.buffer.read_u16(self.offset)?);
        self.offset += 2;
        let name = self
            .reader
            .constant_pool
            .get_utf8(self.reader.buffer.read_u16(self.offset)?)?;
        self.offset += 2;
        let desc = self
            .reader
            .constant_pool
            .get_utf8(self.reader.buffer.read_u16(self.offset)?)?;
        self.offset += 2;
        let attribute_count = self.reader.buffer.read_u16(self.offset)?;
        self.offset += 2;
        let mut annotation_default_offset = 0;
        let mut code_offset = 0;
        let mut exceptions = Vec::new();
        let mut invisible_annotations_count = 0;
        let mut invisible_annotations_offset = 0;
        let mut invisible_parameter_annotations_offset = 0;
        let mut invisible_type_annotations_count = 0;
        let mut invisible_type_annotations_offset = 0;
        let mut is_deprecated = false;
        let mut parameters_count = 0;
        let mut parameters_offset = 0;
        let mut signature = None;
        let mut visible_annotations_count = 0;
        let mut visible_annotations_offset = 0;
        let mut visible_parameter_annotations_offset = 0;
        let mut visible_type_annotations_count = 0;
        let mut visible_type_annotations_offset = 0;
        let mut custom_attribute_offsets = Vec::new();
        for _ in 0..attribute_count {
            let attribute_name = self
                .reader
                .constant_pool
                .get_utf8_as_bytes(self.reader.buffer.read_u16(self.offset)?)?;
            self.offset += 2;
            let attribute_length = self.reader.buffer.read_u32(self.offset)?;
            self.offset += 4;
            match attribute_name {
                b"AnnotationDefault" => annotation_default_offset = self.offset,
                b"Code" => {
                    if !self
                        .reader
                        .reader_flags
                        .contains(ClassReaderFlags::SkipCode)
                    {
                        code_offset = self.offset;
                    }
                }
                b"Deprecated" => is_deprecated = true,
                b"Exceptions" => {
                    let exception_count = self.reader.buffer.read_u16(self.offset)?;
                    exceptions.reserve(exception_count as usize);
                    for i in 0..exception_count {
                        exceptions.push(
                            self.reader.constant_pool.get_class(
                                self.reader
                                    .buffer
                                    .read_u16(self.offset + 2 + 2 * i as usize)?,
                            )?,
                        );
                    }
                }
                b"MethodParameters" => {
                    if !self
                        .reader
                        .reader_flags
                        .contains(ClassReaderFlags::SkipDebug)
                    {
                        parameters_count = self.reader.buffer.read_u16(self.offset)?;
                        parameters_offset = self.offset + 2;
                    }
                }
                b"RuntimeInvisibleAnnotations" => {
                    invisible_annotations_count = self.reader.buffer.read_u16(self.offset)?;
                    invisible_annotations_offset = self.offset + 2;
                }
                b"RuntimeInvisibleParameterAnnotations" => {
                    invisible_parameter_annotations_offset = self.offset;
                }
                b"RuntimeInvisibleTypeAnnotations" => {
                    invisible_type_annotations_count = self.reader.buffer.read_u16(self.offset)?;
                    invisible_type_annotations_offset = self.offset + 2;
                }
                b"RuntimeVisibleAnnotations" => {
                    visible_annotations_count = self.reader.buffer.read_u16(self.offset)?;
                    visible_annotations_offset = self.offset + 2;
                }
                b"RuntimeVisibleParameterAnnotations" => {
                    visible_parameter_annotations_offset = self.offset;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    visible_type_annotations_count = self.reader.buffer.read_u16(self.offset)?;
                    visible_type_annotations_offset = self.offset + 2;
                }
                b"Signature" => {
                    signature = Some(
                        self.reader
                            .constant_pool
                            .get_utf8(self.reader.buffer.read_u16(self.offset)?)?,
                    );
                }
                b"Synthetic" => access.insert(MethodAccess::Synthetic),
                _ => custom_attribute_offsets.push(self.offset - 6),
            }
            self.offset += attribute_length as usize;
        }
        Ok(ClassMethodEvent {
            access,
            name,
            desc,
            signature,
            exceptions,
            events: MethodReaderEvents {
                reader: self.reader,
                annotation_default_offset,
                code_offset,
                invisible_annotations_count,
                invisible_annotations_offset,
                invisible_parameter_annotations_offset,
                invisible_type_annotations_count,
                invisible_type_annotations_offset,
                is_deprecated,
                parameters_count,
                parameters_offset,
                visible_annotations_count,
                visible_annotations_offset,
                visible_parameter_annotations_offset,
                visible_type_annotations_count,
                visible_type_annotations_offset,
                custom_attribute_offsets,
                code_data: None,
                bootstrap_methods: self.bootstrap_methods.clone(),
                state: 0,
                code_index: 0,
            },
        })
    }
}

impl<'reader, 'class> Iterator for ClassMethodsIterator<'reader, 'class> {
    type Item = ClassFileResult<ClassMethodEvent<'class, MethodReaderEvents<'reader, 'class>>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(self.event())
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count as usize, Some(self.count as usize))
    }
}

impl FusedIterator for ClassMethodsIterator<'_, '_> {}

impl ExactSizeIterator for ClassMethodsIterator<'_, '_> {}

#[derive(Debug)]
pub struct FieldReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    is_deprecated: bool,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    custom_attributes_offsets: Vec<usize>,
    state: u8,
}

impl<'reader, 'class> FieldReaderEvents<'reader, 'class> {
    pub fn is_deprecated(&self) -> bool {
        self.is_deprecated
    }

    pub fn annotations(&self) -> AnnotationReaderIterator<'reader, 'class> {
        AnnotationReaderIterator::new(
            self.reader,
            self.visible_annotations_count,
            self.visible_annotations_offset,
            self.invisible_annotations_count,
            self.invisible_annotations_offset,
        )
    }

    pub fn type_annotations(&self) -> TypeAnnotationReaderIterator<'reader, 'class> {
        TypeAnnotationReaderIterator::new(
            self.reader,
            self.visible_type_annotations_count,
            self.visible_type_annotations_offset,
            self.invisible_type_annotations_count,
            self.invisible_type_annotations_offset,
        )
    }

    pub fn attributes(&self) -> CustomAttributeReaderIterator<'reader, 'class> {
        CustomAttributeReaderIterator::new(self.reader, self.custom_attributes_offsets.clone())
    }
}

impl<'reader, 'class> Iterator for FieldReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<FieldEvent<'class, FieldReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let state = self.state;
            self.state += 1;
            match state {
                0 => {
                    if self.is_deprecated {
                        return Some(Ok(FieldEvent::Deprecated));
                    }
                }
                1 => {
                    if self.visible_annotations_offset != 0
                        && self.invisible_annotations_offset != 0
                    {
                        return Some(Ok(FieldEvent::Annotations(self.annotations())));
                    }
                }
                2 => {
                    if self.visible_type_annotations_offset != 0
                        && self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(FieldEvent::TypeAnnotations(self.type_annotations())));
                    }
                }
                3 => {
                    if !self.custom_attributes_offsets.is_empty() {
                        return Some(Ok(FieldEvent::Attributes(self.attributes())));
                    }
                }
                _ => return None,
            }
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct MethodReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    annotation_default_offset: usize,
    code_offset: usize,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    invisible_parameter_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    is_deprecated: bool,
    parameters_count: u16,
    parameters_offset: usize,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    visible_parameter_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    custom_attribute_offsets: Vec<usize>,
    code_data: Option<CodeData<'reader, 'class>>,
    bootstrap_methods: BootstrapMethods<'reader, 'class>,
    state: u8,
    code_index: u16,
}

impl<'reader, 'class> MethodReaderEvents<'reader, 'class> {
    pub fn is_deprecated(&self) -> bool {
        self.is_deprecated
    }

    pub fn parameters(&self) -> MethodParameterReaderIterator<'reader, 'class> {
        MethodParameterReaderIterator::new(
            self.reader,
            self.parameters_count,
            self.parameters_offset,
        )
    }

    pub fn annotation_default(&self) -> ClassFileResult<Option<AnnotationValue<'class>>> {
        if self.annotation_default_offset == 0 {
            return Ok(None);
        }

        let mut offset = self.annotation_default_offset;
        read_annotation_value(self.reader, &mut offset, 0).map(Some)
    }

    pub fn annotations(&self) -> AnnotationReaderIterator<'reader, 'class> {
        AnnotationReaderIterator::new(
            self.reader,
            self.visible_annotations_count,
            self.visible_annotations_offset,
            self.invisible_annotations_count,
            self.invisible_annotations_offset,
        )
    }

    pub fn type_annotations(&self) -> TypeAnnotationReaderIterator<'reader, 'class> {
        TypeAnnotationReaderIterator::new(
            self.reader,
            self.visible_type_annotations_count,
            self.visible_type_annotations_offset,
            self.invisible_type_annotations_count,
            self.invisible_type_annotations_offset,
        )
    }

    pub fn parameter_annotations(
        &self,
    ) -> MethodParameterAnnotationsReaderIterator<'reader, 'class> {
        MethodParameterAnnotationsReaderIterator::new(
            self.reader,
            self.visible_parameter_annotations_offset,
            self.invisible_parameter_annotations_offset,
        )
    }

    pub fn attributes(&self) -> CustomAttributeReaderIterator<'reader, 'class> {
        CustomAttributeReaderIterator::new(self.reader, self.custom_attribute_offsets.clone())
    }

    pub fn has_code(&self) -> bool {
        self.code_offset != 0
    }
}

impl<'reader, 'class> Iterator for MethodReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<MethodEvent<'class, MethodReaderEventProviders<'reader, 'class>>>;

    fn next(&mut self) -> Option<Self::Item> {
        const START_INSNS_STATE: u8 = 10;
        const END_INSNS_STATE: u8 = 16;
        const MAX_STATE: u8 = 22;

        loop {
            let state = self.state;
            self.state += 1;

            match state {
                0 => {
                    if self.is_deprecated {
                        return Some(Ok(MethodEvent::Deprecated));
                    }
                }
                1 => {
                    if self.parameters_offset != 0 {
                        return Some(Ok(MethodEvent::Parameters(self.parameters())));
                    }
                }
                2 => {
                    if let Some(annotation_default) = self.annotation_default().transpose() {
                        return Some(annotation_default.map(MethodEvent::AnnotationDefault));
                    }
                }
                3 => {
                    if self.visible_annotations_offset != 0
                        || self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(MethodEvent::Annotations(self.annotations())));
                    }
                }
                4 => {
                    if self.visible_type_annotations_offset != 0
                        || self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(MethodEvent::TypeAnnotations(self.type_annotations())));
                    }
                }
                5 => {
                    if self.visible_parameter_annotations_offset != 0 {
                        return Some(
                            self.reader
                                .buffer
                                .read_u8(self.visible_parameter_annotations_offset)
                                .map(|count| {
                                    MethodEvent::AnnotableParameterCount(
                                        MethodAnnotableParameterCountEvent {
                                            count,
                                            visible: true,
                                        },
                                    )
                                }),
                        );
                    }
                }
                6 => {
                    if self.invisible_parameter_annotations_offset != 0 {
                        return Some(
                            self.reader
                                .buffer
                                .read_u8(self.invisible_parameter_annotations_offset)
                                .map(|count| {
                                    MethodEvent::AnnotableParameterCount(
                                        MethodAnnotableParameterCountEvent {
                                            count,
                                            visible: false,
                                        },
                                    )
                                }),
                        );
                    }
                }
                7 => {
                    if self.visible_parameter_annotations_offset != 0
                        || self.invisible_parameter_annotations_offset != 0
                    {
                        return Some(Ok(MethodEvent::ParameterAnnotations(
                            self.parameter_annotations(),
                        )));
                    }
                }
                8 => {
                    if !self.custom_attribute_offsets.is_empty() {
                        return Some(Ok(MethodEvent::Attributes(self.attributes())));
                    }
                }
                9 => {
                    if self.code_offset == 0 {
                        self.state = MAX_STATE;
                        return None;
                    }

                    let code_data = match CodeData::read(
                        self.reader,
                        self.code_offset,
                        &self.bootstrap_methods,
                    ) {
                        Ok(code_data) => code_data,
                        Err(err) => return Some(Err(err)),
                    };

                    let label_creator = code_data.label_creator.clone();
                    self.code_data = Some(code_data);
                    return Some(Ok(MethodEvent::Code { label_creator }));
                }
                START_INSNS_STATE => {
                    let code_data = self
                        .code_data
                        .as_ref()
                        .expect("should not reach this state with no code data");

                    if self.code_index as usize >= code_data.insn_metadata.len() {
                        self.state = END_INSNS_STATE;
                        continue;
                    }

                    if let Some(label) = code_data.insn_metadata[self.code_index as usize].label {
                        return Some(Ok(MethodEvent::Label(label)));
                    }
                }
                11 => {
                    let code_data = self
                        .code_data
                        .as_ref()
                        .expect("should not reach this state with no code data");

                    if let Some(line_number) =
                        code_data.insn_metadata[self.code_index as usize].line_number
                    {
                        return Some(Ok(MethodEvent::LineNumber {
                            line: line_number,
                            start: code_data.insn_metadata[self.code_index as usize]
                                .label
                                .expect("line number should have label"),
                        }));
                    }
                }
                12 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");

                    if let Some(frame) = code_data.insn_metadata[self.code_index as usize]
                        .frame
                        .take()
                    {
                        return Some(Ok(MethodEvent::Frame(frame)));
                    }
                }
                13 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");

                    if let Some(insn_event) = code_data.insn_metadata[self.code_index as usize]
                        .insn_event
                        .take()
                    {
                        return Some(Ok(insn_event));
                    }
                }
                14 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    let insn_metadata = &mut code_data.insn_metadata[self.code_index as usize];
                    if !insn_metadata.annotations.is_empty() {
                        return Some(Ok(MethodEvent::InsnAnnotations(
                            WrapWithResultReaderIterator::new(
                                mem::take(&mut insn_metadata.annotations).into_iter(),
                            ),
                        )));
                    }
                }
                15 => {
                    self.code_index += 1;
                    self.state = START_INSNS_STATE;
                    continue;
                }
                END_INSNS_STATE => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    if !code_data.lvt.is_empty() {
                        return Some(Ok(MethodEvent::LocalVariables(
                            WrapWithResultReaderIterator::new(
                                mem::take(&mut code_data.lvt).into_iter(),
                            ),
                        )));
                    }
                }
                17 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    if !code_data.local_variable_annotations.is_empty() {
                        return Some(Ok(MethodEvent::LocalVariableAnnotations(
                            WrapWithResultReaderIterator::new(
                                mem::take(&mut code_data.local_variable_annotations).into_iter(),
                            ),
                        )));
                    }
                }
                18 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    if !code_data.try_catch_blocks.is_empty() {
                        return Some(Ok(MethodEvent::TryCatchBlocks(
                            WrapWithResultReaderIterator::new(
                                mem::take(&mut code_data.try_catch_blocks).into_iter(),
                            ),
                        )));
                    }
                }
                19 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    if !code_data.try_catch_block_annotations.is_empty() {
                        return Some(Ok(MethodEvent::TryCatchBlockAnnotations(
                            WrapWithResultReaderIterator::new(
                                mem::take(&mut code_data.try_catch_block_annotations).into_iter(),
                            ),
                        )));
                    }
                }
                20 => {
                    let code_data = self
                        .code_data
                        .as_mut()
                        .expect("should not reach this state with no code data");
                    if !code_data.custom_attribute_offsets.is_empty() {
                        return Some(Ok(MethodEvent::CodeAttributes(
                            CustomAttributeReaderIterator::new(
                                self.reader,
                                mem::take(&mut code_data.custom_attribute_offsets),
                            ),
                        )));
                    }
                }
                21 => {
                    let code_data = self
                        .code_data
                        .as_ref()
                        .expect("should not reach this state with no code data");
                    return Some(Ok(MethodEvent::Maxs(MethodMaxsEvent {
                        max_locals: code_data.max_locals,
                        max_stack: code_data.max_stack,
                    })));
                }
                MAX_STATE => return None,
                _ => return None,
            }
        }
    }
}

#[derive(Debug)]
struct CodeData<'reader, 'class> {
    max_stack: u16,
    max_locals: u16,
    label_creator: LabelCreator,
    insn_metadata: Box<[InstructionMetadata<'reader, 'class>]>,
    try_catch_blocks: Vec<MethodTryCatchBlockEvent<'class>>,
    try_catch_block_annotations: Vec<MethodTryCatchBlockAnnotationEvent<'class>>,
    lvt: Vec<MethodLocalVariableEvent<'class>>,
    local_variable_annotations: Vec<MethodLocalVariableAnnotationEvent<'class>>,
    custom_attribute_offsets: Vec<usize>,
}

impl<'reader, 'class> CodeData<'reader, 'class> {
    fn read(
        reader: &'reader ClassReader<'class>,
        mut offset: usize,
        bootstrap_methods: &BootstrapMethods<'reader, 'class>,
    ) -> ClassFileResult<CodeData<'reader, 'class>> {
        let max_stack = reader.buffer.read_u16(offset)?;
        offset += 2;
        let max_locals = reader.buffer.read_u16(offset)?;
        offset += 2;

        let label_creator = LabelCreator::default();

        let code_length = reader.buffer.read_u32(offset)?;
        offset += 4;
        if code_length == 0 || code_length > 65535 {
            return Err(ClassFileError::BadCodeSize(code_length));
        }

        let code = reader.buffer.read_bytes(offset, code_length as usize)?;
        offset += code_length as usize;

        let mut insn_metadata = (0..=code_length)
            .map(|_| InstructionMetadata::default())
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self::read_code(
            reader,
            code,
            bootstrap_methods,
            &mut insn_metadata,
            &label_creator,
        )?;

        let try_catch_block_count = reader.buffer.read_u16(offset)?;
        offset += 2;
        let mut try_catch_blocks = Vec::with_capacity(try_catch_block_count as usize);

        for _ in 0..try_catch_block_count {
            let start_pc = reader.buffer.read_u16(offset)?;
            offset += 2;
            let end_pc = reader.buffer.read_u16(offset)?;
            offset += 2;
            let handler_pc = reader.buffer.read_u16(offset)?;
            offset += 2;
            let ty = reader
                .constant_pool
                .get_optional_class(reader.buffer.read_u16(offset)?)?;
            offset += 2;

            let start = insn_metadata
                .get_code_mut(start_pc as usize)?
                .get_or_create_label(&label_creator);
            let end = insn_metadata
                .get_code_mut(end_pc as usize)?
                .get_or_create_label(&label_creator);
            let handler = insn_metadata
                .get_code_mut(handler_pc as usize)?
                .get_or_create_label(&label_creator);

            try_catch_blocks.push(MethodTryCatchBlockEvent {
                start,
                end,
                handler,
                ty,
            })
        }

        let attribute_count = reader.buffer.read_u16(offset)?;
        offset += 2;

        let mut local_variable_annotations = Vec::new();
        let mut lvt = Vec::new();
        let mut lvtt_offsets = Vec::new();
        let mut stack_map_compressed = true;
        let mut stack_map_table_offset = 0;
        let mut try_catch_block_annotations = Vec::new();
        let mut custom_attribute_offsets = Vec::new();

        for _ in 0..attribute_count {
            let attribute_name = reader
                .constant_pool
                .get_utf8_as_bytes(reader.buffer.read_u16(offset)?)?;
            offset += 2;
            let attribute_length = reader.buffer.read_u32(offset)?;
            offset += 4;

            match attribute_name {
                b"LineNumberTable" => {
                    if !reader.reader_flags.contains(ClassReaderFlags::SkipDebug) {
                        let line_numbers_count = reader.buffer.read_u16(offset)?;
                        for i in 0..line_numbers_count {
                            let start_pc = reader.buffer.read_u16(offset + 2 + 4 * i as usize)?;
                            let line_number =
                                reader.buffer.read_u16(offset + 4 + 4 * i as usize)?;
                            let metadata = insn_metadata.get_code_mut(start_pc as usize)?;
                            metadata.get_or_create_label(&label_creator);
                            metadata.line_number = Some(line_number);
                        }
                    }
                }
                b"LocalVariableTable" => {
                    if !reader.reader_flags.contains(ClassReaderFlags::SkipDebug) {
                        let local_variables_count = reader.buffer.read_u16(offset)?;
                        lvt.reserve(local_variables_count as usize);
                        for i in 0..local_variables_count {
                            let start_pc = reader.buffer.read_u16(offset + 2 + 10 * i as usize)?;
                            let length = reader.buffer.read_u16(offset + 4 + 10 * i as usize)?;
                            let name = reader
                                .constant_pool
                                .get_utf8(reader.buffer.read_u16(offset + 6 + 10 * i as usize)?)?;
                            let desc = reader
                                .constant_pool
                                .get_utf8(reader.buffer.read_u16(offset + 8 + 10 * i as usize)?)?;
                            let index = reader.buffer.read_u16(offset + 10 + 10 * i as usize)?;

                            let start = insn_metadata
                                .get_code_mut(start_pc as usize)?
                                .get_or_create_label(&label_creator);
                            let end = insn_metadata
                                .get_code_mut(start_pc as usize + length as usize)?
                                .get_or_create_label(&label_creator);

                            lvt.push(MethodLocalVariableEvent {
                                start,
                                end,
                                name,
                                desc,
                                signature: None,
                                index,
                            })
                        }
                    }
                }
                b"LocalVariableTypeTable" => lvtt_offsets.push(offset),
                b"StackMap" => {
                    stack_map_table_offset = offset;
                    stack_map_compressed = false;
                }
                b"StackMapTable" => stack_map_table_offset = offset,
                b"RuntimeInvisibleTypeAnnotations" => {
                    Self::read_code_annotations(
                        reader,
                        offset,
                        false,
                        &mut local_variable_annotations,
                        &mut try_catch_block_annotations,
                        &mut insn_metadata,
                        &label_creator,
                    )?;
                }
                b"RuntimeVisibleTypeAnnotations" => {
                    Self::read_code_annotations(
                        reader,
                        offset,
                        true,
                        &mut local_variable_annotations,
                        &mut try_catch_block_annotations,
                        &mut insn_metadata,
                        &label_creator,
                    )?;
                }
                _ => custom_attribute_offsets.push(offset - 6),
            }

            offset += attribute_length as usize;
        }

        if !reader.reader_flags.contains(ClassReaderFlags::SkipDebug) {
            for &lvtt_offset in &lvtt_offsets {
                let count = reader.buffer.read_u16(lvtt_offset)?;
                for i in 0..count {
                    let start_pc = reader.buffer.read_u16(lvtt_offset + 2 + 10 * i as usize)?;
                    let signature = reader
                        .constant_pool
                        .get_utf8(reader.buffer.read_u16(lvtt_offset + 8 + 10 * i as usize)?)?;
                    let index = reader.buffer.read_u16(lvtt_offset + 10 + 10 * i as usize)?;

                    if let Some(start) = insn_metadata.get_code_ref(start_pc as usize)?.label {
                        if let Some(lvt_entry) = lvt
                            .iter_mut()
                            .find(|lvt_entry| lvt_entry.start == start && lvt_entry.index == index)
                        {
                            lvt_entry.signature = Some(signature);
                        }
                    }
                }
            }
        }

        if !reader.reader_flags.contains(ClassReaderFlags::SkipFrames)
            && stack_map_table_offset != 0
        {
            Self::read_frames(
                reader,
                stack_map_table_offset,
                stack_map_compressed,
                &mut insn_metadata,
                &label_creator,
            )?;
        }

        Ok(CodeData {
            max_stack,
            max_locals,
            label_creator,
            insn_metadata,
            try_catch_blocks,
            try_catch_block_annotations,
            lvt,
            local_variable_annotations,
            custom_attribute_offsets,
        })
    }

    fn read_code(
        reader: &'reader ClassReader<'class>,
        code: &[u8],
        bootstrap_methods: &BootstrapMethods<'reader, 'class>,
        insn_metadata: &mut [InstructionMetadata<'reader, 'class>],
        label_creator: &LabelCreator,
    ) -> ClassFileResult<()> {
        let mut i = 0;
        while i < code.len() {
            let insn_base = i;
            let opcode = code[i];
            let insn = match opcode {
                InternalOpcodes::LDC_W | InternalOpcodes::LDC2_W => {
                    let cst_index =
                        u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                    i += 3;
                    MethodEvent::LdcInsn(Self::get_ldc_constant(
                        reader,
                        cst_index,
                        bootstrap_methods,
                    )?)
                }
                InternalOpcodes::ILOAD_0..=InternalOpcodes::ILOAD_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::ILoad,
                        var_index: (opcode - InternalOpcodes::ILOAD_0) as u16,
                    }
                }
                InternalOpcodes::LLOAD_0..=InternalOpcodes::LLOAD_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::LLoad,
                        var_index: (opcode - InternalOpcodes::LLOAD_0) as u16,
                    }
                }
                InternalOpcodes::FLOAD_0..=InternalOpcodes::FLOAD_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::FLoad,
                        var_index: (opcode - InternalOpcodes::FLOAD_0) as u16,
                    }
                }
                InternalOpcodes::DLOAD_0..=InternalOpcodes::DLOAD_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::DLoad,
                        var_index: (opcode - InternalOpcodes::DLOAD_0) as u16,
                    }
                }
                InternalOpcodes::ALOAD_0..=InternalOpcodes::ALOAD_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::ALoad,
                        var_index: (opcode - InternalOpcodes::ALOAD_0) as u16,
                    }
                }
                InternalOpcodes::ISTORE_0..=InternalOpcodes::ISTORE_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::IStore,
                        var_index: (opcode - InternalOpcodes::ISTORE_0) as u16,
                    }
                }
                InternalOpcodes::LSTORE_0..=InternalOpcodes::LSTORE_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::LStore,
                        var_index: (opcode - InternalOpcodes::LSTORE_0) as u16,
                    }
                }
                InternalOpcodes::FSTORE_0..=InternalOpcodes::FSTORE_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::FStore,
                        var_index: (opcode - InternalOpcodes::FSTORE_0) as u16,
                    }
                }
                InternalOpcodes::DSTORE_0..=InternalOpcodes::DSTORE_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::DStore,
                        var_index: (opcode - InternalOpcodes::DSTORE_0) as u16,
                    }
                }
                InternalOpcodes::ASTORE_0..=InternalOpcodes::ASTORE_3 => {
                    i += 1;
                    MethodEvent::VarInsn {
                        opcode: Opcode::AStore,
                        var_index: (opcode - InternalOpcodes::ASTORE_0) as u16,
                    }
                }
                InternalOpcodes::WIDE => {
                    let next_opcode = code.get_code(i + 1)?;
                    let next_opcode = Opcode::try_from(next_opcode)
                        .map_err(|_| ClassFileError::BadOpcode(next_opcode))?;
                    match next_opcode {
                        Opcode::ILoad
                        | Opcode::FLoad
                        | Opcode::ALoad
                        | Opcode::LLoad
                        | Opcode::DLoad
                        | Opcode::IStore
                        | Opcode::FStore
                        | Opcode::AStore
                        | Opcode::LStore
                        | Opcode::DStore
                        | Opcode::Ret => {
                            let var_index =
                                u16::from_be_bytes([code.get_code(i + 2)?, code.get_code(i + 3)?]);
                            i += 4;
                            MethodEvent::VarInsn {
                                opcode: next_opcode,
                                var_index,
                            }
                        }
                        Opcode::IInc => {
                            let var_index =
                                u16::from_be_bytes([code.get_code(i + 2)?, code.get_code(i + 3)?]);
                            let increment =
                                i16::from_be_bytes([code.get_code(i + 4)?, code.get_code(i + 5)?]);
                            i += 6;
                            MethodEvent::IIncInsn {
                                var_index,
                                increment,
                            }
                        }
                        _ => return Err(ClassFileError::BadWideOpcode(next_opcode)),
                    }
                }
                InternalOpcodes::GOTO_W => {
                    let branch = i32::from_be_bytes([
                        code.get_code(i + 1)?,
                        code.get_code(i + 2)?,
                        code.get_code(i + 3)?,
                        code.get_code(i + 4)?,
                    ]);
                    let label = insn_metadata
                        .get_code_mut(i.wrapping_add_signed(branch as isize))?
                        .get_or_create_label(label_creator);
                    i += 5;
                    MethodEvent::JumpInsn {
                        opcode: Opcode::Goto,
                        label,
                    }
                }
                InternalOpcodes::JSR_W => {
                    let branch = i32::from_be_bytes([
                        code.get_code(i + 1)?,
                        code.get_code(i + 2)?,
                        code.get_code(i + 3)?,
                        code.get_code(i + 4)?,
                    ]);
                    let label = insn_metadata
                        .get_code_mut(i.wrapping_add_signed(branch as isize))?
                        .get_or_create_label(label_creator);
                    i += 5;
                    MethodEvent::JumpInsn {
                        opcode: Opcode::Jsr,
                        label,
                    }
                }
                _ => {
                    let opcode =
                        Opcode::try_from(opcode).map_err(|_| ClassFileError::BadOpcode(opcode))?;
                    match opcode {
                        Opcode::Nop
                        | Opcode::AConstNull
                        | Opcode::IConstM1
                        | Opcode::IConst0
                        | Opcode::IConst1
                        | Opcode::IConst2
                        | Opcode::IConst3
                        | Opcode::IConst4
                        | Opcode::IConst5
                        | Opcode::LConst0
                        | Opcode::LConst1
                        | Opcode::FConst0
                        | Opcode::FConst1
                        | Opcode::FConst2
                        | Opcode::DConst0
                        | Opcode::DConst1
                        | Opcode::IALoad
                        | Opcode::LALoad
                        | Opcode::FALoad
                        | Opcode::DALoad
                        | Opcode::AALoad
                        | Opcode::BALoad
                        | Opcode::CALoad
                        | Opcode::SALoad
                        | Opcode::IAStore
                        | Opcode::LAStore
                        | Opcode::FAStore
                        | Opcode::DAStore
                        | Opcode::AAStore
                        | Opcode::BAStore
                        | Opcode::CAStore
                        | Opcode::SAStore
                        | Opcode::Pop
                        | Opcode::Pop2
                        | Opcode::Dup
                        | Opcode::DupX1
                        | Opcode::DupX2
                        | Opcode::Dup2
                        | Opcode::Dup2X1
                        | Opcode::Dup2X2
                        | Opcode::Swap
                        | Opcode::IAdd
                        | Opcode::LAdd
                        | Opcode::FAdd
                        | Opcode::DAdd
                        | Opcode::ISub
                        | Opcode::LSub
                        | Opcode::FSub
                        | Opcode::DSub
                        | Opcode::IMul
                        | Opcode::LMul
                        | Opcode::FMul
                        | Opcode::DMul
                        | Opcode::IDiv
                        | Opcode::LDiv
                        | Opcode::FDiv
                        | Opcode::DDiv
                        | Opcode::IRem
                        | Opcode::LRem
                        | Opcode::FRem
                        | Opcode::DRem
                        | Opcode::INeg
                        | Opcode::LNeg
                        | Opcode::FNeg
                        | Opcode::DNeg
                        | Opcode::IShl
                        | Opcode::LShl
                        | Opcode::IShr
                        | Opcode::LShr
                        | Opcode::IUShr
                        | Opcode::LUShr
                        | Opcode::IAnd
                        | Opcode::LAnd
                        | Opcode::IOr
                        | Opcode::LOr
                        | Opcode::IXor
                        | Opcode::LXor
                        | Opcode::I2l
                        | Opcode::I2f
                        | Opcode::I2d
                        | Opcode::L2i
                        | Opcode::L2f
                        | Opcode::L2d
                        | Opcode::F2i
                        | Opcode::F2l
                        | Opcode::F2d
                        | Opcode::D2i
                        | Opcode::D2l
                        | Opcode::D2f
                        | Opcode::I2b
                        | Opcode::I2c
                        | Opcode::I2s
                        | Opcode::LCmp
                        | Opcode::FCmpL
                        | Opcode::FCmpG
                        | Opcode::DCmpL
                        | Opcode::DCmpG
                        | Opcode::IReturn
                        | Opcode::LReturn
                        | Opcode::FReturn
                        | Opcode::DReturn
                        | Opcode::AReturn
                        | Opcode::Return
                        | Opcode::ArrayLength
                        | Opcode::AThrow
                        | Opcode::MonitorEnter
                        | Opcode::MonitorExit => {
                            i += 1;
                            MethodEvent::Insn(opcode)
                        }
                        Opcode::BIPush => {
                            let value = code.get_code(i + 1)? as i8;
                            i += 2;
                            MethodEvent::BIPushInsn(value)
                        }
                        Opcode::SIPush => {
                            let value =
                                i16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            i += 3;
                            MethodEvent::SIPushInsn(value)
                        }
                        Opcode::Ldc => {
                            let cst_index = code.get_code(i + 1)? as u16;
                            i += 2;
                            MethodEvent::LdcInsn(Self::get_ldc_constant(
                                reader,
                                cst_index,
                                bootstrap_methods,
                            )?)
                        }
                        Opcode::ILoad
                        | Opcode::LLoad
                        | Opcode::FLoad
                        | Opcode::DLoad
                        | Opcode::ALoad
                        | Opcode::IStore
                        | Opcode::LStore
                        | Opcode::FStore
                        | Opcode::DStore
                        | Opcode::AStore
                        | Opcode::Ret => {
                            let var_index = code.get_code(i + 1)? as u16;
                            i += 2;
                            MethodEvent::VarInsn { opcode, var_index }
                        }
                        Opcode::IInc => {
                            let var_index = code.get_code(i + 1)? as u16;
                            let increment = code.get_code(i + 2)? as i8 as i16;
                            i += 3;
                            MethodEvent::IIncInsn {
                                var_index,
                                increment,
                            }
                        }
                        Opcode::IfEq
                        | Opcode::IfNe
                        | Opcode::IfLt
                        | Opcode::IfGe
                        | Opcode::IfGt
                        | Opcode::IfLe
                        | Opcode::IfICmpEq
                        | Opcode::IfICmpNe
                        | Opcode::IfICmpLt
                        | Opcode::IfICmpGe
                        | Opcode::IfICmpGt
                        | Opcode::IfICmpLe
                        | Opcode::IfACmpEq
                        | Opcode::IfACmpNe
                        | Opcode::Goto
                        | Opcode::Jsr
                        | Opcode::IfNull
                        | Opcode::IfNonNull => {
                            let branch =
                                i16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let label = insn_metadata
                                .get_code_mut(i.wrapping_add_signed(branch as isize))?
                                .get_or_create_label(label_creator);
                            i += 3;
                            MethodEvent::JumpInsn { opcode, label }
                        }
                        Opcode::TableSwitch => {
                            i = (i + 1).next_multiple_of(4);
                            let dflt_branch = i32::from_be_bytes([
                                code.get_code(i)?,
                                code.get_code(i + 1)?,
                                code.get_code(i + 2)?,
                                code.get_code(i + 3)?,
                            ]);
                            let dflt = insn_metadata
                                .get_code_mut(insn_base.wrapping_add_signed(dflt_branch as isize))?
                                .get_or_create_label(label_creator);
                            let low = i32::from_be_bytes([
                                code.get_code(i + 4)?,
                                code.get_code(i + 5)?,
                                code.get_code(i + 6)?,
                                code.get_code(i + 7)?,
                            ]);
                            let high = i32::from_be_bytes([
                                code.get_code(i + 8)?,
                                code.get_code(i + 9)?,
                                code.get_code(i + 10)?,
                                code.get_code(i + 11)?,
                            ]);
                            if low > high {
                                return Err(ClassFileError::TableSwitchBoundsWrongOrder {
                                    low,
                                    high,
                                });
                            }
                            let label_count_m1 = high.wrapping_sub(low) as u32;
                            let labels = (0..=label_count_m1)
                                .map(|idx| -> ClassFileResult<_> {
                                    let branch = i32::from_be_bytes([
                                        code.get_code(i + 12 + 4 * idx as usize)?,
                                        code.get_code(i + 13 + 4 * idx as usize)?,
                                        code.get_code(i + 14 + 4 * idx as usize)?,
                                        code.get_code(i + 15 + 4 * idx as usize)?,
                                    ]);
                                    Ok(insn_metadata
                                        .get_code_mut(
                                            insn_base.wrapping_add_signed(branch as isize),
                                        )?
                                        .get_or_create_label(label_creator))
                                })
                                .collect::<ClassFileResult<Vec<_>>>()?;
                            i += 16 + 4 * label_count_m1 as usize;
                            MethodEvent::TableSwitchInsn {
                                dflt,
                                low,
                                high,
                                labels,
                            }
                        }
                        Opcode::LookupSwitch => {
                            i = (i + 1).next_multiple_of(4);
                            let dflt_branch = i32::from_be_bytes([
                                code.get_code(i)?,
                                code.get_code(i + 1)?,
                                code.get_code(i + 2)?,
                                code.get_code(i + 3)?,
                            ]);
                            let dflt = insn_metadata
                                .get_code_mut(insn_base.wrapping_add_signed(dflt_branch as isize))?
                                .get_or_create_label(label_creator);
                            let npairs = u32::from_be_bytes([
                                code.get_code(i + 4)?,
                                code.get_code(i + 5)?,
                                code.get_code(i + 6)?,
                                code.get_code(i + 7)?,
                            ]);
                            let values = (0..npairs)
                                .map(|idx| -> ClassFileResult<_> {
                                    let value = i32::from_be_bytes([
                                        code.get_code(i + 8 + 8 * idx as usize)?,
                                        code.get_code(i + 9 + 8 * idx as usize)?,
                                        code.get_code(i + 10 + 8 * idx as usize)?,
                                        code.get_code(i + 11 + 8 * idx as usize)?,
                                    ]);
                                    let branch = i32::from_be_bytes([
                                        code.get_code(i + 12 + 8 * idx as usize)?,
                                        code.get_code(i + 13 + 8 * idx as usize)?,
                                        code.get_code(i + 14 + 8 * idx as usize)?,
                                        code.get_code(i + 15 + 8 * idx as usize)?,
                                    ]);
                                    Ok((
                                        value,
                                        insn_metadata
                                            .get_code_mut(
                                                insn_base.wrapping_add_signed(branch as isize),
                                            )?
                                            .get_or_create_label(label_creator),
                                    ))
                                })
                                .collect::<ClassFileResult<Vec<_>>>()?;
                            i += 4 + 8 * npairs as usize;
                            MethodEvent::LookupSwitchInsn { dflt, values }
                        }
                        Opcode::GetStatic
                        | Opcode::PutStatic
                        | Opcode::GetField
                        | Opcode::PutField => {
                            let cp_index =
                                u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let field = reader.constant_pool.get_field_ref(cp_index)?;
                            i += 3;
                            MethodEvent::FieldInsn {
                                opcode,
                                owner: field.owner,
                                name: field.name,
                                desc: field.desc,
                            }
                        }
                        Opcode::InvokeVirtual
                        | Opcode::InvokeSpecial
                        | Opcode::InvokeStatic
                        | Opcode::InvokeInterface => {
                            let cp_index =
                                u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let is_interface = reader.constant_pool.get_type(cp_index)?
                                == ConstantPoolTag::InterfaceMethodRef;
                            let method = if is_interface {
                                reader.constant_pool.get_interface_method_ref(cp_index)?
                            } else {
                                reader.constant_pool.get_method_ref(cp_index)?
                            };
                            i += if opcode == Opcode::InvokeInterface {
                                5
                            } else {
                                3
                            };
                            MethodEvent::MethodInsn {
                                opcode,
                                owner: method.owner,
                                name: method.name,
                                desc: method.desc,
                                is_interface,
                            }
                        }
                        Opcode::InvokeDynamic => {
                            let cp_index =
                                u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let dynamic = reader.constant_pool.get_invoke_dynamic(cp_index)?;
                            let bootstrap_method = bootstrap_methods
                                .get(dynamic.bootstrap_method_attr_index)?
                                .clone();
                            i += 5;
                            MethodEvent::InvokeDynamicInsn {
                                name: dynamic.name,
                                desc: dynamic.desc,
                                bootstrap_method_handle: bootstrap_method.handle,
                                bootstrap_method_arguments: bootstrap_method.args,
                            }
                        }
                        Opcode::New
                        | Opcode::ANewArray
                        | Opcode::CheckCast
                        | Opcode::Instanceof => {
                            let cp_index =
                                u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let ty = reader.constant_pool.get_class(cp_index)?;
                            i += 3;
                            MethodEvent::TypeInsn { opcode, ty }
                        }
                        Opcode::NewArray => {
                            let atype = code.get_code(i + 1)?;
                            let atype = NewArrayType::try_from(atype)
                                .map_err(|_| ClassFileError::BadNewArrayType(atype))?;
                            i += 2;
                            MethodEvent::NewArrayInsn(atype)
                        }
                        Opcode::MultiANewArray => {
                            let cp_index =
                                u16::from_be_bytes([code.get_code(i + 1)?, code.get_code(i + 2)?]);
                            let desc = reader.constant_pool.get_class(cp_index)?;
                            let dimensions = code.get_code(i + 3)?;
                            i += 4;
                            MethodEvent::MultiANewArrayInsn { desc, dimensions }
                        }
                    }
                }
            };

            insn_metadata[insn_base].insn_event = Some(insn);
        }

        Ok(())
    }

    fn get_ldc_constant(
        reader: &ClassReader<'class>,
        index: u16,
        bootstrap_methods: &BootstrapMethods<'reader, 'class>,
    ) -> ClassFileResult<LdcConstant<'class>> {
        Ok(match reader.constant_pool.get(index)? {
            ConstantPoolEntry::Integer(i) => LdcConstant::Integer(i),
            ConstantPoolEntry::Float(f) => LdcConstant::Float(f),
            ConstantPoolEntry::Long(l) => LdcConstant::Long(l),
            ConstantPoolEntry::Double(d) => LdcConstant::Double(d),
            ConstantPoolEntry::String(s) => LdcConstant::String(s),
            ConstantPoolEntry::Class(c) => LdcConstant::Class(c),
            ConstantPoolEntry::MethodType(mt) => LdcConstant::MethodType(mt),
            ConstantPoolEntry::MethodHandle(h) => LdcConstant::Handle(h),
            ConstantPoolEntry::Dynamic(d) => {
                let bootstrap_method = bootstrap_methods
                    .get(d.bootstrap_method_attr_index)?
                    .clone();
                LdcConstant::ConstantDynamic(ConstantDynamic {
                    name: d.name,
                    desc: d.desc,
                    bootstrap_method: bootstrap_method.handle,
                    bootstrap_method_arguments: bootstrap_method.args,
                })
            }
            _ => {
                return Err(ClassFileError::BadConstantPoolTypeExpectedLdcOperand(
                    reader.constant_pool.get_type(index)?,
                ))
            }
        })
    }

    fn read_code_annotations(
        reader: &ClassReader<'class>,
        mut offset: usize,
        visible: bool,
        local_variable_annotations: &mut Vec<MethodLocalVariableAnnotationEvent<'class>>,
        try_catch_block_annotations: &mut Vec<MethodTryCatchBlockAnnotationEvent<'class>>,
        insn_metadata: &mut [InstructionMetadata<'reader, 'class>],
        label_creator: &LabelCreator,
    ) -> ClassFileResult<()> {
        let mut ann_offset = offset;
        let ann_count = reader.buffer.read_u16(ann_offset)?;
        ann_offset += 2;
        for _ in 0..ann_count {
            let (annotation, code_loc) = read_type_annotation(reader, &mut ann_offset)?;
            match code_loc {
                TypeAnnotationCodeLocation::None => {
                    // ignore invalid annotation
                }
                TypeAnnotationCodeLocation::Insn(pc) => {
                    insn_metadata
                        .get_code_mut(pc as usize)?
                        .annotations
                        .push(AnnotationEvent {
                            visible,
                            annotation,
                        });
                }
                TypeAnnotationCodeLocation::LocalVariable(ranges) => {
                    let ranges = ranges
                        .into_iter()
                        .map(|range| -> ClassFileResult<_> {
                            Ok((
                                insn_metadata
                                    .get_code_mut(range.start_pc as usize)?
                                    .get_or_create_label(label_creator),
                                insn_metadata
                                    .get_code_mut(range.start_pc as usize + range.length as usize)?
                                    .get_or_create_label(label_creator),
                                range.index,
                            ))
                        })
                        .collect::<ClassFileResult<Vec<_>>>()?;
                    local_variable_annotations.push(MethodLocalVariableAnnotationEvent {
                        ranges,
                        visible,
                        annotation,
                    });
                }
                TypeAnnotationCodeLocation::TryCatchBlock(index) => {
                    try_catch_block_annotations.push(MethodTryCatchBlockAnnotationEvent {
                        try_catch_block_index: index,
                        annotation,
                    });
                }
            }
        }

        Ok(())
    }

    fn read_frames(
        reader: &ClassReader<'class>,
        mut offset: usize,
        compressed: bool,
        insn_metadata: &mut [InstructionMetadata<'reader, 'class>],
        label_creator: &LabelCreator,
    ) -> ClassFileResult<()> {
        let frame_count = reader.buffer.read_u16(offset)?;
        offset += 2;

        let mut last_code_offset = None;

        for _ in 0..frame_count {
            let frame_type = if compressed {
                let frame_type = reader.buffer.read_u8(offset)?;
                offset += 1;
                frame_type
            } else {
                255 // full
            };

            let (offset_delta, frame) = match frame_type {
                0..=63 => (frame_type as u16, Frame::Same),
                64..=127 => {
                    let stack_value =
                        Self::read_frame_value(reader, &mut offset, insn_metadata, label_creator)?;
                    ((frame_type - 64) as u16, Frame::Same1 { stack_value })
                }
                247 => {
                    let offset_delta = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    let stack_value =
                        Self::read_frame_value(reader, &mut offset, insn_metadata, label_creator)?;
                    (offset_delta, Frame::Same1 { stack_value })
                }
                248..=250 => {
                    let offset_delta = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    (
                        offset_delta,
                        Frame::Chop {
                            num_locals: 251 - frame_type,
                        },
                    )
                }
                251 => {
                    let offset_delta = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    (offset_delta, Frame::Same)
                }
                252..=254 => {
                    let offset_delta = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    let locals = (0..frame_type - 251)
                        .map(|_| {
                            Self::read_frame_value(
                                reader,
                                &mut offset,
                                insn_metadata,
                                label_creator,
                            )
                        })
                        .collect::<ClassFileResult<Vec<_>>>()?;
                    (offset_delta, Frame::Append { locals })
                }
                255 => {
                    let offset_delta = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    let local_count = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    let locals = (0..local_count)
                        .map(|_| {
                            Self::read_frame_value(
                                reader,
                                &mut offset,
                                insn_metadata,
                                label_creator,
                            )
                        })
                        .collect::<ClassFileResult<Vec<_>>>()?;
                    let stack_count = reader.buffer.read_u16(offset)?;
                    offset += 2;
                    let stack = (0..stack_count)
                        .map(|_| {
                            Self::read_frame_value(
                                reader,
                                &mut offset,
                                insn_metadata,
                                label_creator,
                            )
                        })
                        .collect::<ClassFileResult<Vec<_>>>()?;
                    (offset_delta, Frame::Full { locals, stack })
                }
                _ => return Err(ClassFileError::BadFrameType(frame_type)),
            };

            let code_offset = match last_code_offset {
                None => offset_delta as usize,
                Some(last_code_offset) => last_code_offset + offset_delta as usize + 1,
            };
            last_code_offset = Some(code_offset);
            insn_metadata.get_code_mut(code_offset)?.frame = Some(frame);
        }

        Ok(())
    }

    fn read_frame_value(
        reader: &ClassReader<'class>,
        offset: &mut usize,
        insn_metadata: &mut [InstructionMetadata<'reader, 'class>],
        label_creator: &LabelCreator,
    ) -> ClassFileResult<FrameValue<'class>> {
        let tag = reader.buffer.read_u8(*offset)?;
        *offset += 1;

        Ok(match tag {
            0 => FrameValue::Top,
            1 => FrameValue::Integer,
            2 => FrameValue::Float,
            3 => FrameValue::Double,
            4 => FrameValue::Long,
            5 => FrameValue::Null,
            6 => FrameValue::UninitializedThis,
            7 => {
                let ty = reader
                    .constant_pool
                    .get_class(reader.buffer.read_u16(*offset)?)?;
                *offset += 2;
                FrameValue::Class(ty)
            }
            8 => {
                let cp_index = reader.buffer.read_u16(*offset)?;
                *offset += 2;
                FrameValue::Uninitialized(
                    insn_metadata
                        .get_code_mut(cp_index as usize)?
                        .get_or_create_label(label_creator),
                )
            }
            _ => return Err(ClassFileError::BadFrameValueTag(tag)),
        })
    }
}

#[derive(Debug, Default)]
struct InstructionMetadata<'reader, 'class>
where
    'class: 'reader,
{
    insn_event: Option<MethodEvent<'class, MethodReaderEventProviders<'reader, 'class>>>,
    label: Option<Label>,
    line_number: Option<u16>,
    frame: Option<Frame<'class>>,
    annotations: Vec<AnnotationEvent<TypeAnnotationNode<'class>>>,
}

impl InstructionMetadata<'_, '_> {
    fn get_or_create_label(&mut self, label_creator: &LabelCreator) -> Label {
        *self
            .label
            .get_or_insert_with(|| label_creator.create_label())
    }
}

trait CodeSliceExtensions<T> {
    fn get_code(self, index: usize) -> ClassFileResult<T>
    where
        T: Copy;
    fn get_code_ref<'a>(self, index: usize) -> ClassFileResult<&'a T>
    where
        Self: 'a;
}

impl<T> CodeSliceExtensions<T> for &[T] {
    fn get_code(self, index: usize) -> ClassFileResult<T>
    where
        T: Copy,
    {
        Ok(*self.get_code_ref(index)?)
    }

    fn get_code_ref<'a>(self, index: usize) -> ClassFileResult<&'a T>
    where
        Self: 'a,
    {
        self.get(index)
            .ok_or(ClassFileError::CodeOffsetOutOfBounds {
                index,
                len: self.len(),
            })
    }
}

trait CodeSliceExtensionsMut<T> {
    fn get_code_mut<'a>(self, index: usize) -> ClassFileResult<&'a mut T>
    where
        Self: 'a;
}

impl<T> CodeSliceExtensionsMut<T> for &mut [T] {
    fn get_code_mut<'a>(self, index: usize) -> ClassFileResult<&'a mut T>
    where
        Self: 'a,
    {
        let len = self.len();
        self.get_mut(index)
            .ok_or(ClassFileError::CodeOffsetOutOfBounds { index, len })
    }
}

#[derive(Debug)]
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

    type InsnAnnotations = WrapWithResultReaderIterator<
        std::vec::IntoIter<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type LocalVariables =
        WrapWithResultReaderIterator<std::vec::IntoIter<MethodLocalVariableEvent<'class>>>;

    type LocalVariableAnnotations = WrapWithResultReaderIterator<
        std::vec::IntoIter<MethodLocalVariableAnnotationEvent<'class>>,
    >;

    type TryCatchBlocks =
        WrapWithResultReaderIterator<std::vec::IntoIter<MethodTryCatchBlockEvent<'class>>>;

    type TryCatchBlockAnnotations = WrapWithResultReaderIterator<
        std::vec::IntoIter<MethodTryCatchBlockAnnotationEvent<'class>>,
    >;

    type CodeAttributes = CustomAttributeReaderIterator<'reader, 'class>;
}

define_simple_iterator!(
    MethodParameterReaderIterator,
    MethodParameterEvent<'class>,
    |reader: &ClassReader<'class>, offset: &mut usize| -> ClassFileResult<_> {
        let name = reader
            .constant_pool
            .get_optional_utf8(reader.buffer.read_u16(*offset)?)?;
        *offset += 2;
        let access = ParameterAccess::from_bits_retain(reader.buffer.read_u16(*offset)?);
        *offset += 2;
        Ok(MethodParameterEvent { name, access })
    }
);

#[derive(Debug)]
pub struct MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    visible_offset: usize,
    invisible_offset: usize,
    param_count: u8,
    param_index: u8,
    annotations_remaining: u16,
    state: u8,
}

impl<'reader, 'class> MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    fn new(
        reader: &'reader ClassReader<'class>,
        visible_offset: usize,
        invisible_offset: usize,
    ) -> Self {
        MethodParameterAnnotationsReaderIterator {
            reader,
            visible_offset,
            invisible_offset,
            param_count: 0,
            param_index: 0,
            annotations_remaining: 0,
            state: 0,
        }
    }
}

impl<'reader, 'class> Iterator for MethodParameterAnnotationsReaderIterator<'reader, 'class> {
    type Item = ClassFileResult<MethodParameterAnnotationEvent<'class>>;

    fn next(&mut self) -> Option<Self::Item> {
        // // L0
        // if self.visible_offset != 0 { // else goto L3
        //     self.param_count = self.reader.buffer.read_u8(self.visible_offset)?;
        //     self.visible_offset += 1;
        //
        //     // L1
        //     while self.param_index < self.param_count { // else goto L3
        //         self.annotations_remaining = self.reader.buffer.read_u16(self.visible_offset)?;
        //         self.visible_offset += 2;
        //
        //         // L2
        //         while self.annotations_remaining > 0 { // else goto L2.1
        //             self.annotations_remaining -= 1;
        //             let annotation = read_annotation(self.reader, &mut self.visible_offset, 0)?;
        //             yield Ok(MethodParameterAnnotationEvent {
        //                 parameter: self.param_index,
        //                 visible: true,
        //                 annotation
        //             });
        //             // goto L2
        //         }
        //         // L2.1
        //         self.param_index += 1;
        //         // goto L1
        //     }
        // }
        //
        // // L3
        // if self.invisible_offset != 0 { // else goto L6
        //     self.param_index = 0;
        //     self.param_count = self.reader.buffer.read_u8(self.invisible_offset)?;
        //     self.invisible_offset += 1;
        //
        //     // L4
        //     while self.param_index < self.param_count { // else goto L6
        //         self.annotations_remaining = self.reader.buffer.read_u16(self.invisible_offset)?;
        //         self.invisible_offset += 2;
        //
        //         // L5
        //         while self.annotations_remaining > 0 { // else goto L5.1
        //             self.annotations_remaining -= 1;
        //             let annotation = read_annotation(self.reader, &mut self.invisible_offset, 0)?;
        //             yield Ok(MethodParameterAnnotationEvent {
        //                 parameter: self.param_index,
        //                 visible: false,
        //                 annotation
        //             });
        //             // goto L5
        //         }
        //         // L5.1
        //         self.param_index += 1;
        //         // goto L4
        //     }
        // }
        // // L6
        // return;

        loop {
            match self.state {
                0 => {
                    if self.visible_offset == 0 {
                        self.state = 3;
                        continue;
                    }

                    self.param_count = match self.reader.buffer.read_u8(self.visible_offset) {
                        Ok(v) => v,
                        Err(err) => return Some(Err(err)),
                    };
                    self.visible_offset += 1;

                    self.state = 1;
                }
                1 => {
                    if self.param_index >= self.param_count {
                        self.state = 3;
                        continue;
                    }

                    self.annotations_remaining =
                        match self.reader.buffer.read_u16(self.visible_offset) {
                            Ok(v) => v,
                            Err(err) => return Some(Err(err)),
                        };
                    self.visible_offset += 2;

                    self.state = 2;
                }
                2 => {
                    if self.annotations_remaining == 0 {
                        self.param_index += 1;
                        self.state = 1;
                        continue;
                    }

                    self.annotations_remaining -= 1;
                    let annotation = match read_annotation(self.reader, &mut self.visible_offset, 0)
                    {
                        Ok(v) => v,
                        Err(err) => return Some(Err(err)),
                    };
                    return Some(Ok(MethodParameterAnnotationEvent {
                        parameter: self.param_index,
                        visible: true,
                        annotation,
                    }));
                }
                3 => {
                    if self.invisible_offset == 0 {
                        self.state = 6;
                        continue;
                    }

                    self.param_index = 0;
                    self.param_count = match self.reader.buffer.read_u8(self.invisible_offset) {
                        Ok(v) => v,
                        Err(err) => return Some(Err(err)),
                    };
                    self.invisible_offset += 1;

                    self.state = 4;
                }
                4 => {
                    if self.param_index >= self.param_count {
                        self.state = 6;
                        continue;
                    }

                    self.annotations_remaining =
                        match self.reader.buffer.read_u16(self.invisible_offset) {
                            Ok(v) => v,
                            Err(err) => return Some(Err(err)),
                        };
                    self.invisible_offset += 2;

                    self.state = 5;
                }
                5 => {
                    if self.annotations_remaining == 0 {
                        self.param_index += 1;
                        self.state = 4;
                        continue;
                    }

                    self.annotations_remaining -= 1;
                    let annotation =
                        match read_annotation(self.reader, &mut self.invisible_offset, 0) {
                            Ok(v) => v,
                            Err(err) => return Some(Err(err)),
                        };
                    return Some(Ok(MethodParameterAnnotationEvent {
                        parameter: self.param_index,
                        visible: false,
                        annotation,
                    }));
                }
                _ => return None,
            }
        }
    }
}

impl FusedIterator for MethodParameterAnnotationsReaderIterator<'_, '_> {}

#[derive(Debug)]
pub struct WrapWithResultReaderIterator<I> {
    inner: I,
}

impl<I> WrapWithResultReaderIterator<I> {
    fn new(inner: I) -> Self {
        WrapWithResultReaderIterator { inner }
    }
}

impl<I> Iterator for WrapWithResultReaderIterator<I>
where
    I: Iterator,
{
    type Item = ClassFileResult<I::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(Ok)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I> FusedIterator for WrapWithResultReaderIterator<I> where I: FusedIterator {}

impl<I> ExactSizeIterator for WrapWithResultReaderIterator<I>
where
    I: ExactSizeIterator,
{
    fn len(&self) -> usize {
        self.inner.len()
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
    LocalVariable(Vec<TypeAnnotationLocalVariableRange>),
    Insn(u16),
    TryCatchBlock(u16),
}

impl TypeAnnotationCodeLocation {
    fn read_local_variable(
        reader: &ClassReader<'_>,
        offset: &mut usize,
    ) -> ClassFileResult<Vec<TypeAnnotationLocalVariableRange>> {
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
            table.push(TypeAnnotationLocalVariableRange {
                start_pc,
                length,
                index,
            });
        }
        Ok(table)
    }
}

struct TypeAnnotationLocalVariableRange {
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
    let target_type = TypeReferenceTargetType::try_from(target_type)
        .map_err(|_| ClassFileError::BadTypeAnnotationTarget(target_type))?;
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
            code_location = TypeAnnotationCodeLocation::TryCatchBlock(try_catch_block_index);
            TypeReference::ExceptionParameter
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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
                                self.offset + 2,
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct RecordComponentReaderEvents<'reader, 'class> {
    reader: &'reader ClassReader<'class>,
    invisible_annotations_count: u16,
    invisible_annotations_offset: usize,
    invisible_type_annotations_count: u16,
    invisible_type_annotations_offset: usize,
    visible_annotations_count: u16,
    visible_annotations_offset: usize,
    visible_type_annotations_count: u16,
    visible_type_annotations_offset: usize,
    custom_attributes_offsets: Vec<usize>,
    state: u8,
}

impl<'reader, 'class> RecordComponentReaderEvents<'reader, 'class> {
    pub fn annotations(&self) -> AnnotationReaderIterator<'reader, 'class> {
        AnnotationReaderIterator::new(
            self.reader,
            self.visible_annotations_count,
            self.visible_annotations_offset,
            self.invisible_annotations_count,
            self.invisible_annotations_offset,
        )
    }

    pub fn type_annotations(&self) -> TypeAnnotationReaderIterator<'reader, 'class> {
        TypeAnnotationReaderIterator::new(
            self.reader,
            self.visible_type_annotations_count,
            self.visible_type_annotations_offset,
            self.invisible_type_annotations_count,
            self.invisible_type_annotations_offset,
        )
    }

    pub fn attributes(&self) -> CustomAttributeReaderIterator<'reader, 'class> {
        CustomAttributeReaderIterator::new(self.reader, self.custom_attributes_offsets.clone())
    }
}

impl<'reader, 'class> Iterator for RecordComponentReaderEvents<'reader, 'class> {
    type Item = ClassFileResult<
        RecordComponentEvent<'class, RecordComponentReaderEventProviders<'reader, 'class>>,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let state = self.state;
            self.state += 1;
            match state {
                0 => {
                    if self.visible_annotations_offset != 0
                        || self.invisible_annotations_offset != 0
                    {
                        return Some(Ok(RecordComponentEvent::Annotations(self.annotations())));
                    }
                }
                1 => {
                    if self.visible_type_annotations_offset != 0
                        || self.invisible_type_annotations_offset != 0
                    {
                        return Some(Ok(RecordComponentEvent::TypeAnnotations(
                            self.type_annotations(),
                        )));
                    }
                }
                2 => {
                    if !self.custom_attributes_offsets.is_empty() {
                        return Some(Ok(RecordComponentEvent::Attributes(self.attributes())));
                    }
                }
                _ => return None,
            }
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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

    fn read(&self, offset: usize) -> ClassFileResult<Box<dyn Attribute>> {
        let name = self
            .reader
            .constant_pool
            .get_utf8(self.reader.buffer.read_u16(offset)?)?;
        let len = self.reader.buffer.read_u32(offset)?;
        let buffer = self
            .reader
            .buffer
            .slice(offset + 6..offset + 6 + len as usize)?;
        match self.reader.attribute_readers.get(name.as_ref()) {
            Some(reader) => reader.read(&name, self.reader, buffer),
            None => Ok(Box::new(UnknownAttribute {
                name: name.into_owned(),
                data: buffer.data.to_vec(),
            })),
        }
    }
}

impl Iterator for CustomAttributeReaderIterator<'_, '_> {
    type Item = ClassFileResult<Box<dyn Attribute>>;

    fn next(&mut self) -> Option<Self::Item> {
        let offset = *self.offsets.get(self.index)?;
        self.index += 1;
        Some(self.read(offset))
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
    use crate::tree::{AnnotationNode, AnnotationValue, TypeAnnotationNode};
    use crate::{
        AnnotationEvent, ClassAccess, ClassEventSource, ClassFileResult, ClassInnerClassEvent,
        ClassOuterClassEvent, ClassReader, ClassReaderFlags, InnerClassAccess, ModuleProvidesEvent,
        ModuleRelationAccess, ModuleRelationEvent, ModuleRequireAccess, ModuleRequireEvent,
        TypePath, TypeReference,
    };
    use java_string::JavaStr;
    use std::borrow::Cow;
    use test_helpers::{include_class, java_version};

    #[test]
    fn test_hello_world() {
        const BYTECODE: &[u8] = include_class!("HelloWorld");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            ClassAccess::Public | ClassAccess::Super,
            reader.access().unwrap()
        );
        assert_eq!(JavaStr::from_str("HelloWorld"), reader.name().unwrap());
        assert_eq!(
            JavaStr::from_str("java/lang/Object"),
            reader.super_name().unwrap().unwrap()
        );
    }

    #[test]
    fn test_interfaces() {
        const BYTECODE: &[u8] = include_class!("TestInterfaces");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![
                JavaStr::from_str("java/lang/Runnable"),
                JavaStr::from_str("java/io/Serializable")
            ],
            reader
                .interfaces()
                .unwrap()
                .collect::<ClassFileResult<Vec<_>>>()
                .unwrap()
        )
    }

    #[test]
    fn test_signature() {
        const BYTECODE: &[u8] = include_class!("TestSignature");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            JavaStr::from_str("<T:Ljava/lang/Object;>Ljava/lang/Object;"),
            reader.events().unwrap().signature().unwrap().unwrap()
        )
    }

    #[test]
    fn test_deprecated() {
        const BYTECODE: &[u8] = include_class!("TestDeprecated");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert!(reader.events().unwrap().is_deprecated())
    }

    #[test]
    fn test_synthetic() {
        const BYTECODE: &[u8] = include_class!("TestSyntheticClass$1");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            JavaStr::from_str("TestSyntheticClass$1"),
            reader.name().unwrap()
        );
        assert!(reader.events().unwrap().is_synthetic());
    }

    #[test]
    fn test_synthetic_old() {
        const BYTECODE: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/TestSyntheticClass$1_Old.class"
        ));
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert!(reader.events().unwrap().is_synthetic());
    }

    #[test]
    fn test_source_file() {
        const BYTECODE: &[u8] = include_class!("HelloWorld");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            JavaStr::from_str("HelloWorld.java"),
            reader
                .events()
                .unwrap()
                .source()
                .unwrap()
                .unwrap()
                .source
                .unwrap()
        )
    }

    #[test]
    fn test_module() {
        const BYTECODE: &[u8] = include_class!("module-info");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(ClassAccess::Module, reader.access().unwrap());
        let module = reader.events().unwrap().module().unwrap().unwrap();
        assert_eq!(JavaStr::from_str("test"), module.name);
        assert_eq!(JavaStr::from_str("1.2.3"), module.version.unwrap());
        let mut events = module.events;

        let mut requires = events.next().unwrap().unwrap().unwrap_requires();
        assert_eq!(
            Some(Ok(ModuleRequireEvent {
                module: JavaStr::from_str("java.base").into(),
                version: Some(JavaStr::from_str(java_version!()).into()),
                access: ModuleRequireAccess::empty()
            })),
            requires.next()
        );
        assert_eq!(
            Some(Ok(ModuleRequireEvent {
                module: JavaStr::from_str("java.logging").into(),
                version: Some(JavaStr::from_str(java_version!()).into()),
                access: ModuleRequireAccess::StaticPhase
            })),
            requires.next()
        );
        assert_eq!(
            Some(Ok(ModuleRequireEvent {
                module: JavaStr::from_str("java.net.http").into(),
                version: Some(JavaStr::from_str(java_version!()).into()),
                access: ModuleRequireAccess::Transitive
            })),
            requires.next()
        );
        assert!(requires.next().is_none());

        let mut exports = events.next().unwrap().unwrap().unwrap_exports();
        assert_eq!(
            Some(Ok(ModuleRelationEvent {
                package: JavaStr::from_str("pkg").into(),
                modules: Vec::new(),
                access: ModuleRelationAccess::empty()
            })),
            exports.next()
        );
        assert_eq!(
            Some(Ok(ModuleRelationEvent {
                package: JavaStr::from_str("pkg2").into(),
                modules: vec![JavaStr::from_str("java.base").into()],
                access: ModuleRelationAccess::empty()
            })),
            exports.next()
        );
        assert!(exports.next().is_none());

        let mut opens = events.next().unwrap().unwrap().unwrap_opens();
        assert_eq!(
            Some(Ok(ModuleRelationEvent {
                package: JavaStr::from_str("pkg2").into(),
                modules: Vec::new(),
                access: ModuleRelationAccess::empty()
            })),
            opens.next()
        );
        assert_eq!(
            Some(Ok(ModuleRelationEvent {
                package: JavaStr::from_str("pkg").into(),
                modules: vec![JavaStr::from_str("java.base").into()],
                access: ModuleRelationAccess::empty()
            })),
            opens.next()
        );
        assert!(opens.next().is_none());

        let mut uses = events.next().unwrap().unwrap().unwrap_uses();
        assert_eq!(
            Some(Ok(JavaStr::from_str("java/lang/Runnable").into())),
            uses.next()
        );
        assert!(uses.next().is_none());

        let mut provides = events.next().unwrap().unwrap().unwrap_provides();
        assert_eq!(
            Some(Ok(ModuleProvidesEvent {
                service: JavaStr::from_str("java/lang/Runnable").into(),
                providers: vec![JavaStr::from_str("pkg/ClassInPackage").into()],
            })),
            provides.next()
        );
        assert!(provides.next().is_none());

        assert!(events.next().is_none());
    }

    #[test]
    fn test_nest_host() {
        const BYTECODE: &[u8] = include_class!("TestInnerClass$Inner");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            JavaStr::from_str("TestInnerClass"),
            reader.events().unwrap().nest_host().unwrap().unwrap()
        );
    }

    #[test]
    fn test_nest_members() {
        const BYTECODE: &[u8] = include_class!("TestInnerClass");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![JavaStr::from_str("TestInnerClass$Inner")],
            reader
                .events()
                .unwrap()
                .nest_members()
                .collect::<ClassFileResult<Vec<Cow<JavaStr>>>>()
                .unwrap(),
        );
    }

    #[test]
    fn test_outer_class() {
        const BYTECODE: &[u8] = include_class!("TestLocalClass$1Local");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            ClassOuterClassEvent {
                owner: JavaStr::from_str("TestLocalClass").into(),
                method_name: Some(JavaStr::from_str("test").into()),
                method_desc: Some(JavaStr::from_str("()V").into()),
            },
            reader.events().unwrap().outer_class().unwrap().unwrap()
        );
    }

    #[test]
    fn test_annotations() {
        const BYTECODE: &[u8] = include_class!("TestAnnotations");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![
                AnnotationEvent {
                    visible: true,
                    annotation: AnnotationNode {
                        desc: JavaStr::from_str("LVisibleAnnotation;").into(),
                        values: vec![
                            (
                                JavaStr::from_str("booleanValue").into(),
                                AnnotationValue::Boolean(true)
                            ),
                            (
                                JavaStr::from_str("byteValue").into(),
                                AnnotationValue::Byte(1)
                            ),
                            (
                                JavaStr::from_str("charValue").into(),
                                AnnotationValue::Char('a' as u16)
                            ),
                            (
                                JavaStr::from_str("shortValue").into(),
                                AnnotationValue::Short(2)
                            ),
                            (
                                JavaStr::from_str("intValue").into(),
                                AnnotationValue::Int(3)
                            ),
                            (
                                JavaStr::from_str("longValue").into(),
                                AnnotationValue::Long(4)
                            ),
                            (
                                JavaStr::from_str("floatValue").into(),
                                AnnotationValue::Float(5.0)
                            ),
                            (
                                JavaStr::from_str("doubleValue").into(),
                                AnnotationValue::Double(6.0)
                            ),
                            (
                                JavaStr::from_str("stringValue").into(),
                                AnnotationValue::String(JavaStr::from_str("Hello World").into())
                            ),
                            (
                                JavaStr::from_str("classValue").into(),
                                AnnotationValue::Class(
                                    JavaStr::from_str("Ljava/lang/String;").into()
                                )
                            ),
                            (
                                JavaStr::from_str("enumValue").into(),
                                AnnotationValue::Enum {
                                    desc: JavaStr::from_str("Ljava/lang/annotation/ElementType;")
                                        .into(),
                                    name: JavaStr::from_str("FIELD").into()
                                }
                            ),
                            (
                                JavaStr::from_str("annotationValue").into(),
                                AnnotationValue::Annotation(AnnotationNode {
                                    desc: JavaStr::from_str("Ljava/lang/Deprecated;").into(),
                                    values: vec![(
                                        JavaStr::from_str("forRemoval").into(),
                                        AnnotationValue::Boolean(true)
                                    )],
                                })
                            )
                        ]
                    }
                },
                AnnotationEvent {
                    visible: false,
                    annotation: AnnotationNode {
                        desc: JavaStr::from_str("LInvisibleAnnotation;").into(),
                        values: vec![
                            (
                                JavaStr::from_str("booleans").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Boolean(false),
                                    AnnotationValue::Boolean(true)
                                ])
                            ),
                            (
                                JavaStr::from_str("bytes").into(),
                                AnnotationValue::Array(vec![AnnotationValue::Byte(0)])
                            ),
                            (
                                JavaStr::from_str("chars").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Char('a' as u16),
                                    AnnotationValue::Char('b' as u16)
                                ])
                            ),
                            (
                                JavaStr::from_str("shorts").into(),
                                AnnotationValue::Array(vec![AnnotationValue::Short(1)])
                            ),
                            (
                                JavaStr::from_str("ints").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Int(1),
                                    AnnotationValue::Int(2)
                                ])
                            ),
                            (
                                JavaStr::from_str("longs").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Long(42),
                                    AnnotationValue::Long(69)
                                ])
                            ),
                            (
                                JavaStr::from_str("floats").into(),
                                AnnotationValue::Array(vec![AnnotationValue::Float(420.69)])
                            ),
                            (
                                JavaStr::from_str("doubles").into(),
                                AnnotationValue::Array(vec![AnnotationValue::Double(-100.0)])
                            ),
                            (
                                JavaStr::from_str("strings").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::String(JavaStr::from_str("Hello").into()),
                                    AnnotationValue::String(JavaStr::from_str("World").into())
                                ])
                            ),
                            (
                                JavaStr::from_str("classes").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Class(
                                        JavaStr::from_str("Ljava/lang/Class;").into()
                                    ),
                                    AnnotationValue::Class(JavaStr::from_str("V").into()),
                                    AnnotationValue::Class(
                                        JavaStr::from_str("[Ljava/lang/String;").into()
                                    )
                                ])
                            ),
                            (
                                JavaStr::from_str("enums").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Enum {
                                        desc: JavaStr::from_str(
                                            "Ljava/lang/annotation/ElementType;"
                                        )
                                        .into(),
                                        name: JavaStr::from_str("FIELD").into()
                                    },
                                    AnnotationValue::Enum {
                                        desc: JavaStr::from_str(
                                            "Ljava/lang/annotation/ElementType;"
                                        )
                                        .into(),
                                        name: JavaStr::from_str("METHOD").into()
                                    }
                                ])
                            ),
                            (
                                JavaStr::from_str("annotations").into(),
                                AnnotationValue::Array(vec![
                                    AnnotationValue::Annotation(AnnotationNode {
                                        desc: JavaStr::from_str("Ljava/lang/Deprecated;").into(),
                                        values: Vec::new()
                                    }),
                                    AnnotationValue::Annotation(AnnotationNode {
                                        desc: JavaStr::from_str("Ljava/lang/Deprecated;").into(),
                                        values: Vec::new()
                                    })
                                ])
                            )
                        ],
                    }
                }
            ],
            reader
                .events()
                .unwrap()
                .annotations()
                .collect::<ClassFileResult<Vec<AnnotationEvent<AnnotationNode>>>>()
                .unwrap()
        );
    }

    #[test]
    fn test_type_annotations() {
        const BYTECODE: &[u8] = include_class!("TestAnnotations");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![
                AnnotationEvent {
                    visible: true,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassExtends {
                            interface_index: None
                        },
                        type_path: TypePath::default(),
                        desc: JavaStr::from_str("LVisibleTypeAnnotation;").into(),
                        values: Vec::new(),
                    }
                },
                AnnotationEvent {
                    visible: true,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameter { param_index: 0 },
                        type_path: TypePath::default(),
                        desc: JavaStr::from_str("LVisibleTypeAnnotation;").into(),
                        values: Vec::new(),
                    }
                },
                AnnotationEvent {
                    visible: true,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameterBound {
                            param_index: 1,
                            bound_index: 1
                        },
                        type_path: "0;".parse().unwrap(),
                        desc: JavaStr::from_str("LVisibleTypeAnnotation;").into(),
                        values: Vec::new(),
                    }
                },
                AnnotationEvent {
                    visible: true,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameterBound {
                            param_index: 3,
                            bound_index: 1
                        },
                        type_path: "0;*".parse().unwrap(),
                        desc: JavaStr::from_str("LVisibleTypeAnnotation;").into(),
                        values: Vec::new()
                    }
                },
                AnnotationEvent {
                    visible: true,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameterBound {
                            param_index: 4,
                            bound_index: 1
                        },
                        type_path: "0;*".parse().unwrap(),
                        desc: JavaStr::from_str("LVisibleTypeAnnotation;").into(),
                        values: Vec::new()
                    }
                },
                AnnotationEvent {
                    visible: false,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassExtends {
                            interface_index: Some(0)
                        },
                        type_path: TypePath::default(),
                        desc: JavaStr::from_str("LInvisibleTypeAnnotation;").into(),
                        values: Vec::new()
                    }
                },
                AnnotationEvent {
                    visible: false,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameterBound {
                            param_index: 0,
                            bound_index: 0
                        },
                        type_path: TypePath::default(),
                        desc: JavaStr::from_str("LInvisibleTypeAnnotation;").into(),
                        values: Vec::new(),
                    }
                },
                AnnotationEvent {
                    visible: false,
                    annotation: TypeAnnotationNode {
                        type_ref: TypeReference::ClassTypeParameterBound {
                            param_index: 2,
                            bound_index: 1
                        },
                        type_path: "0;".parse().unwrap(),
                        desc: JavaStr::from_str("LInvisibleTypeAnnotation;").into(),
                        values: Vec::new(),
                    }
                }
            ],
            reader
                .events()
                .unwrap()
                .type_annotations()
                .collect::<ClassFileResult<Vec<AnnotationEvent<TypeAnnotationNode>>>>()
                .unwrap()
        );
    }

    #[test]
    fn test_permitted_subclasses() {
        const BYTECODE: &[u8] = include_class!("TestSealedClass");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![
                JavaStr::from_str("TestSealedClass$Foo"),
                JavaStr::from_str("TestSealedClass$Bar")
            ],
            reader
                .events()
                .unwrap()
                .permitted_subclasses()
                .collect::<ClassFileResult<Vec<_>>>()
                .unwrap()
        );
    }

    #[test]
    fn test_inner_classes() {
        const BYTECODE: &[u8] = include_class!("TestInnerClass");
        let reader = ClassReader::new(BYTECODE, ClassReaderFlags::None).unwrap();
        assert_eq!(
            vec![ClassInnerClassEvent {
                name: JavaStr::from_str("TestInnerClass$Inner").into(),
                inner_name: Some(JavaStr::from_str("Inner").into()),
                outer_name: Some(JavaStr::from_str("TestInnerClass").into()),
                access: InnerClassAccess::Private | InnerClassAccess::Static
            }],
            reader
                .events()
                .unwrap()
                .inner_classes()
                .collect::<ClassFileResult<Vec<_>>>()
                .unwrap()
        );
    }
}
