//! One fixed original-PQ2 integer projection, owned by a supervised child.

use super::{
    Api, DEVICE_NOT_FOUND, GPU, Id, MAX_DEVICES, MAX_PLATFORMS, bounded_count, check, invalid,
};
use crate::{OpenClDevice, OpenClPq2Projection, OpenClProjectionSelection};
use std::ffi::{c_char, c_void};
use std::io;

type Context = unsafe extern "system" fn(
    *const isize,
    u32,
    *const Id,
    Option<unsafe extern "system" fn(*const c_char, *const c_void, usize, *mut c_void)>,
    *mut c_void,
    *mut i32,
) -> Id;
type Queue = unsafe extern "system" fn(Id, Id, u64, *mut i32) -> Id;
type Buffer = unsafe extern "system" fn(Id, u64, usize, *mut c_void, *mut i32) -> Id;
type Program =
    unsafe extern "system" fn(Id, u32, *const *const c_char, *const usize, *mut i32) -> Id;
type Build = unsafe extern "system" fn(
    Id,
    u32,
    *const Id,
    *const c_char,
    Option<unsafe extern "system" fn(Id, *mut c_void)>,
    *mut c_void,
) -> i32;
type Kernel = unsafe extern "system" fn(Id, *const c_char, *mut i32) -> Id;
type Argument = unsafe extern "system" fn(Id, u32, usize, *const c_void) -> i32;
type Enqueue = unsafe extern "system" fn(
    Id,
    Id,
    u32,
    *const usize,
    *const usize,
    *const usize,
    u32,
    *const Id,
    *mut Id,
) -> i32;
type Read = unsafe extern "system" fn(
    Id,
    Id,
    u32,
    usize,
    usize,
    *mut c_void,
    u32,
    *const Id,
    *mut Id,
) -> i32;
type Release = unsafe extern "system" fn(Id) -> i32;

// No optional extensions, floating arithmetic, subgroup widths or vendor binaries.
// One work-item preserves one original 128-symbol PQ2 integer reduction. The host
// retains the original half scales and CPU-prepared Q8_K scale/FMA order.
pub(super) const SOURCE: &str = r"
__kernel void pq2_q8k_block_dots(__global const uchar *weights,
                                __global const char *codes,
                                __global int *output) {
    const size_t block = get_global_id(0);
    const size_t offset = block * 34 + 2;
    const size_t activation = (block % 40) * 128;
    int sum = 0;
    for (uint lane = 0; lane < 128; ++lane) {
        const uint symbol = (weights[offset + lane / 4] >> (2 * (lane % 4))) & 3;
        sum += (convert_int(symbol) - 1) * convert_int(codes[activation + lane]);
    }
    output[block] = sum;
}
";

struct Compute {
    context: Context,
    queue: Queue,
    buffer: Buffer,
    program: Program,
    build: Build,
    kernel: Kernel,
    argument: Argument,
    enqueue: Enqueue,
    read: Read,
    finish: Release,
    release_context: Release,
    release_queue: Release,
    release_buffer: Release,
    release_program: Release,
    release_kernel: Release,
}

impl Compute {
    fn load(api: &Api) -> io::Result<Self> {
        macro_rules! symbol {
            ($name:literal, $kind:ty) => {{
                let pointer = api._library.symbol($name)?;
                // SAFETY: Fixed OpenCL 1.x core symbol and the exact Khronos
                // cl.h ABI above. The containing Api outlives all pointer use.
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $kind>(pointer)
                }
            }};
        }
        Ok(Self {
            context: symbol!(c"clCreateContext", Context),
            queue: symbol!(c"clCreateCommandQueue", Queue),
            buffer: symbol!(c"clCreateBuffer", Buffer),
            program: symbol!(c"clCreateProgramWithSource", Program),
            build: symbol!(c"clBuildProgram", Build),
            kernel: symbol!(c"clCreateKernel", Kernel),
            argument: symbol!(c"clSetKernelArg", Argument),
            enqueue: symbol!(c"clEnqueueNDRangeKernel", Enqueue),
            read: symbol!(c"clEnqueueReadBuffer", Read),
            finish: symbol!(c"clFinish", Release),
            release_context: symbol!(c"clReleaseContext", Release),
            release_queue: symbol!(c"clReleaseCommandQueue", Release),
            release_buffer: symbol!(c"clReleaseMemObject", Release),
            release_program: symbol!(c"clReleaseProgram", Release),
            release_kernel: symbol!(c"clReleaseKernel", Release),
        })
    }
}

struct Owner<'a> {
    api: &'a Compute,
    context: Id,
    queue: Id,
    program: Id,
    kernel: Id,
    buffers: [Id; 3],
    submitted: bool,
}

impl Owner<'_> {
    fn drain(&mut self) {
        if self.submitted {
            // SAFETY: The queue remains live. A successful fence ends all
            // asynchronous use before releasing any object or unloading code.
            if unsafe { (self.api.finish)(self.queue) } != 0 {
                // Retain both this owner and any caller readback storage while
                // terminal completion is unknown, until the supervisor exits us.
                loop {
                    std::thread::park();
                }
            }
            self.submitted = false;
        }
    }

    fn close(&mut self) -> [i32; 7] {
        self.drain();
        let mut codes = [0; 7];
        let [first, second, third] = &mut self.buffers;
        for (index, (id, release)) in [
            (&mut self.kernel, self.api.release_kernel),
            (&mut self.program, self.api.release_program),
            (third, self.api.release_buffer),
            (second, self.api.release_buffer),
            (first, self.api.release_buffer),
            (&mut self.queue, self.api.release_queue),
            (&mut self.context, self.api.release_context),
        ]
        .into_iter()
        .enumerate()
        {
            if !id.is_null() {
                // SAFETY: Each owned object is released once after a terminal
                // fence, while the implementation and context remain live.
                codes[index] = unsafe { release(*id) };
                *id = std::ptr::null_mut();
            }
        }
        codes
    }
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn object(id: Id, code: i32) -> io::Result<()> {
    check(code)?;
    if id.is_null() { Err(invalid()) } else { Ok(()) }
}

fn selected(api: &Api, selection: &OpenClProjectionSelection) -> io::Result<(Id, OpenClDevice)> {
    let mut platforms = [std::ptr::null_mut(); MAX_PLATFORMS];
    let mut count = 0;
    // SAFETY: Eight initialized output IDs, a native count and a live loader.
    check(unsafe { (api.platforms)(8, platforms.as_mut_ptr(), &raw mut count) })?;
    let count = bounded_count(count, MAX_PLATFORMS)?;
    let mut selected = None;
    let mut remaining = MAX_DEVICES;
    for platform in &platforms[..count] {
        if platform.is_null() {
            return Err(invalid());
        }
        let name = api.string(*platform, 0x0902, true)?;
        let mut devices = [std::ptr::null_mut(); MAX_DEVICES];
        let mut count = 0;
        // SAFETY: The platform came from this loader. The fixed array can hold
        // sixteen GPU IDs; the aggregate count is checked before indexing.
        let code =
            unsafe { (api.devices)(*platform, GPU, 16, devices.as_mut_ptr(), &raw mut count) };
        if code == DEVICE_NOT_FOUND {
            continue;
        }
        check(code)?;
        let count = bounded_count(count, remaining)?;
        remaining -= count;
        for id in &devices[..count] {
            let device = api.device(*id)?;
            if name == selection.platform_name
                && device.name == selection.device_name
                && device.driver_version == selection.driver_version
                && device.vendor_id == selection.vendor_id
            {
                if selected.is_some()
                    || !device.available
                    || !device.compiler_available
                    || device.address_bits != 64
                    || !device.name.to_ascii_lowercase().contains("adreno")
                    || !device.name.to_ascii_lowercase().contains("x2")
                {
                    return Err(invalid());
                }
                selected = Some((*id, device));
            }
        }
    }
    selected.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "exact Adreno X2 OpenCL selection unavailable",
        )
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the single audited creation, submission, readback and terminal-fence sequence together so object ownership remains reviewable."
)]
pub(super) fn run(
    selection: &OpenClProjectionSelection,
    weights: &[u8],
    codes: &[u8],
) -> io::Result<OpenClPq2Projection> {
    super::validate_environment()?;
    if weights.len() != 17_408 * 40 * 34 || codes.len() != 5120 {
        return Err(invalid());
    }
    let api = Api::load()?;
    let (device_id, device) = selected(&api, selection)?;
    let output_bytes = 17_408 * 40 * size_of::<i32>();
    if [weights.len(), codes.len(), output_bytes]
        .iter()
        .any(|&size| {
            u64::try_from(size).map_or(true, |size| size > device.maximum_allocation_bytes)
        })
    {
        return Err(invalid());
    }
    let functions = Compute::load(&api)?;
    let mut owner = Owner {
        api: &functions,
        context: std::ptr::null_mut(),
        queue: std::ptr::null_mut(),
        program: std::ptr::null_mut(),
        kernel: std::ptr::null_mut(),
        buffers: [std::ptr::null_mut(); 3],
        submitted: false,
    };
    let mut code = 0;
    // SAFETY: One selected live device; no callback/user pointer is installed.
    owner.context = unsafe {
        (functions.context)(
            std::ptr::null(),
            1,
            &raw const device_id,
            None,
            std::ptr::null_mut(),
            &raw mut code,
        )
    };
    object(owner.context, code)?;
    // SAFETY: Context/device are live and associated. Zero selects an in-order
    // queue without profiling, optional flags or out-of-order execution.
    owner.queue = unsafe { (functions.queue)(owner.context, device_id, 0, &raw mut code) };
    object(owner.queue, code)?;
    for (index, bytes) in [weights, codes].into_iter().enumerate() {
        // SAFETY: COPY_HOST_PTR copies the bounded live input during this call;
        // no host address remains borrowed by the asynchronous queue afterward.
        owner.buffers[index] = unsafe {
            (functions.buffer)(
                owner.context,
                4 | 32,
                bytes.len(),
                bytes.as_ptr().cast_mut().cast(),
                &raw mut code,
            )
        };
        object(owner.buffers[index], code)?;
    }
    // SAFETY: Kernel-write-only object, fixed checked size and no host pointer.
    owner.buffers[2] = unsafe {
        (functions.buffer)(
            owner.context,
            2,
            output_bytes,
            std::ptr::null_mut(),
            &raw mut code,
        )
    };
    object(owner.buffers[2], code)?;
    let source = SOURCE.as_ptr().cast::<c_char>();
    let length = SOURCE.len();
    // SAFETY: One source string with an explicit byte length, copied by OpenCL.
    owner.program = unsafe {
        (functions.program)(
            owner.context,
            1,
            &raw const source,
            &raw const length,
            &raw mut code,
        )
    };
    object(owner.program, code)?;
    // SAFETY: Synchronous build for the same live device. Fixed options install
    // no callback and request OpenCL C 1.2 without fast-math/reassociation flags.
    check(unsafe {
        (functions.build)(
            owner.program,
            1,
            &raw const device_id,
            c"-cl-std=CL1.2".as_ptr(),
            None,
            std::ptr::null_mut(),
        )
    })?;
    // SAFETY: Fixed kernel name from the owned successfully built program.
    owner.kernel =
        unsafe { (functions.kernel)(owner.program, c"pq2_q8k_block_dots".as_ptr(), &raw mut code) };
    object(owner.kernel, code)?;
    for (index, buffer) in (0_u32..3).zip(&owner.buffers) {
        // SAFETY: OpenCL copies one live cl_mem handle of the exact native size.
        check(unsafe {
            (functions.argument)(
                owner.kernel,
                index,
                size_of::<Id>(),
                (&raw const *buffer).cast(),
            )
        })?;
    }
    let work = 17_408 * 40;
    // SAFETY: One-dimensional fixed complete-block range. No local-size promise,
    // offsets, event arrays or host pointers; all referenced buffers stay owned.
    check(unsafe {
        (functions.enqueue)(
            owner.queue,
            owner.kernel,
            1,
            std::ptr::null(),
            &raw const work,
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    })?;
    owner.submitted = true;
    let mut dots = vec![0_i32; work];
    // SAFETY: Blocking read into the full aligned i32 array. Its lifetime spans
    // this call and the following terminal queue fence, including error paths.
    let read = unsafe {
        (functions.read)(
            owner.queue,
            owner.buffers[2],
            1,
            0,
            output_bytes,
            dots.as_mut_ptr().cast(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    // Drain before an error can drop the host readback array.
    owner.drain();
    check(read)?;
    let cleanup_codes = owner.close();
    Ok(OpenClPq2Projection {
        loader_path: api.path,
        device,
        integer_dots: dots,
        cleanup_codes,
    })
}
