#![no_std]

//! Shared syscall numbers and register/result layout for the kernel and apps.
//!
//! The planned x86-64 entry places the syscall number in `RAX` and six
//! arguments in `RDI`, `RSI`, `RDX`, `R10`, `R8`, and `R9`. A return value uses
//! `RAX` for `value` and `RDX` for `error`; zero error means success, and a
//! non-zero error is a positive errno value. The CPU entry is not implemented.

pub const SYSCALL_ABI_MAJOR: u16 = 1;
pub const SYSCALL_ABI_MINOR: u16 = 0;
pub const SYSCALL_COUNT: usize = 20;

pub const ENOSYS: u64 = 38;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SyscallCategory {
    Process,
    File,
    Memory,
    Time,
    SystemInfo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u64)]
pub enum SyscallId {
    Exit = 0,
    Write = 1,
    Read = 2,
    Open = 3,
    Close = 4,
    FileRead = 5,
    FileWrite = 6,
    Seek = 7,
    Stat = 8,
    ReadDir = 9,
    MapMemory = 10,
    UnmapMemory = 11,
    HeapAlloc = 12,
    HeapFree = 13,
    ClockGetTime = 14,
    Sleep = 15,
    GetPid = 16,
    Spawn = 17,
    Wait = 18,
    GetAbiVersion = 19,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyscallInfo {
    pub id: SyscallId,
    pub name: &'static str,
    pub category: SyscallCategory,
    pub argument_count: u8,
    pub arguments: [&'static str; 6],
}

pub const SYSCALLS: [SyscallInfo; SYSCALL_COUNT] = [
    SyscallInfo {
        id: SyscallId::Exit,
        name: "exit",
        category: SyscallCategory::Process,
        argument_count: 1,
        arguments: ["status", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Write,
        name: "write",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["fd", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Read,
        name: "read",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["fd", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Open,
        name: "open",
        category: SyscallCategory::File,
        argument_count: 4,
        arguments: ["path_ptr", "path_len", "flags", "mode", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Close,
        name: "close",
        category: SyscallCategory::File,
        argument_count: 1,
        arguments: ["handle", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::FileRead,
        name: "file_read",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["handle", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::FileWrite,
        name: "file_write",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["handle", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Seek,
        name: "seek",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["handle", "offset", "whence", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Stat,
        name: "stat",
        category: SyscallCategory::File,
        argument_count: 3,
        arguments: ["path_ptr", "path_len", "stat_ptr", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ReadDir,
        name: "readdir",
        category: SyscallCategory::File,
        argument_count: 2,
        arguments: ["handle", "entry_ptr", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::MapMemory,
        name: "map_memory",
        category: SyscallCategory::Memory,
        argument_count: 5,
        arguments: ["address", "length", "flags", "handle", "offset", ""],
    },
    SyscallInfo {
        id: SyscallId::UnmapMemory,
        name: "unmap_memory",
        category: SyscallCategory::Memory,
        argument_count: 2,
        arguments: ["address", "length", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::HeapAlloc,
        name: "heap_alloc",
        category: SyscallCategory::Memory,
        argument_count: 2,
        arguments: ["size", "alignment", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::HeapFree,
        name: "heap_free",
        category: SyscallCategory::Memory,
        argument_count: 2,
        arguments: ["address", "size", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ClockGetTime,
        name: "clock_gettime",
        category: SyscallCategory::Time,
        argument_count: 2,
        arguments: ["clock_id", "timespec_ptr", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Sleep,
        name: "sleep",
        category: SyscallCategory::Time,
        argument_count: 1,
        arguments: ["nanoseconds", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetPid,
        name: "getpid",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Spawn,
        name: "spawn",
        category: SyscallCategory::Process,
        argument_count: 4,
        arguments: ["path_ptr", "path_len", "argv_ptr", "flags", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Wait,
        name: "wait",
        category: SyscallCategory::Process,
        argument_count: 3,
        arguments: ["pid", "status_ptr", "options", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetAbiVersion,
        name: "get_abi_version",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SyscallResult {
    pub value: u64,
    pub error: u64,
}

pub fn syscall_info(number: u64) -> Option<&'static SyscallInfo> {
    let index = usize::try_from(number).ok()?;
    let info = SYSCALLS.get(index)?;
    (info.id as usize == index).then_some(info)
}

impl SyscallResult {
    const fn success(value: u64) -> Self {
        Self { value, error: 0 }
    }

    const fn not_implemented() -> Self {
        Self {
            value: u64::MAX,
            error: ENOSYS,
        }
    }
}

pub fn dispatch(number: u64, _arguments: [u64; 6]) -> SyscallResult {
    match syscall_info(number).map(|info| info.id) {
        Some(SyscallId::GetAbiVersion) => {
            let version = (u64::from(SYSCALL_ABI_MAJOR) << 16) | u64::from(SYSCALL_ABI_MINOR);
            SyscallResult::success(version)
        }
        Some(_) | None => SyscallResult::not_implemented(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syscall_table_has_twenty_stable_sequential_ids() {
        assert_eq!(SYSCALLS.len(), 20);
        for (number, info) in SYSCALLS.iter().enumerate() {
            assert_eq!(info.id as usize, number);
            assert!(!info.name.is_empty());
            assert!(info.argument_count <= info.arguments.len() as u8);
            assert!(info.arguments[usize::from(info.argument_count)..]
                .iter()
                .all(|argument| argument.is_empty()));
            assert_eq!(syscall_info(number as u64), Some(info));
        }
    }

    #[test]
    fn syscall_numbers_are_unique_and_out_of_range_numbers_are_rejected() {
        for (index, info) in SYSCALLS.iter().enumerate() {
            assert!(!SYSCALLS[index + 1..]
                .iter()
                .any(|other| other.id == info.id));
        }

        assert_eq!(syscall_info(SYSCALL_COUNT as u64), None);
        assert_eq!(syscall_info(u64::MAX), None);
    }

    #[test]
    fn get_abi_version_returns_major_minor_pair() {
        let result = dispatch(SyscallId::GetAbiVersion as u64, [0; 6]);

        assert_eq!(result.value, 0x0001_0000);
        assert_eq!(result.error, 0);
    }

    #[test]
    fn unavailable_calls_return_not_implemented() {
        for info in &SYSCALLS[..SYSCALL_COUNT - 1] {
            let result = dispatch(info.id as u64, [0; 6]);

            assert_eq!(result.value, u64::MAX, "{}", info.name);
            assert_eq!(result.error, ENOSYS, "{}", info.name);
        }
    }

    #[test]
    fn unknown_call_returns_not_implemented() {
        let result = dispatch(SYSCALL_COUNT as u64, [0; 6]);

        assert_eq!(result.value, u64::MAX);
        assert_eq!(result.error, ENOSYS);
    }
}
