#![no_std]

//! Shared syscall numbers and result layout for the kernel and future apps.
//!
//! The planned x86-64 entry places the syscall number in `RAX` and six
//! arguments in `RDI`, `RSI`, `RDX`, `R10`, `R8`, and `R9`. The result uses
//! `RAX` for `value` and `RDX` for `error`. The CPU entry is not implemented.

pub const SYSCALL_ABI_MAJOR: u16 = 1;
pub const SYSCALL_ABI_MINOR: u16 = 0;
pub const SYSCALL_COUNT: usize = 20;

pub const ENOSYS: u64 = 38;

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
}

pub const SYSCALLS: [SyscallInfo; SYSCALL_COUNT] = [
    SyscallInfo {
        id: SyscallId::Exit,
        name: "exit",
    },
    SyscallInfo {
        id: SyscallId::Write,
        name: "write",
    },
    SyscallInfo {
        id: SyscallId::Read,
        name: "read",
    },
    SyscallInfo {
        id: SyscallId::Open,
        name: "open",
    },
    SyscallInfo {
        id: SyscallId::Close,
        name: "close",
    },
    SyscallInfo {
        id: SyscallId::FileRead,
        name: "file_read",
    },
    SyscallInfo {
        id: SyscallId::FileWrite,
        name: "file_write",
    },
    SyscallInfo {
        id: SyscallId::Seek,
        name: "seek",
    },
    SyscallInfo {
        id: SyscallId::Stat,
        name: "stat",
    },
    SyscallInfo {
        id: SyscallId::ReadDir,
        name: "readdir",
    },
    SyscallInfo {
        id: SyscallId::MapMemory,
        name: "map_memory",
    },
    SyscallInfo {
        id: SyscallId::UnmapMemory,
        name: "unmap_memory",
    },
    SyscallInfo {
        id: SyscallId::HeapAlloc,
        name: "heap_alloc",
    },
    SyscallInfo {
        id: SyscallId::HeapFree,
        name: "heap_free",
    },
    SyscallInfo {
        id: SyscallId::ClockGetTime,
        name: "clock_gettime",
    },
    SyscallInfo {
        id: SyscallId::Sleep,
        name: "sleep",
    },
    SyscallInfo {
        id: SyscallId::GetPid,
        name: "getpid",
    },
    SyscallInfo {
        id: SyscallId::Spawn,
        name: "spawn",
    },
    SyscallInfo {
        id: SyscallId::Wait,
        name: "wait",
    },
    SyscallInfo {
        id: SyscallId::GetAbiVersion,
        name: "get_abi_version",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SyscallResult {
    pub value: u64,
    pub error: u64,
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
    match syscall_id(number) {
        Some(SyscallId::GetAbiVersion) => {
            let version = (u64::from(SYSCALL_ABI_MAJOR) << 16) | u64::from(SYSCALL_ABI_MINOR);
            SyscallResult::success(version)
        }
        Some(_) | None => SyscallResult::not_implemented(),
    }
}

fn syscall_id(number: u64) -> Option<SyscallId> {
    Some(match number {
        0 => SyscallId::Exit,
        1 => SyscallId::Write,
        2 => SyscallId::Read,
        3 => SyscallId::Open,
        4 => SyscallId::Close,
        5 => SyscallId::FileRead,
        6 => SyscallId::FileWrite,
        7 => SyscallId::Seek,
        8 => SyscallId::Stat,
        9 => SyscallId::ReadDir,
        10 => SyscallId::MapMemory,
        11 => SyscallId::UnmapMemory,
        12 => SyscallId::HeapAlloc,
        13 => SyscallId::HeapFree,
        14 => SyscallId::ClockGetTime,
        15 => SyscallId::Sleep,
        16 => SyscallId::GetPid,
        17 => SyscallId::Spawn,
        18 => SyscallId::Wait,
        19 => SyscallId::GetAbiVersion,
        _ => return None,
    })
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
        }
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
