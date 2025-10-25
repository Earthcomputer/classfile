use crate::tree::{AnnotationNode, AnnotationValue, TypeAnnotationNode};
use crate::{
    Attribute, BootstrapMethodArgument, ClassFileResult, FieldAccess, FieldValue, FrameType,
    FrameValue, Handle, InnerClassAccess, Label, LabelCreator, LdcConstant, MethodAccess,
    ModuleAccess, ModuleRelationAccess, ModuleRequireAccess, NewarrayType, Opcode, ParameterAccess,
    TypePath, TypeReference,
};
use java_string::JavaStr;
use std::borrow::Cow;

#[non_exhaustive]
pub enum ClassEvent<'class, P>
where
    P: ClassEventProviders<'class>,
{
    Class(ClassClassEvent<'class>),
    Source(ClassSourceEvent<'class>),
    Module(ClassModuleEvent<'class, P::ModuleEvents>),
    NestHost(Cow<'class, JavaStr>),
    OuterClass(ClassOuterClassEvent<'class>),
    Annotations(P::Annotations),
    TypeAnnotations(P::TypeAnnotations),
    Attributes(P::Attributes),
    NestMembers(P::NestMembers),
    PermittedSubclasses(P::PermittedSubclasses),
    InnerClasses(P::InnerClasses),
    Record(P::RecordComponents),
    Fields(P::Fields),
    Methods(P::Methods),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassClassEvent<'class> {
    pub major_version: u16,
    pub minor_version: u16,
    pub name: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub super_name: Option<Cow<'class, JavaStr>>,
    pub interfaces: Vec<Cow<'class, JavaStr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassSourceEvent<'class> {
    pub source: Option<Cow<'class, JavaStr>>,
    pub debug: Option<Cow<'class, JavaStr>>,
}

pub struct ClassModuleEvent<'class, E> {
    pub name: Cow<'class, JavaStr>,
    pub access: ModuleAccess,
    pub version: Option<Cow<'class, JavaStr>>,
    pub events: E,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassOuterClassEvent<'class> {
    pub owner: Cow<'class, JavaStr>,
    pub method_name: Option<Cow<'class, JavaStr>>,
    pub method_desc: Option<Cow<'class, JavaStr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassInnerClassEvent<'class> {
    pub name: Cow<'class, JavaStr>,
    pub outer_name: Option<Cow<'class, JavaStr>>,
    pub inner_name: Option<Cow<'class, JavaStr>>,
    pub access: InnerClassAccess,
}

pub struct ClassRecordComponentEvent<'class, E> {
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub events: E,
}

pub struct ClassFieldEvent<'class, E> {
    pub access: FieldAccess,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub value: Option<FieldValue<'class>>,
    pub events: E,
}

pub struct ClassMethodEvent<'class, E> {
    pub access: MethodAccess,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub exceptions: Vec<Cow<'class, JavaStr>>,
    pub events: E,
}

pub trait ClassEventSource<'class> {
    type Providers: ClassEventProviders<'class>;
    type Iterator: Iterator<Item = ClassFileResult<ClassEvent<'class, Self::Providers>>>;

    fn events(self) -> ClassFileResult<Self::Iterator>;
}

impl<'class, T, P> ClassEventSource<'class> for T
where
    P: ClassEventProviders<'class>,
    T: IntoIterator<Item = ClassFileResult<ClassEvent<'class, P>>>,
{
    type Providers = P;
    type Iterator = T::IntoIter;

    fn events(self) -> ClassFileResult<Self::Iterator> {
        Ok(self.into_iter())
    }
}

pub trait ClassEventProviders<'class> {
    type ModuleSubProviders: ModuleEventProviders<'class>;
    type ModuleEvents: IntoIterator<
        Item = ClassFileResult<ModuleEvent<'class, Self::ModuleSubProviders>>,
    >;

    type Annotations: IntoIterator<Item = ClassFileResult<AnnotationEvent<AnnotationNode<'class>>>>;

    type TypeAnnotations: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type Attributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;

    type NestMembers: IntoIterator<Item = ClassFileResult<Cow<'class, JavaStr>>>;

    type PermittedSubclasses: IntoIterator<Item = ClassFileResult<Cow<'class, JavaStr>>>;

    type InnerClasses: IntoIterator<Item = ClassFileResult<ClassInnerClassEvent<'class>>>;

    type RecordComponentSubProviders: RecordComponentEventProviders<'class>;
    type RecordComponentEvents: IntoIterator<
        Item = ClassFileResult<RecordComponentEvent<'class, Self::RecordComponentSubProviders>>,
    >;
    type RecordComponents: IntoIterator<
        Item = ClassFileResult<ClassRecordComponentEvent<'class, Self::RecordComponentEvents>>,
    >;

    type FieldSubProviders: FieldEventProviders<'class>;
    type FieldEvents: IntoIterator<
        Item = ClassFileResult<FieldEvent<'class, Self::FieldSubProviders>>,
    >;
    type Fields: IntoIterator<Item = ClassFileResult<ClassFieldEvent<'class, Self::FieldEvents>>>;

    type MethodSubProviders: MethodEventProviders<'class>;
    type MethodEvents: IntoIterator<
        Item = ClassFileResult<MethodEvent<'class, Self::MethodSubProviders>>,
    >;
    type Methods: IntoIterator<Item = ClassFileResult<ClassMethodEvent<'class, Self::MethodEvents>>>;
}

#[non_exhaustive]
pub enum FieldEvent<'class, P>
where
    P: FieldEventProviders<'class>,
{
    Annotations(P::Annotations),
    TypeAnnotations(P::TypeAnnotations),
    Attributes(P::Attributes),
}

pub trait FieldEventProviders<'class> {
    type Annotations: IntoIterator<Item = ClassFileResult<AnnotationEvent<AnnotationNode<'class>>>>;

    type TypeAnnotations: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type Attributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;
}

#[non_exhaustive]
pub enum MethodEvent<'class, P>
where
    P: MethodEventProviders<'class>,
{
    Parameters(P::Parameters),
    AnnotationDefault(AnnotationValue<'class>),
    Annotations(P::Annotations),
    TypeAnnotations(P::TypeAnnotations),
    AnnotableParameterCount(MethodAnnotableParameterCountEvent),
    ParameterAnnotations(P::ParameterAnnotations),
    Attributes(P::Attributes),
    Code {
        label_creator: P::LabelCreator,
    },
    Frame {
        kind: FrameType,
        locals: Vec<FrameValue<'class>>,
        stack: Vec<FrameValue<'class>>,
    },
    Insn(Opcode),
    BipushInsn(i8),
    SipushInsn(i16),
    NewarrayInsn(NewarrayType),
    VarInsn {
        opcode: Opcode,
        var_index: u16,
    },
    TypeInsn {
        opcode: Opcode,
        ty: Cow<'class, JavaStr>,
    },
    FieldInsn {
        opcode: Opcode,
        owner: Cow<'class, JavaStr>,
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
    },
    MethodInsn {
        opcode: Opcode,
        owner: Cow<'class, JavaStr>,
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
    },
    InvokeDynamicInsn {
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
        bootstrap_method_handle: Handle<'class>,
        bootstrap_method_arguments: Vec<BootstrapMethodArgument<'class>>,
    },
    JumpInsn {
        opcode: Opcode,
        label: Label,
    },
    Label(Label),
    LdcInsn(LdcConstant<'class>),
    IincInsn {
        var_index: u16,
        increment: i16,
    },
    TableSwitchInsn {
        low: i32,
        high: i32,
        dflt: Label,
        labels: Vec<Label>,
    },
    LookupSwitchInsn {
        dflt: Label,
        values: Vec<(i32, Label)>,
    },
    MultiANewArrayInsn {
        desc: Cow<'class, JavaStr>,
        dimensions: u8,
    },
    InsnAnnotation {
        type_ref: TypeReference,
        type_path: TypePath<'class>,
        desc: Cow<'class, JavaStr>,
        visible: bool,
        events: P::InsnAnnotationEvents,
    },
    TryCatchBlock {
        start: Label,
        end: Label,
        handler: Label,
        ty: Option<Cow<'class, JavaStr>>,
    },
    TryCatchAnnotation {
        type_ref: TypeReference,
        type_path: TypePath<'class>,
        desc: Cow<'class, JavaStr>,
        visible: bool,
        events: P::TryCatchAnnotationEvents,
    },
    LocalVariable {
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
        signature: Option<Cow<'class, JavaStr>>,
        start: Label,
        end: Label,
        index: u16,
    },
    LocalVariableAnnotation {
        type_ref: TypeReference,
        type_path: TypePath<'class>,
        ranges: Vec<(Label, Label, u16)>,
        desc: Cow<'class, JavaStr>,
        visible: bool,
        events: P::LocalVariableAnnotationEvents,
    },
    LineNumber {
        line: u16,
        start: Label,
    },
    CodeAttributes(P::CodeAttributes),
    Maxs(MethodMaxsEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodParameterEvent<'class> {
    pub name: Option<Cow<'class, JavaStr>>,
    pub access: ParameterAccess,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodAnnotableParameterCountEvent {
    pub count: u8,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct MethodParameterAnnotationEvent<'class> {
    pub parameter: u8,
    pub visible: bool,
    pub annotation: AnnotationNode<'class>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodMaxsEvent {
    pub max_stack: u16,
    pub max_locals: u16,
}

pub trait MethodEventProviders<'class> {
    type Parameters: IntoIterator<Item = ClassFileResult<MethodParameterEvent<'class>>>;

    type Annotations: IntoIterator<Item = ClassFileResult<AnnotationEvent<AnnotationNode<'class>>>>;

    type TypeAnnotations: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type ParameterAnnotations: IntoIterator<
        Item = ClassFileResult<MethodParameterAnnotationEvent<'class>>,
    >;

    type Attributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;

    type InsnAnnotationEvents: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type TryCatchAnnotationEvents: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type LocalVariableAnnotationEvents: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type CodeAttributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;

    type LabelCreator: LabelCreator;
}

pub struct AnnotationEvent<A> {
    pub visible: bool,
    pub annotation: A,
}

pub enum ModuleEvent<'class, P>
where
    P: ModuleEventProviders<'class>,
{
    MainClass(Cow<'class, JavaStr>),
    Packages(P::Packages),
    Requires(P::Requires),
    Exports(P::Exports),
    Opens(P::Opens),
    Uses(P::Uses),
    Provides(P::Provides),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleRequireEvent<'class> {
    pub module: Cow<'class, JavaStr>,
    pub access: ModuleRequireAccess,
    pub version: Option<Cow<'class, JavaStr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleRelationEvent<'class> {
    pub package: Cow<'class, JavaStr>,
    pub access: ModuleRelationAccess,
    pub modules: Vec<Cow<'class, JavaStr>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleProvidesEvent<'class> {
    pub service: Cow<'class, JavaStr>,
    pub providers: Vec<Cow<'class, JavaStr>>,
}

pub trait ModuleEventProviders<'class> {
    type Packages: IntoIterator<Item = ClassFileResult<Cow<'class, JavaStr>>>;
    type Requires: IntoIterator<Item = ClassFileResult<ModuleRequireEvent<'class>>>;
    type Exports: IntoIterator<Item = ClassFileResult<ModuleRelationEvent<'class>>>;
    type Opens: IntoIterator<Item = ClassFileResult<ModuleRelationEvent<'class>>>;
    type Uses: IntoIterator<Item = ClassFileResult<Cow<'class, JavaStr>>>;
    type Provides: IntoIterator<Item = ClassFileResult<ModuleProvidesEvent<'class>>>;
}

#[non_exhaustive]
pub enum RecordComponentEvent<'class, P>
where
    P: RecordComponentEventProviders<'class>,
{
    Annotations(P::Annotations),
    TypeAnnotations(P::TypeAnnotations),
    Attributes(P::Attributes),
}

pub trait RecordComponentEventProviders<'class> {
    type Annotations: IntoIterator<Item = ClassFileResult<AnnotationEvent<AnnotationNode<'class>>>>;

    type TypeAnnotations: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type Attributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;
}
