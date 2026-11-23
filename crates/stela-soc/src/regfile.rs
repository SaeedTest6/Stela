//! Register files a model describes span by span: the registers of one span
//! share a width and a role the model gives meaning to. An offset no span
//! covers, or an access of another width, has no described meaning and
//! refuses.
use crate::Result;
use alloc::collections::BTreeMap;
use stela_contracts::bus::Width;
use stela_contracts::refuse;

/// Registers `[start, end)` of `width`, one every `width` bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span<R> {
    pub start: u64,
    pub end: u64,
    pub width: Width,
    pub role: R,
}

impl<R> Span<R> {
    pub const fn new(start: u64, end: u64, width: Width, role: R) -> Self {
        Self {
            start,
            end,
            width,
            role,
        }
    }
}

/// The first two descriptors (by index, lower first) of `spans` that cover a
/// common offset, if any. A model's span table must have none: [`role`]
/// refuses every offset inside such a pair, so a table that fails this check
/// loses those registers at run time. Models assert it in a test so the
/// mistake surfaces when the table is written, not when a guest first touches
/// the register. Empty spans cover nothing and never overlap.
pub const fn first_overlap<R>(spans: &[Span<R>]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < spans.len() {
        let a = &spans[i];
        let mut j = i + 1;
        while j < spans.len() {
            let b = &spans[j];
            if a.start < a.end && b.start < b.end && a.start < b.end && b.start < a.end {
                return Some((i, j));
            }
            j += 1;
        }
        i += 1;
    }
    None
}

/// The role of the register at `offset` among `spans`.
pub fn role<R: Copy>(spans: &[Span<R>], offset: u64, width: Width) -> Result<R> {
    let mut matching = spans
        .iter()
        .filter(|span| (span.start..span.end).contains(&offset));
    let span = matching
        .next()
        .ok_or(refuse!("register without described meaning"))?;
    if matching.next().is_some() {
        return Err(refuse!("overlapping register spans make role ambiguous"));
    }
    if width != span.width {
        return Err(refuse!("register access of another width"));
    }
    if !(offset - span.start).is_multiple_of(width.bytes()) {
        return Err(refuse!("access between registers"));
    }
    Ok(span.role)
}

/// Registers that hold what software last wrote, reading 0 until then.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held(BTreeMap<u64, u64>);

impl Held {
    pub fn get(&self, offset: u64) -> u64 {
        self.0.get(&offset).copied().unwrap_or(0)
    }

    pub fn set(&mut self, offset: u64, value: u64) {
        self.0.insert(offset, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_give_roles_only_at_their_width_and_registers() {
        const SPANS: [Span<u8>; 2] = [
            Span::new(0x10, 0x20, Width::Word, 1),
            Span::new(0x100, 0x110, Width::Double, 2),
        ];
        assert_eq!(role(&SPANS, 0x1c, Width::Word), Ok(1));
        assert_eq!(role(&SPANS, 0x108, Width::Double), Ok(2));
        assert!(role(&SPANS, 0x20, Width::Word).is_err());
        assert!(role(&SPANS, 0x10, Width::Double).is_err());
        assert!(role(&SPANS, 0x104, Width::Word).is_err());

        // A malformed map must not silently pick whichever overlapping span
        // happens to appear first in the descriptor array.
        const OVERLAPPING: [Span<u8>; 2] = [
            Span::new(0x200, 0x210, Width::Word, 3),
            Span::new(0x208, 0x218, Width::Word, 4),
        ];
        assert!(role(&OVERLAPPING, 0x208, Width::Word).is_err());
        const OVERLAPPING_REVERSED: [Span<u8>; 2] = [OVERLAPPING[1], OVERLAPPING[0]];
        assert!(role(&OVERLAPPING_REVERSED, 0x208, Width::Word).is_err());

        let mut held = Held::default();
        assert_eq!(held.get(0x10), 0);
        held.set(0x10, 7);
        assert_eq!(held.get(0x10), 7);
    }

    #[test]
    fn first_overlap_finds_only_spans_sharing_an_offset() {
        // Disjoint, in any order, and adjacent (end == next start) are fine.
        let disjoint: [Span<u8>; 3] = [
            Span::new(0x20, 0x30, Width::Word, 1),
            Span::new(0x00, 0x10, Width::Word, 2),
            Span::new(0x30, 0x40, Width::Double, 3),
        ];
        assert_eq!(first_overlap(&disjoint), None);
        assert_eq!(first_overlap::<u8>(&[]), None);

        // One shared offset (0x2c) is enough, whichever order they appear in.
        let sharing: [Span<u8>; 3] = [
            Span::new(0x00, 0x10, Width::Word, 1),
            Span::new(0x28, 0x30, Width::Word, 2),
            Span::new(0x2c, 0x34, Width::Word, 3),
        ];
        assert_eq!(first_overlap(&sharing), Some((1, 2)));
        let contained: [Span<u8>; 2] = [
            Span::new(0x100, 0x200, Width::Word, 1),
            Span::new(0x140, 0x144, Width::Word, 2),
        ];
        assert_eq!(first_overlap(&contained), Some((0, 1)));

        // An empty or inverted span covers no offset, so it overlaps nothing.
        let empty: [Span<u8>; 3] = [
            Span::new(0x10, 0x10, Width::Word, 1),
            Span::new(0x20, 0x08, Width::Word, 2),
            Span::new(0x00, 0x40, Width::Word, 3),
        ];
        assert_eq!(first_overlap(&empty), None);

        // What first_overlap reports is exactly what role() refuses.
        assert!(role(&sharing, 0x2c, Width::Word).is_err());
        assert_eq!(role(&sharing, 0x28, Width::Word), Ok(2));
    }
}
