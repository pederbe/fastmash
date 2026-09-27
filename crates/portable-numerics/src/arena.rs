use crate::NumericFailure;
use fastmash_numeric_contract::{Raw80, Value80};
use std::alloc::{Layout, alloc, dealloc};
use std::ptr::NonNull;

pub(crate) const ARENA_BYTES: usize = 131_072;
const ARENA_CELLS: usize = ARENA_BYTES / size_of::<u64>();
pub(crate) const CACHE_ENDPOINTS: usize = 10;
pub(crate) const WORK_ENDPOINTS: usize = 48;
pub(crate) const STORED_SLOTS: usize = CACHE_ENDPOINTS + WORK_ENDPOINTS;

const CACHE_MAG_BASE: usize = 0;
const CACHE_DESC_BASE: usize = 90;
const WORK_MAG_BASE: usize = 2_048;
const WORK_DESC_BASE: usize = 2_480;
const EXACT_MAG_BASE: usize = 2_528;
const EXACT_DESC_BASE: usize = 2_579;
const SCALAR_BASE: usize = 2_582;
const PRIMITIVE_BASE: usize = 2_048;
const PRIMITIVE_CELLS: usize = 8;

const MIN_EXP: i32 = -3_100_000;
const MAX_EXP: i32 = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot9(pub(crate) u8);

impl Slot9 {
    pub(crate) const fn cache(interval: u8, endpoint: u8) -> Self {
        Self(interval * 2 + endpoint)
    }

    pub(crate) const fn work(register: u8, endpoint: u8) -> Self {
        Self(CACHE_ENDPOINTS as u8 + register * 2 + endpoint)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot17(pub(crate) u8);

impl Slot17 {
    pub(crate) const X0: Self = Self(0);
    pub(crate) const X1: Self = Self(1);
    pub(crate) const X2: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Desc {
    pub(crate) sign: bool,
    pub(crate) len: u8,
    pub(crate) exp: i32,
}

impl Desc {
    pub(crate) const fn zero(sign: bool) -> Self {
        Self {
            sign,
            len: 0,
            exp: 0,
        }
    }
}

pub(crate) struct Arena {
    pointer: NonNull<u64>,
}

#[derive(Clone, Copy)]
pub(crate) struct PrimitiveCell(u8);

impl PrimitiveCell {
    pub(crate) const V0: Self = Self(0);
    pub(crate) const V1: Self = Self(1);
    pub(crate) const V2: Self = Self(2);
    pub(crate) const V3: Self = Self(3);
    pub(crate) const V4: Self = Self(4);
    pub(crate) const V5: Self = Self(5);
    pub(crate) const V6: Self = Self(6);
    pub(crate) const V7: Self = Self(7);
}

pub(crate) struct PrimitiveCells<'a> {
    arena: &'a mut Arena,
}

impl PrimitiveCells<'_> {
    pub(crate) fn get(&self, cell: PrimitiveCell) -> Result<u64, NumericFailure> {
        self.arena
            .cells()
            .get(PRIMITIVE_BASE + usize::from(cell.0))
            .copied()
            .ok_or(NumericFailure::Invariant)
    }

    pub(crate) fn set(&mut self, cell: PrimitiveCell, value: u64) -> Result<(), NumericFailure> {
        *self
            .arena
            .cells_mut()
            .get_mut(PRIMITIVE_BASE + usize::from(cell.0))
            .ok_or(NumericFailure::Invariant)? = value;
        Ok(())
    }
}

impl Arena {
    pub(crate) fn new() -> Result<Self, NumericFailure> {
        Self::new_with(|| {
            // SAFETY: the layout is nonzero and has u64 alignment. Null is
            // handled by the caller and no reference is formed here.
            unsafe { alloc(Self::layout()) }
        })
    }

    fn new_with(allocate: impl FnOnce() -> *mut u8) -> Result<Self, NumericFailure> {
        let pointer = NonNull::new(allocate().cast::<u64>()).ok_or(NumericFailure::Allocation)?;
        let arena = Self { pointer };
        // SAFETY: the successful allocation has exactly ARENA_CELLS u64 cells.
        unsafe { arena.pointer.as_ptr().write_bytes(0, ARENA_CELLS) };
        Ok(arena)
    }

    const fn layout() -> Layout {
        // SAFETY: both values are nonzero powers/multiples accepted by Layout.
        unsafe { Layout::from_size_align_unchecked(ARENA_BYTES, align_of::<u64>()) }
    }

    fn cells(&self) -> &[u64] {
        // SAFETY: pointer owns a live ARENA_CELLS allocation for self's life.
        unsafe { std::slice::from_raw_parts(self.pointer.as_ptr(), ARENA_CELLS) }
    }

    fn cells_mut(&mut self) -> &mut [u64] {
        // SAFETY: &mut self gives exclusive access to the live allocation.
        unsafe { std::slice::from_raw_parts_mut(self.pointer.as_ptr(), ARENA_CELLS) }
    }

    fn slot9_offset(slot: Slot9) -> Result<usize, NumericFailure> {
        let index = usize::from(slot.0);
        if index < CACHE_ENDPOINTS {
            Ok(CACHE_MAG_BASE + index * 9)
        } else if index < STORED_SLOTS {
            Ok(WORK_MAG_BASE + (index - CACHE_ENDPOINTS) * 9)
        } else {
            Err(NumericFailure::Invariant)
        }
    }

    fn slot9_desc(slot: Slot9) -> Result<usize, NumericFailure> {
        let index = usize::from(slot.0);
        if index < CACHE_ENDPOINTS {
            Ok(CACHE_DESC_BASE + index)
        } else if index < STORED_SLOTS {
            Ok(WORK_DESC_BASE + index - CACHE_ENDPOINTS)
        } else {
            Err(NumericFailure::Invariant)
        }
    }

    fn slot17_offset(slot: Slot17) -> Result<usize, NumericFailure> {
        (slot.0 < 3)
            .then_some(EXACT_MAG_BASE + usize::from(slot.0) * 17)
            .ok_or(NumericFailure::Invariant)
    }

    fn slot17_desc(slot: Slot17) -> Result<usize, NumericFailure> {
        (slot.0 < 3)
            .then_some(EXACT_DESC_BASE + usize::from(slot.0))
            .ok_or(NumericFailure::Invariant)
    }

    fn decode(raw: u64, width: u8) -> Result<Desc, NumericFailure> {
        let len = (raw & 0x1f) as u8;
        let sign = raw & 0x20 != 0;
        let normalized = raw & 0x40 != 0;
        let exp = (raw >> 32) as u32 as i32;
        if len > width
            || !(MIN_EXP..=MAX_EXP).contains(&exp)
            || (len == 0 && (exp != 0 || normalized))
            || (len != 0 && !normalized)
        {
            return Err(NumericFailure::Invariant);
        }
        Ok(Desc { sign, len, exp })
    }

    fn encode(desc: Desc, width: u8) -> Result<u64, NumericFailure> {
        if desc.len > width
            || !(MIN_EXP..=MAX_EXP).contains(&desc.exp)
            || (desc.len == 0 && desc.exp != 0)
        {
            return Err(NumericFailure::Invariant);
        }
        Ok((u64::from(desc.exp as u32) << 32)
            | u64::from(desc.len)
            | (u64::from(desc.sign) << 5)
            | (u64::from(desc.len != 0) << 6))
    }

    #[inline(always)]
    pub(crate) fn desc9(&self, slot: Slot9) -> Result<Desc, NumericFailure> {
        let desc = Self::decode(
            *self
                .cells()
                .get(Self::slot9_desc(slot)?)
                .ok_or(NumericFailure::Invariant)?,
            9,
        )?;
        if desc.len != 0 {
            let last = self.limb9(slot, usize::from(desc.len) - 1)?;
            if last == 0 {
                return Err(NumericFailure::Invariant);
            }
        }
        Ok(desc)
    }

    #[inline(always)]
    pub(crate) fn desc17(&self, slot: Slot17) -> Result<Desc, NumericFailure> {
        let desc = Self::decode(
            *self
                .cells()
                .get(Self::slot17_desc(slot)?)
                .ok_or(NumericFailure::Invariant)?,
            17,
        )?;
        if desc.len != 0 {
            let last = self.limb17(slot, usize::from(desc.len) - 1)?;
            if last == 0 {
                return Err(NumericFailure::Invariant);
            }
        }
        Ok(desc)
    }

    pub(crate) fn set_desc9(&mut self, slot: Slot9, desc: Desc) -> Result<(), NumericFailure> {
        let index = Self::slot9_desc(slot)?;
        *self
            .cells_mut()
            .get_mut(index)
            .ok_or(NumericFailure::Invariant)? = Self::encode(desc, 9)?;
        Ok(())
    }

    pub(crate) fn set_desc17(&mut self, slot: Slot17, desc: Desc) -> Result<(), NumericFailure> {
        let index = Self::slot17_desc(slot)?;
        *self
            .cells_mut()
            .get_mut(index)
            .ok_or(NumericFailure::Invariant)? = Self::encode(desc, 17)?;
        Ok(())
    }

    pub(crate) fn limb9(&self, slot: Slot9, index: usize) -> Result<u64, NumericFailure> {
        if index >= 9 {
            return Err(NumericFailure::Invariant);
        }
        Ok(*self
            .cells()
            .get(Self::slot9_offset(slot)? + index)
            .ok_or(NumericFailure::Invariant)?)
    }

    pub(crate) fn set_limb9(
        &mut self,
        slot: Slot9,
        index: usize,
        value: u64,
    ) -> Result<(), NumericFailure> {
        if index >= 9 {
            return Err(NumericFailure::Invariant);
        }
        let offset = Self::slot9_offset(slot)? + index;
        *self
            .cells_mut()
            .get_mut(offset)
            .ok_or(NumericFailure::Invariant)? = value;
        Ok(())
    }

    pub(crate) fn limb17(&self, slot: Slot17, index: usize) -> Result<u64, NumericFailure> {
        if index >= 17 {
            return Err(NumericFailure::Invariant);
        }
        Ok(*self
            .cells()
            .get(Self::slot17_offset(slot)? + index)
            .ok_or(NumericFailure::Invariant)?)
    }

    pub(crate) fn set_limb17(
        &mut self,
        slot: Slot17,
        index: usize,
        value: u64,
    ) -> Result<(), NumericFailure> {
        if index >= 17 {
            return Err(NumericFailure::Invariant);
        }
        let offset = Self::slot17_offset(slot)? + index;
        *self
            .cells_mut()
            .get_mut(offset)
            .ok_or(NumericFailure::Invariant)? = value;
        Ok(())
    }

    pub(crate) fn scalar(&self, index: usize) -> Result<u64, NumericFailure> {
        if index >= 6 {
            return Err(NumericFailure::Invariant);
        }
        Ok(*self
            .cells()
            .get(SCALAR_BASE + index)
            .ok_or(NumericFailure::Invariant)?)
    }

    pub(crate) fn set_scalar(&mut self, index: usize, value: u64) -> Result<(), NumericFailure> {
        if index >= 6 {
            return Err(NumericFailure::Invariant);
        }
        *self
            .cells_mut()
            .get_mut(SCALAR_BASE + index)
            .ok_or(NumericFailure::Invariant)? = value;
        Ok(())
    }

    pub(crate) fn clear9(&mut self, slot: Slot9, sign: bool) -> Result<(), NumericFailure> {
        let offset = Self::slot9_offset(slot)?;
        self.cells_mut()
            .get_mut(offset..offset + 9)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        self.set_desc9(slot, Desc::zero(sign))
    }

    pub(crate) fn clear17(&mut self, slot: Slot17) -> Result<(), NumericFailure> {
        let offset = Self::slot17_offset(slot)?;
        self.cells_mut()
            .get_mut(offset..offset + 17)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        self.set_desc17(slot, Desc::zero(false))
    }

    pub(crate) fn reset_disposable(&mut self) -> Result<(), NumericFailure> {
        self.cells_mut()
            .get_mut(2_048..2_408)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        self.cells_mut()
            .get_mut(2_480..2_520)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        self.cells_mut()
            .get_mut(2_528..2_588)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        Ok(())
    }

    pub(crate) fn release_call_state(&mut self) -> Result<(), NumericFailure> {
        self.cells_mut()
            .get_mut(2_528..2_582)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        self.cells_mut()
            .get_mut(2_585..2_588)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        Ok(())
    }

    pub(crate) fn exact_scope<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, NumericFailure>,
    ) -> Result<T, NumericFailure> {
        let result = operation(self);
        self.cells_mut()
            .get_mut(2_528..2_582)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        result
    }

    pub(crate) fn primitive_value_scope(
        &mut self,
        operation: impl FnOnce(&mut PrimitiveCells<'_>) -> Result<Raw80, NumericFailure>,
    ) -> Result<Value80, NumericFailure> {
        self.reset_disposable()?;
        if !self
            .cells()
            .get(2_048..2_408)
            .ok_or(NumericFailure::Invariant)?
            .iter()
            .chain(
                self.cells()
                    .get(2_480..2_520)
                    .ok_or(NumericFailure::Invariant)?,
            )
            .chain(
                self.cells()
                    .get(2_528..2_588)
                    .ok_or(NumericFailure::Invariant)?,
            )
            .all(|cell| *cell == 0)
        {
            return Err(NumericFailure::Invariant);
        }
        let result = operation(&mut PrimitiveCells { arena: self })
            .and_then(|raw| Value80::from_raw(raw).map_err(|_| NumericFailure::Invariant));
        self.cells_mut()
            .get_mut(PRIMITIVE_BASE..PRIMITIVE_BASE + PRIMITIVE_CELLS)
            .ok_or(NumericFailure::Invariant)?
            .fill(0);
        result
    }

    #[cfg(test)]
    pub(crate) fn allocation_failure() -> Result<Self, NumericFailure> {
        Self::new_with(std::ptr::null_mut)
    }

    #[cfg(test)]
    pub(crate) fn cache_snapshot_for_test(&self) -> [u64; 2_048] {
        let mut snapshot = [0; 2_048];
        snapshot.copy_from_slice(&self.cells()[..2_048]);
        snapshot
    }

    #[cfg(test)]
    pub(crate) fn primitive_cells_clear_for_test(&self) -> bool {
        self.cells()[PRIMITIVE_BASE..PRIMITIVE_BASE + PRIMITIVE_CELLS]
            .iter()
            .all(|cell| *cell == 0)
    }

    #[cfg(test)]
    pub(crate) fn inject_primitive_invariant_for_test(
        &mut self,
    ) -> Result<Value80, NumericFailure> {
        self.primitive_value_scope(|scratch| {
            scratch.set(PrimitiveCell::V7, 1)?;
            Err(NumericFailure::Invariant)
        })
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: this is the matching layout and unique pointer from new_with.
        unsafe { dealloc(self.pointer.as_ptr().cast::<u8>(), Self::layout()) };
    }
}
