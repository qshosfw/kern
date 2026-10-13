use core::marker::PhantomData;

/// Zero-sized type representing Hertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hertz;

/// Zero-sized type representing Kilohertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kilohertz;

/// Zero-sized type representing Megahertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Megahertz;

/// A strongly typed Frequency wrapped in a unit of measure.
/// Prevents mathematically unsound operations (e.g., adding Hz to MHz).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Frequency<Unit> {
    value: u32,
    _marker: PhantomData<Unit>,
}

impl<U> Frequency<U> {
    #[inline(always)]
    pub const fn new(value: u32) -> Self {
        Self {
            value,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    pub const fn raw(&self) -> u32 {
        self.value
    }
}

// ── Conversions ─────────────────────────────────────────────────────────────

impl Frequency<Megahertz> {
    #[inline(always)]
    pub const fn to_hertz(&self) -> Frequency<Hertz> {
        Frequency::new(self.value * 1_000_000)
    }

    #[inline(always)]
    pub const fn to_kilohertz(&self) -> Frequency<Kilohertz> {
        Frequency::new(self.value * 1_000)
    }
}

impl Frequency<Kilohertz> {
    #[inline(always)]
    pub const fn to_hertz(&self) -> Frequency<Hertz> {
        Frequency::new(self.value * 1_000)
    }
}

// ── Convenience Macros / Helpers ────────────────────────────────────────────

impl From<u32> for Frequency<Hertz> {
    #[inline(always)]
    fn from(hz: u32) -> Self {
        Self::new(hz)
    }
}

impl From<Frequency<Megahertz>> for Frequency<Hertz> {
    #[inline(always)]
    fn from(mhz: Frequency<Megahertz>) -> Self {
        mhz.to_hertz()
    }
}
