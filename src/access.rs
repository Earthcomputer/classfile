use bitflags::bitflags;

bitflags! {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
    pub struct ClassAccess: u16 {
        const Public = 0x0001;
        const Final = 0x0010;
        const Super = 0x0020;
        const Interface = 0x0200;
        const Abstract = 0x0400;
        const Synthetic = 0x1000;
        const Annotation = 0x2000;
        const Enum = 0x4000;
        const Module = 0x8000;
    }
}
