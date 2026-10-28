//! Zero-Allocation Record Cursor Interfaces.
//!
//! Provides transactional random-access and streaming iterators across
//! fixed-stride database records without heap allocations.

/// Abstract record cursor for indexed data tables.
pub trait RecordCursor<T> {
    /// Total number of records present in table.
    fn count(&self) -> u16;

    /// Current record index of cursor.
    fn position(&self) -> u16;

    /// Seek cursor to a 0-indexed record position.
    fn seek_record(&mut self, index: u16) -> Result<(), ()>;

    /// Read the record at the current cursor position and advance cursor.
    fn read_record(&mut self, out: &mut T) -> Result<(), ()>;

    /// Write the record at the current cursor position.
    fn write_record(&mut self, record: &T) -> Result<(), ()>;
}

/// In-memory slice record cursor for testing or memory-mapped storage.
pub struct SliceRecordCursor<'a, T> {
    slice: &'a mut [T],
    pos: u16,
}

impl<'a, T: Copy> SliceRecordCursor<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Self {
        Self { slice, pos: 0 }
    }
}

impl<'a, T: Copy> RecordCursor<T> for SliceRecordCursor<'a, T> {
    fn count(&self) -> u16 {
        self.slice.len() as u16
    }

    fn position(&self) -> u16 {
        self.pos
    }

    fn seek_record(&mut self, index: u16) -> Result<(), ()> {
        if (index as usize) < self.slice.len() {
            self.pos = index;
            Ok(())
        } else {
            Err(())
        }
    }

    fn read_record(&mut self, out: &mut T) -> Result<(), ()> {
        if (self.pos as usize) < self.slice.len() {
            *out = self.slice[self.pos as usize];
            self.pos = self.pos.saturating_add(1);
            Ok(())
        } else {
            Err(())
        }
    }

    fn write_record(&mut self, record: &T) -> Result<(), ()> {
        if (self.pos as usize) < self.slice.len() {
            self.slice[self.pos as usize] = *record;
            Ok(())
        } else {
            Err(())
        }
    }
}
