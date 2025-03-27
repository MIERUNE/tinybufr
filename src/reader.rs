//! Reader for the data section of BUFR files

use std::io::Read;

use binrw::{BinRead, BinReaderExt};
use bitstream_io::{BigEndian, BitRead, BitReader};

use crate::sections::DataDescriptionSection;
use crate::tables::{TableBEntry, TableDEntry, Tables};
use crate::{Error, ResolvedDescriptor, XY, resolve_descriptors};

pub struct DataReader<'a, R: Read> {
    data_spec: &'a DataSpec<'a>,
    current_subset_idx: u16,
    reader: BitReader<R, BigEndian>,
    stack: smallvec::SmallVec<[StackEntry<'a>; 8]>,
    temporary_operator: Option<XY>,
    scale_offset: i8,
}

#[derive(Debug)]
pub struct DataSpec<'a> {
    pub number_of_subsets: u16,
    pub is_compressed: bool,
    pub root_descriptors: Vec<ResolvedDescriptor<'a>>,
}

impl<'a> DataSpec<'a> {
    pub fn from_data_description(
        dds: &'a DataDescriptionSection,
        tables: &'a Tables,
    ) -> Result<Self, Error> {
        Ok(Self {
            number_of_subsets: dds.number_of_subsets,
            is_compressed: dds.flags.is_compressed,
            root_descriptors: resolve_descriptors(tables, &dds.descriptors)?,
        })
    }
}

impl<'a, R: BinReaderExt> DataReader<'a, R> {
    pub fn new(
        mut reader: R,
        spec: impl Into<&'a DataSpec<'a>>,
    ) -> Result<DataReader<'a, R>, Error> {
        let spec = spec.into();
        let _data_section_header: DataSectionHeader = reader.read_be()?;
        Ok(DataReader {
            data_spec: spec,
            current_subset_idx: 0,
            reader: BitReader::endian(reader, BigEndian),
            stack: smallvec::SmallVec::new(),
            temporary_operator: None,
            scale_offset: 0,
        })
    }
}

struct StackEntry<'a> {
    descriptors: &'a [ResolvedDescriptor<'a>],
    next: u16,
    entry_type: StackEntryType,
}

enum StackEntryType {
    Sequence,
    Replication { remaining: u16, in_item: bool },
}

impl<'a> StackEntry<'a> {
    fn new_sequence(descriptors: &'a [ResolvedDescriptor<'a>]) -> Self {
        Self {
            descriptors,
            next: 0,
            entry_type: StackEntryType::Sequence,
        }
    }

    fn new_replication(descriptors: &'a [ResolvedDescriptor<'a>], count: u16) -> Self {
        Self {
            descriptors,
            next: descriptors.len() as u16,
            entry_type: StackEntryType::Replication {
                remaining: count,
                in_item: false,
            },
        }
    }
}

fn three_bytes_to_u32(bytes: (u8, u8, u8)) -> u32 {
    (bytes.0 as u32) << 16 | (bytes.1 as u32) << 8 | (bytes.2 as u32)
}

/// The header of the data section (Section 4)
#[derive(BinRead, Debug)]
pub struct DataSectionHeader {
    #[br(map = three_bytes_to_u32, pad_after = 1)]
    pub section_length: u32,
}

#[derive(Debug)]
pub enum DataEvent {
    SubsetStart(u16),
    SubsetEnd,
    CompressedStart,
    ReplicationStart {
        idx: u16,
        count: u16,
    },
    ReplicationItemStart,
    ReplicationItemEnd,
    ReplicationEnd,
    SequenceStart {
        idx: u16,
        xy: XY,
    },
    SequenceEnd,
    OperatorHandled {
        idx: u16,
        x: u8,
        value: i32,
    },
    Data {
        idx: u16,
        xy: XY,
        value: Value,
    },
    CompressedData {
        idx: u16,
        xy: XY,
        values: Vec<Value>,
    },
    Eof,
}

#[derive(Clone)]
pub enum Value {
    Missing,
    Decimal(i32, i8),
    Integer(i32),
    String(String),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Missing => write!(f, "Missing"),
            &Value::Decimal(v, s) => {
                write!(
                    f,
                    "{:.1$}",
                    v as f64 * 10f64.powi(s as i32),
                    if s < 0 { -s } else { 0 } as usize
                )
            }
            Value::Integer(v) => write!(f, "{}", v),
            Value::String(s) => write!(f, "\"{}\"", s),
        }
    }
}

impl<'a, R: Read> DataReader<'a, R> {
    pub fn read_event(&mut self) -> Result<DataEvent, Error> {
        if self.stack.is_empty() {
            if self.data_spec.is_compressed {
                if self.current_subset_idx > 0 {
                    return Ok(DataEvent::Eof);
                }
            } else if self.current_subset_idx == self.data_spec.number_of_subsets {
                return Ok(DataEvent::Eof);
            }

            self.stack
                .push(StackEntry::new_sequence(&self.data_spec.root_descriptors));
            let subset_idx = self.current_subset_idx;
            self.current_subset_idx += 1;
            if self.data_spec.is_compressed {
                return Ok(DataEvent::CompressedStart);
            } else {
                return Ok(DataEvent::SubsetStart(subset_idx));
            }
        }
        self.process_next_descriptor()
    }

    fn process_next_descriptor(&mut self) -> Result<DataEvent, Error> {
        let top = self.stack.last_mut().expect("Stack should not be empty");
        if let StackEntryType::Replication { remaining, in_item } = &mut top.entry_type {
            if top.next as usize >= top.descriptors.len() {
                if *in_item {
                    *in_item = false;
                    return Ok(DataEvent::ReplicationItemEnd);
                }
                if *remaining > 0 {
                    *remaining -= 1;
                    top.next = 0;
                    *in_item = true;
                    return Ok(DataEvent::ReplicationItemStart);
                } else {
                    self.stack.pop();
                    return Ok(DataEvent::ReplicationEnd);
                }
            }
        };

        if top.next as usize >= top.descriptors.len() {
            self.stack.pop();
            return match (self.stack.last(), self.data_spec.is_compressed) {
                (Some(_), _) => Ok(DataEvent::SequenceEnd),
                (None, true) => Ok(DataEvent::Eof),
                (None, false) => Ok(DataEvent::SubsetEnd),
            };
        }

        let descriptors = &top.descriptors;
        let current_desc = &descriptors[top.next as usize];
        let idx = top.next;
        top.next += 1;
        match current_desc {
            ResolvedDescriptor::Data(b) => self.handle_data_descriptor(idx, b),
            ResolvedDescriptor::Replication {
                y,
                descriptors,
                delayed_bits,
            } => self.handle_replication_descriptor(idx, *y, descriptors, *delayed_bits),
            ResolvedDescriptor::Operator(xy) => self.handle_operator_descriptor(idx, *xy),
            ResolvedDescriptor::Sequence(d, elements) => {
                self.handle_sequence_descriptor(idx, d, elements)
            }
        }
    }

    // f = 0
    fn handle_data_descriptor(&mut self, idx: u16, b: &TableBEntry) -> Result<DataEvent, Error> {
        let (bit_width, ref_value, scale) = (
            b.bits as u32,
            b.reference_value,
            (b.scale as i16 + self.scale_offset as i16) as i8,
        );
        match bit_width {
            0..=32 => {
                if self.data_spec.is_compressed {
                    let local_ref_value: u32 = self.reader.read(bit_width)?;
                    let nbinc: u8 = self.reader.read(6)?;

                    Ok(DataEvent::CompressedData {
                        idx,
                        xy: b.xy,
                        values: if nbinc == 0 {
                            // All values are the same if nbinc == 0
                            let v = if local_ref_value == ((1u64 << bit_width) - 1) as u32 {
                                Value::Missing
                            } else if scale == 0 {
                                Value::Integer(local_ref_value as i32 + ref_value)
                            } else {
                                Value::Decimal(
                                    (local_ref_value as i64 + ref_value as i64) as i32,
                                    -scale,
                                )
                            };
                            vec![v; self.data_spec.number_of_subsets as usize]
                        } else {
                            (0..self.data_spec.number_of_subsets)
                                .map(|_| {
                                    let inc: u32 = self.reader.read(nbinc as u32)?;
                                    let v_raw = local_ref_value + inc;
                                    Ok(if v_raw == ((1u64 << bit_width) - 1) as u32 {
                                        Value::Missing
                                    } else if scale == 0 {
                                        Value::Integer(v_raw as i32 + ref_value)
                                    } else {
                                        Value::Decimal(
                                            (v_raw as i64 + ref_value as i64) as i32,
                                            -scale,
                                        )
                                    })
                                })
                                .collect::<std::io::Result<Vec<Value>>>()?
                        },
                    })
                } else {
                    let v_raw: u32 = self.reader.read(bit_width)?;
                    let value = if v_raw == ((1u64 << bit_width) - 1) as u32 {
                        Value::Missing
                    } else if scale == 0 {
                        Value::Integer(v_raw as i32 + ref_value)
                    } else {
                        Value::Decimal((v_raw as i64 + ref_value as i64) as i32, -scale)
                    };
                    Ok(DataEvent::Data {
                        idx,
                        xy: b.xy,
                        value,
                    })
                }
            }
            _ if bit_width % 8 == 0 => {
                let Ok(s) = String::from_utf8(self.reader.read_to_vec((bit_width / 8) as usize)?)
                else {
                    return Err(Error::Fatal(format!(
                        "Failed to parse character string with bit width {}",
                        bit_width
                    )));
                };
                if self.data_spec.is_compressed {
                    Err(Error::NotSupported(
                        "Compressed data for characters not implemented yet".to_string(),
                    ))
                } else {
                    Ok(DataEvent::Data {
                        idx,
                        xy: b.xy,
                        value: Value::String(s),
                    })
                }
            }
            _ => Err(Error::Fatal(format!("Unsupported bit width {}", bit_width))),
        }
    }

    // f = 1
    fn handle_replication_descriptor(
        &mut self,
        idx: u16,
        y: u8,
        elements: &'a [ResolvedDescriptor<'_>],
        delayed_bits: u8,
    ) -> Result<DataEvent, Error> {
        let count = match y {
            0 => self.reader.read::<u16>(delayed_bits as u32)?,
            _ => y as u16,
        };
        self.stack
            .push(StackEntry::new_replication(elements, count));
        Ok(DataEvent::ReplicationStart { idx, count })
    }

    // f = 2
    fn handle_operator_descriptor(&mut self, idx: u16, xy: XY) -> Result<DataEvent, Error> {
        match (xy.x, xy.y) {
            (2, 0) => self.scale_offset = 0,
            (2, y) => self.scale_offset = ((y as i16) - 128) as i8,
            (6, _) => self.temporary_operator = Some(xy),
            _ => {
                return Err(Error::NotSupported(format!(
                    "Operator descriptor {:#?} not supported yet.",
                    xy
                )));
            }
        }
        Ok(DataEvent::OperatorHandled {
            idx,
            x: xy.x,
            value: xy.y as i32,
        })
    }

    // f = 3
    fn handle_sequence_descriptor(
        &mut self,
        idx: u16,
        d: &TableDEntry,
        elements: &'a [ResolvedDescriptor<'_>],
    ) -> Result<DataEvent, Error> {
        self.stack.push(StackEntry::new_sequence(elements));
        Ok(DataEvent::SequenceStart { idx, xy: d.xy })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_fmt() {
        assert_eq!(format!("{:?}", Value::Missing), "Missing");
        assert_eq!(format!("{:?}", Value::Decimal(1234, -2)), "12.34");
        assert_eq!(format!("{:?}", Value::Decimal(1234, 2)), "123400");
        assert_eq!(format!("{:?}", Value::Integer(42)), "42");
        assert_eq!(
            format!("{:?}", Value::String("Hello".to_string())),
            "\"Hello\""
        );
    }
}
