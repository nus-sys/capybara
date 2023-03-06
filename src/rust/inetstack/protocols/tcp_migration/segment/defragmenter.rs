// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

//==============================================================================
// Imports
//==============================================================================

use std::{collections::BinaryHeap, cmp::Reverse};

use crate::runtime::memory::{Buffer, DataBuffer};

use super::TcpMigHeader;

//==============================================================================
// Structures
//==============================================================================

/// Wrapper over a TCPMig packet that is ordered only w.r.t. the header's `fragment_offset` field.
#[derive(Debug)]
struct Fragment(TcpMigHeader, Buffer);

pub struct TcpMigDefragmenter {
    fragment_count: Option<usize>,
    fragments: BinaryHeap<Reverse<Fragment>>,
    data_len: usize,
}

//==============================================================================
// Associate Functions
//==============================================================================

impl TcpMigDefragmenter {
    pub fn new() -> Self {
        Self {
            fragment_count: None,
            fragments: BinaryHeap::new(),
            data_len: 0
        }
    }

    /// Returns None if all the fragments have not been received.
    pub fn defragment(&mut self, hdr: TcpMigHeader, buf: Buffer) -> Option<(TcpMigHeader, Buffer)> {
        if let None = self.fragment_count {
            if !hdr.flag_next_fragment {
                let count = hdr.fragment_offset as usize + 1;
                self.fragment_count = Some(count);
                self.fragments.reserve(count - self.fragments.len());
            }
        }
        self.data_len += buf.len();
        self.fragments.push(Reverse(Fragment(hdr, buf)));

        let count = self.fragment_count?;
        if count != self.fragments.len() {
            None
        } else {
            let mut hdr = self.fragments.peek().unwrap().0.0.clone();
            hdr.flag_next_fragment = false;
            hdr.fragment_offset = 0;

            let mut buf: Vec<u8> = Vec::with_capacity(self.data_len);
            while let Some(Reverse(fragment)) = self.fragments.pop() {
                buf.extend_from_slice(&fragment.1);
            }
            
            self.reset();
            Some((hdr, Buffer::Heap(DataBuffer::from_slice(&buf))))
        }
    }

    fn reset(&mut self) {
        self.fragment_count = None;
        self.fragments.clear();
    }
}

//==============================================================================
// Trait Implementations
//==============================================================================

impl PartialEq for Fragment {
    fn eq(&self, other: &Self) -> bool {
        self.0.fragment_offset == other.0.fragment_offset
    }
}

impl Eq for Fragment {}

impl PartialOrd for Fragment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        if self.0.fragment_offset == other.0.fragment_offset {
            Some(Ordering::Equal)
        } else if self.0.fragment_offset > other.0.fragment_offset {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl Ord for Fragment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

//==============================================================================
// Unit Tests
//==============================================================================

#[cfg(test)]
mod test {
    use super::*;
    use crate::runtime::memory::DataBuffer;
    use std::{net::SocketAddrV4, str::FromStr};

    fn tcpmig_header(fragment_offset: u16, flag_next_fragment: bool) -> TcpMigHeader {
        TcpMigHeader {
            origin: SocketAddrV4::from_str("198.0.0.1:20000").unwrap(),
            remote: SocketAddrV4::from_str("18.45.32.67:19465").unwrap(),
            payload_length: 8,
            fragment_offset,
            flag_load: false,
            flag_next_fragment,
            stage: super::super::super::MigrationStage::ConnectionState,
        }
    }

    fn buffer(buf: &[u8]) -> Buffer {
        Buffer::Heap(DataBuffer::from_slice(buf))
    }

    #[test]
    fn test_tcpmig_defragmentation() {
        let mut defragmenter = TcpMigDefragmenter::new();
        assert_eq!(
            defragmenter.defragment(tcpmig_header(1, true), buffer(&[4, 5, 6])),
            None
        );

        assert_eq!(
            defragmenter.defragment(tcpmig_header(2, false), buffer(&[7, 8])),
            None
        );

        assert_eq!(
            defragmenter.defragment(tcpmig_header(0, true), buffer(&[1, 2, 3])),
            Some((
                tcpmig_header(0, false),
                buffer(&[1, 2, 3, 4, 5, 6, 7, 8])
            ))
        );
    }
}
