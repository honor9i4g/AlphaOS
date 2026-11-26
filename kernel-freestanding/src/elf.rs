#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    InvalidImage,
    OutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadPlan {
    pub entry: u64,
    pub program_header_offset: usize,
    pub program_header_count: usize,
}

#[derive(Clone, Copy)]
pub struct ProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub alignment: u64,
}

const ELF_HEADER_SIZE: usize = 64;
const PROGRAM_HEADER_SIZE: usize = 56;
const MAX_PROGRAM_HEADERS: usize = 128;
const PT_LOAD: u32 = 1;
const PF_WRITE: u32 = 2;
const PF_EXECUTE: u32 = 1;

pub fn validate_image(
    image: &[u8],
    user_start: u64,
    user_end: u64,
) -> Result<LoadPlan, ValidationError> {
    if image.len() < ELF_HEADER_SIZE
        || image.get(0..4) != Some(b"\x7fELF")
        || image[4] != 2
        || image[5] != 1
        || image[6] != 1
        || read_u16(image, 16)? != 2
        || read_u16(image, 18)? != 62
        || read_u32(image, 20)? != 1
        || usize::from(read_u16(image, 52)?) != ELF_HEADER_SIZE
        || usize::from(read_u16(image, 54)?) != PROGRAM_HEADER_SIZE
    {
        return Err(ValidationError::InvalidImage);
    }

    let entry = read_u64(image, 24)?;
    let program_header_offset =
        usize::try_from(read_u64(image, 32)?).map_err(|_| ValidationError::InvalidImage)?;
    let program_header_count = usize::from(read_u16(image, 56)?);
    let table_size = program_header_count
        .checked_mul(PROGRAM_HEADER_SIZE)
        .ok_or(ValidationError::InvalidImage)?;
    let table_end = program_header_offset
        .checked_add(table_size)
        .ok_or(ValidationError::InvalidImage)?;
    if program_header_offset < ELF_HEADER_SIZE
        || program_header_count == 0
        || program_header_count > MAX_PROGRAM_HEADERS
        || table_end > image.len()
    {
        return Err(ValidationError::InvalidImage);
    }

    let plan = LoadPlan {
        entry,
        program_header_offset,
        program_header_count,
    };
    let mut has_load_segment = false;
    let mut entry_is_executable = false;

    for index in 0..program_header_count {
        let segment = program_header(image, plan, index)?;
        if segment.kind != PT_LOAD {
            continue;
        }
        if segment.file_size > segment.memory_size
            || segment.flags & !7 != 0
            || segment.flags & (PF_WRITE | PF_EXECUTE) == PF_WRITE | PF_EXECUTE
        {
            return Err(ValidationError::InvalidImage);
        }
        if segment.alignment > 1
            && (!segment.alignment.is_power_of_two()
                || segment.virtual_address % segment.alignment
                    != segment.offset % segment.alignment)
        {
            return Err(ValidationError::InvalidImage);
        }
        if segment.virtual_address & 0xFFF != segment.offset & 0xFFF {
            return Err(ValidationError::InvalidImage);
        }
        let file_end = segment
            .offset
            .checked_add(segment.file_size)
            .ok_or(ValidationError::InvalidImage)?;
        if file_end > image.len() as u64 {
            return Err(ValidationError::InvalidImage);
        }
        if segment.memory_size == 0 {
            continue;
        }

        let memory_end = segment
            .virtual_address
            .checked_add(segment.memory_size)
            .ok_or(ValidationError::InvalidImage)?;
        let page_start = segment.virtual_address & !0xFFF;
        let page_end = memory_end
            .checked_add(0xFFF)
            .ok_or(ValidationError::InvalidImage)?
            & !0xFFF;
        if page_start < user_start || page_end > user_end {
            return Err(ValidationError::OutOfRange);
        }
        has_load_segment = true;
        if segment.flags & PF_EXECUTE != 0
            && entry >= segment.virtual_address
            && entry < memory_end
        {
            entry_is_executable = true;
        }

        for previous_index in 0..index {
            let previous = program_header(image, plan, previous_index)?;
            if previous.kind != PT_LOAD || previous.memory_size == 0 {
                continue;
            }
            let previous_end = previous
                .virtual_address
                .checked_add(previous.memory_size)
                .ok_or(ValidationError::InvalidImage)?;
            let previous_page_start = previous.virtual_address & !0xFFF;
            let previous_page_end = previous_end
                .checked_add(0xFFF)
                .ok_or(ValidationError::InvalidImage)?
                & !0xFFF;
            if page_start < previous_page_end && previous_page_start < page_end {
                return Err(ValidationError::InvalidImage);
            }
        }
    }

    if !has_load_segment || !entry_is_executable {
        return Err(ValidationError::InvalidImage);
    }
    Ok(plan)
}

pub fn program_header(
    image: &[u8],
    plan: LoadPlan,
    index: usize,
) -> Result<ProgramHeader, ValidationError> {
    if index >= plan.program_header_count {
        return Err(ValidationError::InvalidImage);
    }
    let offset = plan
        .program_header_offset
        .checked_add(
            index
                .checked_mul(PROGRAM_HEADER_SIZE)
                .ok_or(ValidationError::InvalidImage)?,
        )
        .ok_or(ValidationError::InvalidImage)?;
    let end = offset
        .checked_add(PROGRAM_HEADER_SIZE)
        .ok_or(ValidationError::InvalidImage)?;
    if end > image.len() {
        return Err(ValidationError::InvalidImage);
    }
    Ok(ProgramHeader {
        kind: read_u32(image, offset)?,
        flags: read_u32(image, offset + 4)?,
        offset: read_u64(image, offset + 8)?,
        virtual_address: read_u64(image, offset + 16)?,
        file_size: read_u64(image, offset + 32)?,
        memory_size: read_u64(image, offset + 40)?,
        alignment: read_u64(image, offset + 48)?,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ValidationError> {
    let end = offset
        .checked_add(2)
        .ok_or(ValidationError::InvalidImage)?;
    let data: [u8; 2] = bytes
        .get(offset..end)
        .ok_or(ValidationError::InvalidImage)?
        .try_into()
        .map_err(|_| ValidationError::InvalidImage)?;
    Ok(u16::from_le_bytes(data))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ValidationError> {
    let end = offset
        .checked_add(4)
        .ok_or(ValidationError::InvalidImage)?;
    let data: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(ValidationError::InvalidImage)?
        .try_into()
        .map_err(|_| ValidationError::InvalidImage)?;
    Ok(u32::from_le_bytes(data))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ValidationError> {
    let end = offset
        .checked_add(8)
        .ok_or(ValidationError::InvalidImage)?;
    let data: [u8; 8] = bytes
        .get(offset..end)
        .ok_or(ValidationError::InvalidImage)?
        .try_into()
        .map_err(|_| ValidationError::InvalidImage)?;
    Ok(u64::from_le_bytes(data))
}
