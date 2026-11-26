#[path = "../src/elf.rs"]
mod elf;

const USER_START: u64 = 0x4000_0000_0000;
const USER_END: u64 = USER_START + 0x10_000;

fn valid_image() -> Vec<u8> {
    let mut image = vec![0u8; 0x1001];
    image[0..4].copy_from_slice(b"\x7fELF");
    image[4] = 2;
    image[5] = 1;
    image[6] = 1;
    image[16..18].copy_from_slice(&2u16.to_le_bytes());
    image[18..20].copy_from_slice(&62u16.to_le_bytes());
    image[20..24].copy_from_slice(&1u32.to_le_bytes());
    image[24..32].copy_from_slice(&USER_START.to_le_bytes());
    image[32..40].copy_from_slice(&64u64.to_le_bytes());
    image[52..54].copy_from_slice(&64u16.to_le_bytes());
    image[54..56].copy_from_slice(&56u16.to_le_bytes());
    image[56..58].copy_from_slice(&1u16.to_le_bytes());

    let program_header = 64;
    image[program_header..program_header + 4].copy_from_slice(&1u32.to_le_bytes());
    image[program_header + 4..program_header + 8].copy_from_slice(&5u32.to_le_bytes());
    image[program_header + 8..program_header + 16].copy_from_slice(&0x1000u64.to_le_bytes());
    image[program_header + 16..program_header + 24].copy_from_slice(&USER_START.to_le_bytes());
    image[program_header + 32..program_header + 40].copy_from_slice(&1u64.to_le_bytes());
    image[program_header + 40..program_header + 48].copy_from_slice(&0x1000u64.to_le_bytes());
    image[program_header + 48..program_header + 56].copy_from_slice(&0x1000u64.to_le_bytes());
    image[0x1000] = 0xC3;
    image
}

#[test]
fn accepts_a_bounded_executable_load_segment() {
    let plan = elf::validate_image(&valid_image(), USER_START, USER_END).unwrap();
    assert_eq!(plan.entry, USER_START);
    assert_eq!(plan.program_header_count, 1);
}

#[test]
fn rejects_truncated_segment_file_data() {
    let mut image = valid_image();
    image.truncate(0x1000);
    assert_eq!(
        elf::validate_image(&image, USER_START, USER_END),
        Err(elf::ValidationError::InvalidImage)
    );
}

#[test]
fn rejects_segments_outside_user_region() {
    assert_eq!(
        elf::validate_image(&valid_image(), USER_START, USER_START + 0x800),
        Err(elf::ValidationError::OutOfRange)
    );
}

#[test]
fn rejects_entry_points_outside_executable_segments() {
    let mut image = valid_image();
    image[24..32].copy_from_slice(&(USER_START + 0x1000).to_le_bytes());
    assert_eq!(
        elf::validate_image(&image, USER_START, USER_END),
        Err(elf::ValidationError::InvalidImage)
    );
}
