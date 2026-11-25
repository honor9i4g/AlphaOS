#![no_std]

//! Shared syscall numbers and register/result layout for the kernel and apps.
//!
//! The x86-64 syscall entry places the syscall number in `RAX` and six
//! arguments in `RDI`, `RSI`, `RDX`, `R10`, `R8`, and `R9`. A return value uses
//! `RAX` for `value` and `RDX` for `error`; zero error means success, and a
//! non-zero error is a positive errno value. The freestanding kernel currently
//! has a prototype entry path; this ABI does not imply that every service is
//! implemented or that a general application runtime exists.

pub const SYSCALL_ABI_MAJOR: u16 = 1;
pub const SYSCALL_ABI_MINOR: u16 = 4;
pub const SYSCALL_COUNT: usize = 150;

pub const EPERM: u64 = 1;
pub const ENOENT: u64 = 2;
pub const ESRCH: u64 = 3;
pub const EIO: u64 = 5;
pub const EBADF: u64 = 9;
pub const ECHILD: u64 = 10;
pub const EAGAIN: u64 = 11;
pub const ENOMEM: u64 = 12;
pub const EACCES: u64 = 13;
pub const EFAULT: u64 = 14;
pub const EEXIST: u64 = 17;
pub const ENODEV: u64 = 19;
pub const ENOTDIR: u64 = 20;
pub const EISDIR: u64 = 21;
pub const EINVAL: u64 = 22;
pub const ENOSPC: u64 = 28;
pub const EPIPE: u64 = 32;
pub const ERANGE: u64 = 34;
pub const ENOSYS: u64 = 38;
pub const ENOTSUP: u64 = 95;
pub const EAFNOSUPPORT: u64 = 97;
pub const ENOTCONN: u64 = 107;

pub const SYSTEM_INFO_ABI_VERSION: u64 = 0;
pub const SYSTEM_INFO_MEMORY: u64 = 1;
pub const SYSTEM_INFO_FRAMEBUFFER: u64 = 2;

pub const DEVICE_KEYBOARD: u32 = 1;
pub const DEVICE_FRAMEBUFFER: u32 = 2;

pub const DEVICE_ACCESS_READ: u32 = 1;
pub const DEVICE_ACCESS_WRITE: u32 = 2;

pub const DEVICE_KIND_INPUT: u32 = 1;
pub const DEVICE_KIND_DISPLAY: u32 = 2;

pub const INPUT_EVENT_SYN: u16 = 0;
pub const INPUT_EVENT_KEY: u16 = 1;
pub const INPUT_SYN_DROPPED: u16 = 1;
pub const INPUT_MODIFIER_SHIFT: u32 = 1 << 0;
pub const INPUT_MODIFIER_CONTROL: u32 = 1 << 1;
pub const INPUT_MODIFIER_ALT: u32 = 1 << 2;
pub const INPUT_MODIFIER_CAPS_LOCK: u32 = 1 << 3;

pub const KEY_EVENT_RELEASE: i32 = 0;
pub const KEY_EVENT_PRESS: i32 = 1;
pub const KEY_EVENT_REPEAT: i32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum KeyCode {
    A = 0x04,
    B = 0x05,
    C = 0x06,
    D = 0x07,
    E = 0x08,
    F = 0x09,
    G = 0x0A,
    H = 0x0B,
    I = 0x0C,
    J = 0x0D,
    K = 0x0E,
    L = 0x0F,
    M = 0x10,
    N = 0x11,
    O = 0x12,
    P = 0x13,
    Q = 0x14,
    R = 0x15,
    S = 0x16,
    T = 0x17,
    U = 0x18,
    V = 0x19,
    W = 0x1A,
    X = 0x1B,
    Y = 0x1C,
    Z = 0x1D,
    Digit1 = 0x1E,
    Digit2 = 0x1F,
    Digit3 = 0x20,
    Digit4 = 0x21,
    Digit5 = 0x22,
    Digit6 = 0x23,
    Digit7 = 0x24,
    Digit8 = 0x25,
    Digit9 = 0x26,
    Digit0 = 0x27,
    Enter = 0x28,
    Escape = 0x29,
    Backspace = 0x2A,
    Tab = 0x2B,
    Space = 0x2C,
    Minus = 0x2D,
    Equal = 0x2E,
    LeftBracket = 0x2F,
    RightBracket = 0x30,
    Backslash = 0x31,
    Semicolon = 0x33,
    Apostrophe = 0x34,
    Grave = 0x35,
    Comma = 0x36,
    Period = 0x37,
    Slash = 0x38,
    CapsLock = 0x39,
    F1 = 0x3A,
    F2 = 0x3B,
    F3 = 0x3C,
    F4 = 0x3D,
    F5 = 0x3E,
    F6 = 0x3F,
    F7 = 0x40,
    F8 = 0x41,
    F9 = 0x42,
    F10 = 0x43,
    F11 = 0x44,
    F12 = 0x45,
    PrintScreen = 0x46,
    ScrollLock = 0x47,
    Pause = 0x48,
    Insert = 0x49,
    Home = 0x4A,
    PageUp = 0x4B,
    Delete = 0x4C,
    End = 0x4D,
    PageDown = 0x4E,
    Right = 0x4F,
    Left = 0x50,
    Down = 0x51,
    Up = 0x52,
    NumLock = 0x53,
    KeypadSlash = 0x54,
    KeypadEnter = 0x58,
    LeftControl = 0xE0,
    LeftShift = 0xE1,
    LeftAlt = 0xE2,
    LeftMeta = 0xE3,
    RightControl = 0xE4,
    RightShift = 0xE5,
    RightAlt = 0xE6,
    RightMeta = 0xE7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct InputEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
    pub modifiers: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct AbiVersionInfo {
    pub major: u16,
    pub minor: u16,
    pub syscall_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct MemoryInfo {
    pub managed_bytes: u64,
    pub allocated_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct FramebufferInfo {
    pub width: u64,
    pub height: u64,
    pub pitch_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct FileStat {
    pub size: u64,
    pub mode: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct TimeSpec {
    pub seconds: u64,
    pub nanoseconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct DirEntry {
    pub name_len: u32,
    pub entry_type: u32,
    pub size: u64,
    pub name: [u8; 64],
}

pub const SEEK_SET: u64 = 0;
pub const SEEK_CUR: u64 = 1;
pub const SEEK_END: u64 = 2;

pub const O_READ: u32 = 0x1;
pub const O_WRITE: u32 = 0x2;
pub const O_CREATE: u32 = 0x4;
pub const O_TRUNC: u32 = 0x8;
pub const O_DIR: u32 = 0x10;

pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

pub const MAP_ANON: u32 = 0x1;
pub const PROT_READ: u32 = 0x1;
pub const PROT_WRITE: u32 = 0x2;
pub const PROT_EXEC: u32 = 0x4;

pub const AF_INET: u32 = 2;
pub const SOCK_STREAM: u32 = 1;
pub const SOCK_DGRAM: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct DeviceInfo {
    pub id: u32,
    pub kind: u32,
    pub capabilities: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SyscallCategory {
    Process,
    File,
    Memory,
    Time,
    SystemInfo,
    Device,
    Communication,
    Network,
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
    DeviceList = 20,
    DeviceOpen = 21,
    DeviceControl = 22,
    DeviceRead = 23,
    DeviceWrite = 24,
    GetSystemInfo = 25,
    ChannelPairCreate = 26,
    ChannelSend = 27,
    ChannelReceive = 28,
    ChannelClose = 29,
    SharedMemoryCreate = 30,
    Yield = 31,
    GetTid = 32,
    GetPpid = 33,
    GetUid = 34,
    GetGid = 35,
    SetPriority = 36,
    GetPriority = 37,
    VirtualAlloc = 38,
    VirtualFree = 39,
    VirtualProtect = 40,
    Truncate = 41,
    Fstat = 42,
    Sync = 43,
    Pipe = 44,
    SocketCreate = 45,
    SocketBind = 46,
    SocketListen = 47,
    SocketAccept = 48,
    SocketConnect = 49,
    Reboot = 50,
    Reserved51 = 51,
    Reserved52 = 52,
    Reserved53 = 53,
    Reserved54 = 54,
    Reserved55 = 55,
    Reserved56 = 56,
    Reserved57 = 57,
    Reserved58 = 58,
    Reserved59 = 59,
    Reserved60 = 60,
    Reserved61 = 61,
    Reserved62 = 62,
    Reserved63 = 63,
    Reserved64 = 64,
    Reserved65 = 65,
    Reserved66 = 66,
    Reserved67 = 67,
    Reserved68 = 68,
    Reserved69 = 69,
    Reserved70 = 70,
    Reserved71 = 71,
    Reserved72 = 72,
    Reserved73 = 73,
    Reserved74 = 74,
    Reserved75 = 75,
    Reserved76 = 76,
    Reserved77 = 77,
    Reserved78 = 78,
    Reserved79 = 79,
    Reserved80 = 80,
    Reserved81 = 81,
    Reserved82 = 82,
    Reserved83 = 83,
    Reserved84 = 84,
    Reserved85 = 85,
    Reserved86 = 86,
    Reserved87 = 87,
    Reserved88 = 88,
    Reserved89 = 89,
    Reserved90 = 90,
    Reserved91 = 91,
    Reserved92 = 92,
    Reserved93 = 93,
    Reserved94 = 94,
    Reserved95 = 95,
    Reserved96 = 96,
    Reserved97 = 97,
    Reserved98 = 98,
    Reserved99 = 99,
    Reserved100 = 100,
    Reserved101 = 101,
    Reserved102 = 102,
    Reserved103 = 103,
    Reserved104 = 104,
    Reserved105 = 105,
    Reserved106 = 106,
    Reserved107 = 107,
    Reserved108 = 108,
    Reserved109 = 109,
    Reserved110 = 110,
    Reserved111 = 111,
    Reserved112 = 112,
    Reserved113 = 113,
    Reserved114 = 114,
    Reserved115 = 115,
    Reserved116 = 116,
    Reserved117 = 117,
    Reserved118 = 118,
    Reserved119 = 119,
    Reserved120 = 120,
    Reserved121 = 121,
    Reserved122 = 122,
    Reserved123 = 123,
    Reserved124 = 124,
    Reserved125 = 125,
    Reserved126 = 126,
    Reserved127 = 127,
    Reserved128 = 128,
    Reserved129 = 129,
    Reserved130 = 130,
    Reserved131 = 131,
    Reserved132 = 132,
    Reserved133 = 133,
    Reserved134 = 134,
    Reserved135 = 135,
    Reserved136 = 136,
    Reserved137 = 137,
    Reserved138 = 138,
    Reserved139 = 139,
    Reserved140 = 140,
    Reserved141 = 141,
    Reserved142 = 142,
    Reserved143 = 143,
    Reserved144 = 144,
    Reserved145 = 145,
    Reserved146 = 146,
    Reserved147 = 147,
    Reserved148 = 148,
    Reserved149 = 149,
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
    SyscallInfo {
        id: SyscallId::DeviceList,
        name: "device_list",
        category: SyscallCategory::Device,
        argument_count: 2,
        arguments: ["entries_ptr", "capacity", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::DeviceOpen,
        name: "device_open",
        category: SyscallCategory::Device,
        argument_count: 2,
        arguments: ["device_id", "access_flags", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::DeviceControl,
        name: "device_control",
        category: SyscallCategory::Device,
        argument_count: 6,
        arguments: [
            "device_handle",
            "request",
            "input_ptr",
            "input_len",
            "output_ptr",
            "output_len",
        ],
    },
    SyscallInfo {
        id: SyscallId::DeviceRead,
        name: "device_read",
        category: SyscallCategory::Device,
        argument_count: 3,
        arguments: ["device_handle", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::DeviceWrite,
        name: "device_write",
        category: SyscallCategory::Device,
        argument_count: 3,
        arguments: ["device_handle", "buffer_ptr", "length", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetSystemInfo,
        name: "get_system_info",
        category: SyscallCategory::SystemInfo,
        argument_count: 3,
        arguments: ["info_kind", "buffer_ptr", "buffer_len", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ChannelPairCreate,
        name: "channel_pair_create",
        category: SyscallCategory::Communication,
        argument_count: 2,
        arguments: ["handles_ptr", "flags", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ChannelSend,
        name: "channel_send",
        category: SyscallCategory::Communication,
        argument_count: 4,
        arguments: ["channel_handle", "message_ptr", "length", "flags", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ChannelReceive,
        name: "channel_receive",
        category: SyscallCategory::Communication,
        argument_count: 4,
        arguments: ["channel_handle", "buffer_ptr", "capacity", "flags", "", ""],
    },
    SyscallInfo {
        id: SyscallId::ChannelClose,
        name: "channel_close",
        category: SyscallCategory::Communication,
        argument_count: 1,
        arguments: ["channel_handle", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SharedMemoryCreate,
        name: "shared_memory_create",
        category: SyscallCategory::Communication,
        argument_count: 2,
        arguments: ["size", "flags", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Yield,
        name: "yield",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetTid,
        name: "gettid",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetPpid,
        name: "getppid",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetUid,
        name: "getuid",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetGid,
        name: "getgid",
        category: SyscallCategory::Process,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SetPriority,
        name: "set_priority",
        category: SyscallCategory::Process,
        argument_count: 2,
        arguments: ["pid", "priority", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::GetPriority,
        name: "get_priority",
        category: SyscallCategory::Process,
        argument_count: 1,
        arguments: ["pid", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::VirtualAlloc,
        name: "virtual_alloc",
        category: SyscallCategory::Memory,
        argument_count: 3,
        arguments: ["address", "length", "flags", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::VirtualFree,
        name: "virtual_free",
        category: SyscallCategory::Memory,
        argument_count: 2,
        arguments: ["address", "length", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::VirtualProtect,
        name: "virtual_protect",
        category: SyscallCategory::Memory,
        argument_count: 3,
        arguments: ["address", "length", "flags", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Truncate,
        name: "truncate",
        category: SyscallCategory::File,
        argument_count: 2,
        arguments: ["handle", "length", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Fstat,
        name: "fstat",
        category: SyscallCategory::File,
        argument_count: 2,
        arguments: ["handle", "stat_ptr", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Sync,
        name: "sync",
        category: SyscallCategory::File,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Pipe,
        name: "pipe",
        category: SyscallCategory::Communication,
        argument_count: 1,
        arguments: ["handles_ptr", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SocketCreate,
        name: "socket_create",
        category: SyscallCategory::Network,
        argument_count: 3,
        arguments: ["domain", "type", "protocol", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SocketBind,
        name: "socket_bind",
        category: SyscallCategory::Network,
        argument_count: 3,
        arguments: ["socket_handle", "addr_ptr", "addr_len", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SocketListen,
        name: "socket_listen",
        category: SyscallCategory::Network,
        argument_count: 2,
        arguments: ["socket_handle", "backlog", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SocketAccept,
        name: "socket_accept",
        category: SyscallCategory::Network,
        argument_count: 3,
        arguments: ["socket_handle", "addr_ptr", "addr_len_ptr", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::SocketConnect,
        name: "socket_connect",
        category: SyscallCategory::Network,
        argument_count: 3,
        arguments: ["socket_handle", "addr_ptr", "addr_len", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reboot,
        name: "reboot",
        category: SyscallCategory::SystemInfo,
        argument_count: 1,
        arguments: ["cmd", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved51,
        name: "reserved_51",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved52,
        name: "reserved_52",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved53,
        name: "reserved_53",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved54,
        name: "reserved_54",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved55,
        name: "reserved_55",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved56,
        name: "reserved_56",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved57,
        name: "reserved_57",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved58,
        name: "reserved_58",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved59,
        name: "reserved_59",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved60,
        name: "reserved_60",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved61,
        name: "reserved_61",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved62,
        name: "reserved_62",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved63,
        name: "reserved_63",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved64,
        name: "reserved_64",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved65,
        name: "reserved_65",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved66,
        name: "reserved_66",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved67,
        name: "reserved_67",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved68,
        name: "reserved_68",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved69,
        name: "reserved_69",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved70,
        name: "reserved_70",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved71,
        name: "reserved_71",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved72,
        name: "reserved_72",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved73,
        name: "reserved_73",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved74,
        name: "reserved_74",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved75,
        name: "reserved_75",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved76,
        name: "reserved_76",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved77,
        name: "reserved_77",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved78,
        name: "reserved_78",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved79,
        name: "reserved_79",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved80,
        name: "reserved_80",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved81,
        name: "reserved_81",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved82,
        name: "reserved_82",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved83,
        name: "reserved_83",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved84,
        name: "reserved_84",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved85,
        name: "reserved_85",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved86,
        name: "reserved_86",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved87,
        name: "reserved_87",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved88,
        name: "reserved_88",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved89,
        name: "reserved_89",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved90,
        name: "reserved_90",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved91,
        name: "reserved_91",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved92,
        name: "reserved_92",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved93,
        name: "reserved_93",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved94,
        name: "reserved_94",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved95,
        name: "reserved_95",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved96,
        name: "reserved_96",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved97,
        name: "reserved_97",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved98,
        name: "reserved_98",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved99,
        name: "reserved_99",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved100,
        name: "reserved_100",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved101,
        name: "reserved_101",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved102,
        name: "reserved_102",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved103,
        name: "reserved_103",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved104,
        name: "reserved_104",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved105,
        name: "reserved_105",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved106,
        name: "reserved_106",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved107,
        name: "reserved_107",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved108,
        name: "reserved_108",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved109,
        name: "reserved_109",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved110,
        name: "reserved_110",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved111,
        name: "reserved_111",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved112,
        name: "reserved_112",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved113,
        name: "reserved_113",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved114,
        name: "reserved_114",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved115,
        name: "reserved_115",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved116,
        name: "reserved_116",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved117,
        name: "reserved_117",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved118,
        name: "reserved_118",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved119,
        name: "reserved_119",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved120,
        name: "reserved_120",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved121,
        name: "reserved_121",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved122,
        name: "reserved_122",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved123,
        name: "reserved_123",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved124,
        name: "reserved_124",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved125,
        name: "reserved_125",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved126,
        name: "reserved_126",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved127,
        name: "reserved_127",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved128,
        name: "reserved_128",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved129,
        name: "reserved_129",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved130,
        name: "reserved_130",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved131,
        name: "reserved_131",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved132,
        name: "reserved_132",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved133,
        name: "reserved_133",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved134,
        name: "reserved_134",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved135,
        name: "reserved_135",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved136,
        name: "reserved_136",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved137,
        name: "reserved_137",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved138,
        name: "reserved_138",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved139,
        name: "reserved_139",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved140,
        name: "reserved_140",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved141,
        name: "reserved_141",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved142,
        name: "reserved_142",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved143,
        name: "reserved_143",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved144,
        name: "reserved_144",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved145,
        name: "reserved_145",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved146,
        name: "reserved_146",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved147,
        name: "reserved_147",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved148,
        name: "reserved_148",
        category: SyscallCategory::SystemInfo,
        argument_count: 0,
        arguments: ["", "", "", "", "", ""],
    },
    SyscallInfo {
        id: SyscallId::Reserved149,
        name: "reserved_149",
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
    fn syscall_table_has_stable_sequential_ids_and_complete_metadata() {
        assert_eq!(SYSCALLS.len(), 150);
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
    fn original_syscall_numbers_remain_stable_and_all_categories_are_present() {
        assert_eq!(SyscallId::Exit as u64, 0);
        assert_eq!(SyscallId::GetAbiVersion as u64, 19);
        assert_eq!(SyscallId::Reboot as u64, 50);
        assert!(SYSCALLS
            .iter()
            .any(|info| info.category == SyscallCategory::Device));
        assert!(SYSCALLS
            .iter()
            .any(|info| info.category == SyscallCategory::Communication));
        assert!(SYSCALLS
            .iter()
            .any(|info| info.category == SyscallCategory::Network));
    }

    #[test]
    fn system_information_structures_have_stable_c_layouts() {
        assert_eq!(core::mem::size_of::<AbiVersionInfo>(), 8);
        assert_eq!(core::mem::size_of::<MemoryInfo>(), 24);
        assert_eq!(core::mem::size_of::<FramebufferInfo>(), 24);
        assert_eq!(core::mem::size_of::<DeviceInfo>(), 16);
        assert_eq!(core::mem::size_of::<InputEvent>(), 16);
        assert_ne!(SYSTEM_INFO_ABI_VERSION, SYSTEM_INFO_MEMORY);
        assert_ne!(SYSTEM_INFO_MEMORY, SYSTEM_INFO_FRAMEBUFFER);
    }

    #[test]
    fn input_event_key_codes_and_modifiers_are_stable() {
        assert_eq!(KeyCode::A as u16, 0x04);
        assert_eq!(KeyCode::Enter as u16, 0x28);
        assert_eq!(KeyCode::LeftShift as u16, 0xE1);
        assert_eq!(INPUT_EVENT_KEY, 1);
        assert_eq!(INPUT_EVENT_SYN, 0);
        assert_eq!(INPUT_SYN_DROPPED, 1);
        assert_eq!(KEY_EVENT_RELEASE, 0);
        assert_eq!(KEY_EVENT_PRESS, 1);
        assert_eq!(KEY_EVENT_REPEAT, 2);
        assert_eq!(INPUT_MODIFIER_SHIFT | INPUT_MODIFIER_CAPS_LOCK, 9);
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

        assert_eq!(result.value, 0x0001_0004);
        assert_eq!(result.error, 0);
    }

    #[test]
    fn unavailable_calls_return_not_implemented() {
        for info in SYSCALLS
            .iter()
            .filter(|info| info.id != SyscallId::GetAbiVersion)
        {
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
