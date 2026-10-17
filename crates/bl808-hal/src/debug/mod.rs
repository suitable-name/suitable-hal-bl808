use core::fmt::Debug as FmtDebug;

pub struct Debug {
    _inner: (),
}

impl Debug {
    pub fn new() -> Self {
        Self { _inner: () }
    }
}

impl Default for Debug {
    fn default() -> Self {
        Self::new()
    }
}

impl FmtDebug for Debug {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Debug").finish()
    }
}
