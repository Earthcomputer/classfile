use crate::{ConstantDynamic, Handle};
use java_string::JavaStr;
use std::borrow::Cow;
use strum::{Display, FromRepr};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromRepr)]
#[repr(u8)]
#[non_exhaustive]
#[strum(serialize_all = "lowercase")]
pub enum Opcode {
    Nop = 0,
    #[strum(serialize = "aconst_null")]
    AConstNull = 1,
    #[strum(serialize = "iconst_m1")]
    IConstM1 = 2,
    #[strum(serialize = "iconst_0")]
    IConst0 = 3,
    #[strum(serialize = "iconst_1")]
    IConst1 = 4,
    #[strum(serialize = "iconst_2")]
    IConst2 = 5,
    #[strum(serialize = "iconst_3")]
    IConst3 = 6,
    #[strum(serialize = "iconst_4")]
    IConst4 = 7,
    #[strum(serialize = "iconst_5")]
    IConst5 = 8,
    #[strum(serialize = "lconst_0")]
    LConst0 = 9,
    #[strum(serialize = "lconst_1")]
    LConst1 = 10,
    #[strum(serialize = "fconst_0")]
    FConst0 = 11,
    #[strum(serialize = "fconst_1")]
    FConst1 = 12,
    #[strum(serialize = "fconst_2")]
    FConst2 = 13,
    #[strum(serialize = "dconst_0")]
    DConst0 = 14,
    #[strum(serialize = "dconst_1")]
    DConst1 = 15,
    BIPush = 16,
    SIPush = 17,
    Ldc = 18,
    ILoad = 21,
    LLoad = 22,
    FLoad = 23,
    DLoad = 24,
    ALoad = 25,
    IALoad = 46,
    LALoad = 47,
    FALoad = 48,
    DALoad = 49,
    AALoad = 50,
    BALoad = 51,
    CALoad = 52,
    SALoad = 53,
    IStore = 54,
    LStore = 55,
    FStore = 56,
    DStore = 57,
    AStore = 58,
    IAStore = 79,
    LAStore = 80,
    FAStore = 81,
    DAStore = 82,
    AAStore = 83,
    BAStore = 84,
    CAStore = 85,
    SAStore = 86,
    Pop = 87,
    Pop2 = 88,
    Dup = 89,
    #[strum(serialize = "dup_x1")]
    DupX1 = 90,
    #[strum(serialize = "dup_x2")]
    DupX2 = 91,
    Dup2 = 92,
    #[strum(serialize = "dup2_x1")]
    Dup2X1 = 93,
    #[strum(serialize = "dup2_x2")]
    Dup2X2 = 94,
    Swap = 95,
    IAdd = 96,
    LAdd = 97,
    FAdd = 98,
    DAdd = 99,
    ISub = 100,
    LSub = 101,
    FSub = 102,
    DSub = 103,
    IMul = 104,
    LMul = 105,
    FMul = 106,
    DMul = 107,
    IDiv = 108,
    LDiv = 109,
    FDiv = 110,
    DDiv = 111,
    IRem = 112,
    LRem = 113,
    FRem = 114,
    DRem = 115,
    INeg = 116,
    LNeg = 117,
    FNeg = 118,
    DNeg = 119,
    IShl = 120,
    LShl = 121,
    IShr = 122,
    LShr = 123,
    IUShr = 124,
    LUShr = 125,
    IAnd = 126,
    LAnd = 127,
    IOr = 128,
    LOr = 129,
    IXor = 130,
    LXor = 131,
    IInc = 132,
    I2l = 133,
    I2f = 134,
    I2d = 135,
    L2i = 136,
    L2f = 137,
    L2d = 138,
    F2i = 139,
    F2l = 140,
    F2d = 141,
    D2i = 142,
    D2l = 143,
    D2f = 144,
    I2b = 145,
    I2c = 146,
    I2s = 147,
    LCmp = 148,
    FCmpL = 149,
    FCmpG = 150,
    DCmpL = 151,
    DCmpG = 152,
    IfEq = 153,
    IfNe = 154,
    IfLt = 155,
    IfGe = 156,
    IfGt = 157,
    IfLe = 158,
    #[strum(serialize = "if_icmpeq")]
    IfICmpEq = 159,
    #[strum(serialize = "if_icmpne")]
    IfICmpNe = 160,
    #[strum(serialize = "if_icmplt")]
    IfICmpLt = 161,
    #[strum(serialize = "if_icmpge")]
    IfICmpGe = 162,
    #[strum(serialize = "if_icmpgt")]
    IfICmpGt = 163,
    #[strum(serialize = "if_icmple")]
    IfICmpLe = 164,
    #[strum(serialize = "if_acmpeq")]
    IfACmpEq = 165,
    #[strum(serialize = "if_acmpne")]
    IfACmpNe = 166,
    Goto = 167,
    Jsr = 168,
    Ret = 169,
    TableSwitch = 170,
    LookupSwitch = 171,
    IReturn = 172,
    LReturn = 173,
    FReturn = 174,
    DReturn = 175,
    AReturn = 176,
    Return = 177,
    GetStatic = 178,
    PutStatic = 179,
    GetField = 180,
    PutField = 181,
    InvokeVirtual = 182,
    InvokeSpecial = 183,
    InvokeStatic = 184,
    InvokeInterface = 185,
    InvokeDynamic = 186,
    New = 187,
    NewArray = 188,
    ANewArray = 189,
    ArrayLength = 190,
    AThrow = 191,
    CheckCast = 192,
    Instanceof = 193,
    MonitorEnter = 194,
    MonitorExit = 195,
    MultiANewArray = 197,
    IfNull = 198,
    IfNonNull = 199,
}

pub(crate) struct InternalOpcodes;

impl InternalOpcodes {
    pub(crate) const LDC_W: u8 = 19;
    pub(crate) const LDC2_W: u8 = 20;
    pub(crate) const ILOAD_0: u8 = 26;
    pub(crate) const ILOAD_1: u8 = 27;
    pub(crate) const ILOAD_2: u8 = 28;
    pub(crate) const ILOAD_3: u8 = 29;
    pub(crate) const LLOAD_0: u8 = 30;
    pub(crate) const LLOAD_1: u8 = 31;
    pub(crate) const LLOAD_2: u8 = 32;
    pub(crate) const LLOAD_3: u8 = 33;
    pub(crate) const FLOAD_0: u8 = 34;
    pub(crate) const FLOAD_1: u8 = 35;
    pub(crate) const FLOAD_2: u8 = 36;
    pub(crate) const FLOAD_3: u8 = 37;
    pub(crate) const DLOAD_0: u8 = 38;
    pub(crate) const DLOAD_1: u8 = 39;
    pub(crate) const DLOAD_2: u8 = 40;
    pub(crate) const DLOAD_3: u8 = 41;
    pub(crate) const ALOAD_0: u8 = 42;
    pub(crate) const ALOAD_1: u8 = 43;
    pub(crate) const ALOAD_2: u8 = 44;
    pub(crate) const ALOAD_3: u8 = 45;
    pub(crate) const ISTORE_0: u8 = 59;
    pub(crate) const ISTORE_1: u8 = 60;
    pub(crate) const ISTORE_2: u8 = 61;
    pub(crate) const ISTORE_3: u8 = 62;
    pub(crate) const LSTORE_0: u8 = 63;
    pub(crate) const LSTORE_1: u8 = 64;
    pub(crate) const LSTORE_2: u8 = 65;
    pub(crate) const LSTORE_3: u8 = 66;
    pub(crate) const FSTORE_0: u8 = 67;
    pub(crate) const FSTORE_1: u8 = 68;
    pub(crate) const FSTORE_2: u8 = 69;
    pub(crate) const FSTORE_3: u8 = 70;
    pub(crate) const DSTORE_0: u8 = 71;
    pub(crate) const DSTORE_1: u8 = 72;
    pub(crate) const DSTORE_2: u8 = 73;
    pub(crate) const DSTORE_3: u8 = 74;
    pub(crate) const ASTORE_0: u8 = 75;
    pub(crate) const ASTORE_1: u8 = 76;
    pub(crate) const ASTORE_2: u8 = 77;
    pub(crate) const ASTORE_3: u8 = 78;
    pub(crate) const WIDE: u8 = 196;
    pub(crate) const GOTO_W: u8 = 200;
    pub(crate) const JSR_W: u8 = 201;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Display, FromRepr)]
#[repr(u8)]
pub enum NewArrayType {
    Boolean = 4,
    Char = 5,
    Float = 6,
    Double = 7,
    Byte = 8,
    Short = 9,
    Int = 10,
    Long = 11,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum LdcConstant<'class> {
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(Cow<'class, JavaStr>),
    Class(Cow<'class, JavaStr>),
    MethodType(Cow<'class, JavaStr>),
    Handle(Handle<'class>),
    ConstantDynamic(ConstantDynamic<'class>),
}
