//! Standalone native-layout comparison with the separately compiled public C headers.
#![forbid(unsafe_code)]
#![deny(warnings)]

use std::io::{self, Write};
use std::mem::offset_of;
use std::process::ExitCode;

#[repr(C)]
struct Buffer {
    fd: u32,
    size: u32,
    offset: u32,
    flags: u32,
    address: u64,
}

#[repr(C)]
struct Tensor {
    data: u64,
    size: u32,
    flags: u32,
    kind: u32,
    buffer: u16,
    tensor: u16,
    dimensions: [u32; 4],
    strides: [u32; 4],
}

#[repr(C)]
struct HtpBuffer {
    base: u64,
    size: u64,
    flags: u32,
    fd: u32,
}

#[repr(C)]
struct Operation {
    opcode: u32,
    flags: u32,
    params: [i32; 16],
    kernel_params: [i32; 32],
    source: [u16; 10],
    destination: [u16; 4],
    padding: [u16; 2],
}

#[repr(C)]
struct Request {
    sequence: u64,
    buffers: u32,
    tensors: u32,
    operations: u32,
    traces: u32,
}

#[repr(C)]
struct Response {
    sequence: u64,
    cycles_start: u64,
    cycles_stop: u64,
    status: u32,
    buffers: u32,
    tensors: u32,
    operations: u32,
    microseconds: u32,
    traces: [u32; 11],
}

#[repr(C)]
struct Profile {
    opcode: u32,
    microseconds: u32,
    cycles_start: u32,
    cycles_stop: u32,
    counters: [u32; 8],
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 || !cfg!(all(windows, target_arch = "aarch64")) {
        return ExitCode::FAILURE;
    }
    let buffer = Buffer {
        fd: 0xfedc_ba98,
        size: 0x1020_3040,
        offset: 0x0102_0304,
        flags: 4 | 16 | 128,
        address: 0x8123_4567_89ab_cdef,
    };
    // Encode the declared fields independently; no unsafe object byte reads or copied C output.
    let bytes: Vec<u8> = [buffer.fd, buffer.size, buffer.offset, buffer.flags]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .chain(buffer.address.to_le_bytes())
        .collect();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let output = format!(
        concat!(
            "{{\"schemaVersion\":1,\"kind\":\"windows-arm64-Rust-queue-abi-control\",",
            "\"queueBuffer\":{{\"bytes\":{},\"alignment\":{},\"fdOffset\":{},",
            "\"sizeOffset\":{},\"offsetOffset\":{},\"flagsOffset\":{},\"addressOffset\":{}}},",
            "\"htp\":{{\"tensorBytes\":{},\"bufferBytes\":{},\"operationBytes\":{},",
            "\"requestBytes\":{},\"responseBytes\":{},\"profileBytes\":{}}},",
            "\"infiniteTimeout\":{},\"bufferHex\":\"{}\",",
            "\"driverLoaded\":false,\"deviceExecuted\":false,\"profileEligible\":false}}\n"
        ),
        size_of::<Buffer>(), align_of::<Buffer>(), offset_of!(Buffer, fd),
        offset_of!(Buffer, size), offset_of!(Buffer, offset), offset_of!(Buffer, flags),
        offset_of!(Buffer, address), size_of::<Tensor>(), size_of::<HtpBuffer>(),
        size_of::<Operation>(), size_of::<Request>(), size_of::<Response>(), size_of::<Profile>(),
        u32::MAX, hex,
    );
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
