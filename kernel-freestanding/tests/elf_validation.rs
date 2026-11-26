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
fn derives_user_page_permissions_from_segment_flags() {
    let executable = elf::user_page_flags(5, true);
    assert_eq!(executable & 0x7, 0x5);
    assert_eq!(executable >> 63, 0);

    let writable_data = elf::user_page_flags(6, true);
    assert_eq!(writable_data & 0x7, 0x7);
    assert_eq!(writable_data >> 63, 1);

    let data_without_nx = elf::user_page_flags(6, false);
    assert_eq!(data_without_nx & 0x7, 0x7);
    assert_eq!(data_without_nx >> 63, 0);
}

#[test]
fn user_mapping_access_requires_present_user_and_write_permissions() {
    let writable_user_path = [0x7; 4];
    assert!(elf::user_mapping_allows_access(writable_user_path, false));
    assert!(elf::user_mapping_allows_access(writable_user_path, true));

    let readonly_user_page = [0x7, 0x7, 0x7, 0x5];
    assert!(elf::user_mapping_allows_access(readonly_user_page, false));
    assert!(!elf::user_mapping_allows_access(readonly_user_page, true));

    let supervisor_page = [0x7, 0x7, 0x7, 0x3];
    assert!(!elf::user_mapping_allows_access(supervisor_page, false));

    let absent_page = [0x7, 0x7, 0x7, 0x4];
    assert!(!elf::user_mapping_allows_access(absent_page, false));
}

#[test]
fn validates_page_aligned_user_unmap_ranges_and_rounds_length_up() {
    assert_eq!(
        elf::checked_user_unmap_range(0x5000, 0x1001, 0x4000, 0x8000, 0x1000),
        Some((0x2000, 0x7000))
    );
    assert_eq!(
        elf::checked_user_unmap_range(0x5001, 0x1000, 0x4000, 0x8000, 0x1000),
        None
    );
    assert_eq!(
        elf::checked_user_unmap_range(0x3000, 0x1000, 0x4000, 0x8000, 0x1000),
        None
    );
    assert_eq!(
        elf::checked_user_unmap_range(0x7000, 0x1001, 0x4000, 0x8000, 0x1000),
        None
    );
    assert_eq!(
        elf::checked_user_unmap_range(u64::MAX - 0xFFF, 0x2000, 0, u64::MAX, 0x1000),
        None
    );
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

#[test]
fn rejects_writable_executable_segments() {
    let mut image = valid_image();
    image[64 + 4..64 + 8].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        elf::validate_image(&image, USER_START, USER_END),
        Err(elf::ValidationError::InvalidImage)
    );
}

#[test]
fn rejects_wrapped_segment_file_ranges() {
    let mut image = valid_image();
    image[64 + 8..64 + 16].copy_from_slice(&u64::MAX.wrapping_sub(0xFFF).to_le_bytes());
    image[64 + 32..64 + 40].copy_from_slice(&0x2000u64.to_le_bytes());
    image[64 + 40..64 + 48].copy_from_slice(&0x3000u64.to_le_bytes());
    assert_eq!(
        elf::validate_image(&image, USER_START, USER_END),
        Err(elf::ValidationError::InvalidImage)
    );
}

#[test]
fn rejects_overlapping_load_segments() {
    let mut image = valid_image();
    image.resize(0x2001, 0);
    image[56..58].copy_from_slice(&2u16.to_le_bytes());

    let second = 64 + 56;
    image[second..second + 4].copy_from_slice(&1u32.to_le_bytes());
    image[second + 4..second + 8].copy_from_slice(&5u32.to_le_bytes());
    image[second + 8..second + 16].copy_from_slice(&0x2000u64.to_le_bytes());
    image[second + 16..second + 24].copy_from_slice(&USER_START.to_le_bytes());
    image[second + 32..second + 40].copy_from_slice(&1u64.to_le_bytes());
    image[second + 40..second + 48].copy_from_slice(&0x1000u64.to_le_bytes());
    image[second + 48..second + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    assert_eq!(
        elf::validate_image(&image, USER_START, USER_END),
        Err(elf::ValidationError::InvalidImage)
    );
}
