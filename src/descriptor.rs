//! Descriptors (FXY)

use std::fmt::Debug;

use binrw::BinRead;
use serde::Serialize;

use crate::{
    Error,
    tables::{TableBEntry, TableDEntry, Tables},
};

/// Descriptor (FXY)
#[derive(BinRead, Hash, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[br(map = |x: u16| Descriptor {
    f: (x >> 14) as u8,
    x: ((x >> 8) & 0x3f) as u8,
    y: (x & 0xff) as u8
})]
pub struct Descriptor {
    pub f: u8,
    pub x: u8,
    pub y: u8,
}

impl Debug for Descriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Descriptor {0:1}{1:02}{2:03}", self.f, self.x, self.y)
    }
}

impl Descriptor {
    pub fn xy(&self) -> XY {
        XY {
            x: self.x,
            y: self.y,
        }
    }
}

#[derive(Hash, Debug, Clone, Copy, Eq, PartialEq)]
pub struct XY {
    pub x: u8,
    pub y: u8,
}

#[derive(Debug)]
pub enum ResolvedDescriptor<'a> {
    Data(&'a TableBEntry),
    Replication(XY),
    Operator(XY),
    Sequence(&'a TableDEntry, Vec<ResolvedDescriptor<'a>>),
}

impl<'a> ResolvedDescriptor<'a> {
    pub fn from_descriptor(desc: &Descriptor, tables: &Tables) -> Result<Self, Error> {
        Ok(match desc.f {
            0 => {
                let Some(b) = tables.table_b.get(&desc.xy()) else {
                    return Err(Error::Fatal(format!(
                        "Table B entry not found for xy: {:?}",
                        desc.xy()
                    )));
                };
                ResolvedDescriptor::Data(b)
            }
            1 => ResolvedDescriptor::Replication(desc.xy()),
            2 => ResolvedDescriptor::Operator(desc.xy()),
            3 => {
                let Some(d) = tables.table_d.get(&desc.xy()) else {
                    return Err(Error::Fatal(format!(
                        "Table D entry not found for xy: {:?}",
                        desc.xy()
                    )));
                };
                let resolved_elements = resolve_descriptors(tables, d.elements)?;
                ResolvedDescriptor::Sequence(d, resolved_elements)
            }
            _ => {
                return Err(Error::Fatal(format!(
                    "Table B entry not found for xy: {:?}",
                    desc.xy()
                )));
            }
        })
    }
}

pub(crate) fn resolve_descriptors<'a>(
    tables: &Tables,
    descriptors: &'a [Descriptor],
) -> Result<Vec<ResolvedDescriptor<'a>>, Error> {
    descriptors
        .iter()
        .map(|desc| ResolvedDescriptor::from_descriptor(desc, tables))
        .collect::<Result<Vec<ResolvedDescriptor<'a>>, _>>()
}
