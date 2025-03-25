//! The header sections of a BUFR file

use binrw::{BinRead, BinReaderExt};
use serde::Serialize;

use crate::{Descriptor, Error};

/// The header sections of a BUFR file
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct HeaderSections {
    pub indicator_section: IndicatorSection,
    pub identification_section: IdentificationSection,
    pub optional_section: Option<OptionalSection>,
    pub data_description_section: DataDescriptionSection,
}

impl HeaderSections {
    pub fn read<R: BinReaderExt>(mut reader: R) -> Result<Self, Error> {
        // Indicator section
        let indicator_section: IndicatorSection = reader.read_be()?;

        // Identification section
        let identification_section: IdentificationSection = match indicator_section.edition_number {
            3 => reader.read_be::<IdentificationSectionV3>()?.into(),
            4 => reader.read_be::<IdentificationSection>()?,
            _ => {
                return Err(Error::Fatal(format!(
                    "Unsupported edition number {}",
                    indicator_section.edition_number
                )));
            }
        };

        // Optional section
        let optional_section: Option<OptionalSection> =
            match identification_section.flags.has_optional_section {
                true => Some(reader.read_be()?),
                false => None,
            };

        // Data description section
        let data_description_section: DataDescriptionSection = reader.read_be()?;

        Ok(HeaderSections {
            indicator_section,
            identification_section,
            optional_section,
            data_description_section,
        })
    }
}

fn three_bytes_to_u32(bytes: (u8, u8, u8)) -> u32 {
    (bytes.0 as u32) << 16 | (bytes.1 as u32) << 8 | (bytes.2 as u32)
}

/// Indicator section (Section 0)
#[derive(BinRead, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[brw(magic = b"BUFR")]
pub struct IndicatorSection {
    #[br(map = three_bytes_to_u32)]
    pub total_length: u32,
    pub edition_number: u8,
}

/// Identification section (Section 1)
#[derive(BinRead, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct IdentificationSection {
    #[br(map = three_bytes_to_u32)]
    pub section_length: u32,
    pub master_table_number: u8,
    pub centre: u16,
    pub sub_centre: u16,
    pub update_sequence_number: u8,
    pub flags: IdentificationSectionFlags,
    pub data_category: u8,
    pub international_data_sub_category: u8,
    pub local_data_sub_category: u8,
    pub master_table_version: u8,
    pub local_tables_version: u8,
    pub typical_year: u16,
    pub typical_month: u8,
    pub typical_day: u8,
    pub typical_hour: u8,
    pub typical_minute: u8,
    pub typical_second: u8,
    #[br(assert(section_length >= 22, "Identification section (BUFR4) length must be >= 22"))]
    #[br(count = section_length - 22)]
    pub local_use: Vec<u8>,
}

#[derive(BinRead, Debug )]
pub struct IdentificationSectionV3 {
    #[br(map = three_bytes_to_u32)]
    pub section_length: u32,
    pub master_table_number: u8,
    pub sub_centre: u8,
    pub centre: u8,
    pub update_sequence_number: u8,
    pub flags: IdentificationSectionFlags,
    pub data_category: u8,
    pub data_sub_category: u8,
    pub master_table_version: u8,
    pub local_tables_version: u8,
    pub typical_year: u8,
    pub typical_month: u8,
    pub typical_day: u8,
    pub typical_hour: u8,
    pub typical_minute: u8,
    #[br(assert(section_length >= 17, "Identification section (BUFR3) length must be >= 17"))]
    #[br(count = section_length - 17)]
    pub local_use: Vec<u8>,
}

impl From<IdentificationSectionV3> for IdentificationSection {
    fn from(value: IdentificationSectionV3) -> Self {
        Self {
            section_length: value.section_length,
            master_table_number: value.master_table_number,
            centre: value.centre as u16,
            sub_centre: value.sub_centre as u16,
            update_sequence_number: value.update_sequence_number,
            flags: value.flags,
            data_category: value.data_category,
            international_data_sub_category: value.data_sub_category,
            local_data_sub_category: 0,
            master_table_version: value.master_table_version,
            local_tables_version: value.local_tables_version,
            typical_year: value.typical_year as u16,
            typical_month: value.typical_month,
            typical_day: value.typical_day,
            typical_hour: value.typical_hour,
            typical_minute: value.typical_minute,
            typical_second: 0,
            local_use: value.local_use,
        }
    }
}

#[derive(BinRead, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[br(map = |b: u8| 
    Self {
        has_optional_section: b & 0b10000000 != 0,
    }
)]
pub struct IdentificationSectionFlags {
    pub has_optional_section: bool,
}

/// Optional section (Section 2)
#[derive(BinRead, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct OptionalSection {
    #[br(map = three_bytes_to_u32, pad_after = 1)]
    pub section_length: u32,
    #[br(assert(section_length >= 4, "Optional section length must be >= 4"))]
    #[br(count = section_length - 4)]
    pub optional: Vec<u8>,
}

/// Data description section (Section 3)
#[derive(BinRead, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct DataDescriptionSection {
    #[br(map = three_bytes_to_u32, pad_after = 1)]
    pub section_length: u32,
    pub number_of_subsets: u16,
    pub flags: DataDescriptionSectionFlags,
    #[br(assert(section_length >= 7, "Data description section length must be >= 7"))]
    #[br(count = (section_length - 7) / 2)]
    pub descriptors: Vec<Descriptor>,
    #[br(count = section_length as usize - 7 - (2 * descriptors.len()))]
    pub _padding: Vec<u8>,
}

#[derive(BinRead, Debug, Default)]
#[cfg_attr(feature = "serde", derive(Serialize))]
#[br(map = |b: u8| {
    Self {
        is_observed_data: b & 0b10000000 != 0,
        is_compressed: b & 0b01000000 != 0,
    }
})]
pub struct DataDescriptionSectionFlags {
    pub is_observed_data: bool,
    pub is_compressed: bool,
}

/// End section (Section 5)
#[derive(BinRead, Debug)]
#[brw(magic = b"7777")]
pub struct EndSection {}

/// Check if the end section appears in the stream
pub fn ensure_end_section<R: std::io::Read>(edition: u8, reader: &mut R) -> Result<(), Error> {
    if edition == 3 {
        let mut buf: [u8; 1] = [0; 1];
        reader.read_exact(&mut buf)?;
        match buf[0] {
            0x0 => {}
            b'7' => {
                let mut buf: [u8; 3] = [0; 3];
                reader.read_exact(&mut buf)?;
                if &buf != b"777" {
                    return Err(Error::Fatal("Invalid end section".to_string()));
                }
            }
            _ => {
                return Err(Error::Fatal("Invalid end section".to_string()));
            }
        }
    }
    let mut buf: [u8; 4] = [0; 4];
    reader.read_exact(&mut buf)?;
    if &buf != b"7777" {
        return Err(Error::Fatal("Invalid end section".to_string()));
    }
    Ok(())
}
