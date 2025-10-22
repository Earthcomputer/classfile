use crate::{ClassAccess, ClassFileError, ClassFileResult, ConstantPool, LATEST_MAJOR_VERSION};
use java_string::JavaStr;
use std::borrow::Cow;
use std::slice::SliceIndex;

pub struct ClassReader<'class> {
    buffer: ClassBuffer<'class>,
    pub constant_pool: ConstantPool<'class>,
    metadata_start: usize,
}

impl<'class> ClassReader<'class> {
    pub fn new(data: &'class [u8]) -> ClassFileResult<ClassReader<'class>> {
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
        })
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
pub struct InterfacesIterator<'a, 'class> {
    reader: &'a ClassReader<'class>,
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

#[cfg(test)]
mod test {
    use crate::{ClassAccess, ClassReader};
    use classfile_macros::include_class;
    use std::borrow::Cow;

    const HELLO_WORLD_BYTECODE: &[u8] = include_class!("test/HelloWorld.java")[0];

    #[test]
    fn test_hello_world() {
        let reader = ClassReader::new(HELLO_WORLD_BYTECODE).unwrap();
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
