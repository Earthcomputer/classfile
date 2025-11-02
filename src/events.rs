use crate::tree::{AnnotationNode, AnnotationValue, TypeAnnotationNode};
use crate::{
    Attribute, BootstrapMethodArgument, ClassAccess, ClassFileResult, FieldAccess, FieldValue,
    Frame, FrameValue, Handle, InnerClassAccess, Label, LabelCreator, LdcConstant, MethodAccess,
    ModuleAccess, ModuleRelationAccess, ModuleRequireAccess, NewArrayType, Opcode, ParameterAccess,
    TypePath, TypeReference,
};
use derive_more::{Debug, IsVariant, TryUnwrap, Unwrap};
use java_string::JavaStr;
use std::borrow::Cow;

#[derive(Debug, IsVariant, TryUnwrap, Unwrap)]
#[non_exhaustive]
pub enum ClassEvent<'class, P>
where
    P: ClassEventProviders<'class>,
{
    Class(ClassClassEvent<'class>),
    Synthetic,
    Deprecated,
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
    pub access: ClassAccess,
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct ClassRecordComponentEvent<'class, E> {
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub events: E,
}

#[derive(Debug)]
pub struct ClassFieldEvent<'class, E> {
    pub access: FieldAccess,
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub value: Option<FieldValue<'class>>,
    pub events: E,
}

#[derive(Debug)]
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

#[derive(Debug, IsVariant, TryUnwrap, Unwrap)]
#[non_exhaustive]
pub enum FieldEvent<'class, P>
where
    P: FieldEventProviders<'class>,
{
    Deprecated,
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

#[derive(Debug, IsVariant, TryUnwrap, Unwrap)]
#[non_exhaustive]
pub enum MethodEvent<'class, P>
where
    P: MethodEventProviders<'class>,
{
    Deprecated,
    Parameters(P::Parameters),
    AnnotationDefault(AnnotationValue<'class>),
    Annotations(P::Annotations),
    TypeAnnotations(P::TypeAnnotations),
    AnnotableParameterCount(MethodAnnotableParameterCountEvent),
    ParameterAnnotations(P::ParameterAnnotations),
    Attributes(P::Attributes),
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    Code {
        label_creator: LabelCreator,
    },
    Frame(Frame<'class>),
    Insn(Opcode),
    BIPushInsn(i8),
    SIPushInsn(i16),
    NewArrayInsn(NewArrayType),
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    VarInsn {
        opcode: Opcode,
        var_index: u16,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    TypeInsn {
        opcode: Opcode,
        ty: Cow<'class, JavaStr>,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    FieldInsn {
        opcode: Opcode,
        owner: Cow<'class, JavaStr>,
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    MethodInsn {
        opcode: Opcode,
        owner: Cow<'class, JavaStr>,
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
        is_interface: bool,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    InvokeDynamicInsn {
        name: Cow<'class, JavaStr>,
        desc: Cow<'class, JavaStr>,
        bootstrap_method_handle: Handle<'class>,
        bootstrap_method_arguments: Vec<BootstrapMethodArgument<'class>>,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    JumpInsn {
        opcode: Opcode,
        label: Label,
    },
    Label(Label),
    LdcInsn(LdcConstant<'class>),
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    IIncInsn {
        var_index: u16,
        increment: i16,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    TableSwitchInsn {
        low: i32,
        high: i32,
        dflt: Label,
        labels: Vec<Label>,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    LookupSwitchInsn {
        dflt: Label,
        values: Vec<(i32, Label)>,
    },
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    MultiANewArrayInsn {
        desc: Cow<'class, JavaStr>,
        dimensions: u8,
    },
    InsnAnnotations(P::InsnAnnotations),
    #[try_unwrap(ignore)]
    #[unwrap(ignore)]
    LineNumber {
        line: u16,
        start: Label,
    },
    LocalVariables(P::LocalVariables),
    LocalVariableAnnotations(P::LocalVariableAnnotations),
    TryCatchBlocks(P::TryCatchBlocks),
    TryCatchBlockAnnotations(P::TryCatchBlockAnnotations),
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodLocalVariableEvent<'class> {
    pub name: Cow<'class, JavaStr>,
    pub desc: Cow<'class, JavaStr>,
    pub signature: Option<Cow<'class, JavaStr>>,
    pub start: Label,
    pub end: Label,
    pub index: u16,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct MethodLocalVariableAnnotationEvent<'class> {
    pub ranges: Vec<(Label, Label, u16)>,
    pub visible: bool,
    pub annotation: TypeAnnotationNode<'class>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MethodTryCatchBlockEvent<'class> {
    pub start: Label,
    pub end: Label,
    pub handler: Label,
    pub ty: Option<Cow<'class, JavaStr>>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct MethodTryCatchBlockAnnotationEvent<'class> {
    pub try_catch_block_index: u16,
    pub annotation: TypeAnnotationNode<'class>,
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

    type InsnAnnotations: IntoIterator<
        Item = ClassFileResult<AnnotationEvent<TypeAnnotationNode<'class>>>,
    >;

    type LocalVariables: IntoIterator<Item = ClassFileResult<MethodLocalVariableEvent<'class>>>;

    type LocalVariableAnnotations: IntoIterator<
        Item = ClassFileResult<MethodLocalVariableAnnotationEvent<'class>>,
    >;

    type TryCatchBlocks: IntoIterator<Item = ClassFileResult<MethodTryCatchBlockEvent<'class>>>;

    type TryCatchBlockAnnotations: IntoIterator<
        Item = ClassFileResult<MethodTryCatchBlockAnnotationEvent<'class>>,
    >;

    type CodeAttributes: IntoIterator<Item = ClassFileResult<Box<dyn Attribute>>>;
}

#[derive(Debug, PartialEq, PartialOrd)]
pub struct AnnotationEvent<A> {
    pub visible: bool,
    pub annotation: A,
}

#[derive(Debug, IsVariant, TryUnwrap, Unwrap)]
#[non_exhaustive]
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

#[derive(Debug, IsVariant, TryUnwrap, Unwrap)]
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
