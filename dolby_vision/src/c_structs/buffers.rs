use libc::size_t;

pub trait Freeable {
    /// # Safety
    /// The pointers should all be valid.
    unsafe fn free(&self);
}

/// Struct representing a data buffer
#[repr(C)]
pub struct Data {
    /// Pointer to the data buffer
    pub data: *const u8,
    /// Data buffer size
    pub len: size_t,
}

impl From<Vec<u8>> for Data {
    fn from(buf: Vec<u8>) -> Self {
        Data {
            len: buf.len(),
            data: Box::into_raw(buf.into_boxed_slice()) as *const u8,
        }
    }
}

impl From<Vec<bool>> for Data {
    fn from(buf: Vec<bool>) -> Self {
        let res: Vec<u8> = buf.into_iter().map(|e| e as u8).collect();

        Data {
            len: res.len(),
            data: Box::into_raw(res.into_boxed_slice()) as *const u8,
        }
    }
}

impl<const N: usize> From<[bool; N]> for Data {
    fn from(array: [bool; N]) -> Self {
        let res: [u8; N] = array.map(|e| e as u8);

        Data {
            len: array.len(),
            data: Box::into_raw(Box::new(res)) as *const u8,
        }
    }
}

impl Freeable for Data {
    unsafe fn free(&self) {
        if !self.data.is_null() {
            unsafe {
                Vec::from_raw_parts(self.data as *mut u8, self.len, self.len);
            }
        }
    }
}
