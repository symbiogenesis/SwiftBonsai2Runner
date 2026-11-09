//! Bounded read-only Vulkan queries. The caller owns the driver-call deadline.

use std::ffi::{CStr, OsString, c_char};
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW};

use crate::{
    VulkanAdapter, VulkanInstanceModules, VulkanInventory, VulkanModuleFileCandidate,
    VulkanQueryFailure, WindowsGraphicsAdapterCount,
};

const MAX_ADAPTERS: usize = 16;
const MAX_QUEUES: usize = 64;
const MAX_EXTENSIONS: usize = 1024;
const MAX_MODULES: usize = 256;
const MAX_MODULE_NAME: usize = 260;

mod vk {
    use std::ffi::{c_char, c_void};

    pub(super) const API_VERSION_1_0: u32 = 1 << 22;
    pub(super) const API_VERSION_1_1: u32 = (1 << 22) | (1 << 12);
    pub(super) const API_VERSION_1_2: u32 = (1 << 22) | (2 << 12);
    pub(super) const API_VERSION_1_3: u32 = (1 << 22) | (3 << 12);

    pub(super) const SUCCESS: i32 = 0;
    #[cfg(test)]
    pub(super) const INCOMPLETE: i32 = 5;
    #[cfg(test)]
    pub(super) const ERROR_INCOMPATIBLE_DRIVER: i32 = -9;

    pub(super) const STRUCTURE_TYPE_APPLICATION_INFO: i32 = 0;
    pub(super) const STRUCTURE_TYPE_INSTANCE_CREATE_INFO: i32 = 1;
    pub(super) const STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2: i32 = 1_000_059_000;
    pub(super) const STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2: i32 = 1_000_059_001;
    pub(super) const STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES: i32 =
        1_000_082_000;
    pub(super) const STRUCTURE_TYPE_PHYSICAL_DEVICE_SUBGROUP_PROPERTIES: i32 = 1_000_094_000;
    pub(super) const STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES: i32 =
        1_000_280_000;

    pub(super) const PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU: i32 = 1;
    pub(super) const PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: i32 = 2;
    pub(super) const PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU: i32 = 3;
    pub(super) const PHYSICAL_DEVICE_TYPE_CPU: i32 = 4;

    pub(super) const QUEUE_COMPUTE_BIT: u32 = 0x0000_0002;
    pub(super) const MEMORY_HEAP_DEVICE_LOCAL_BIT: u32 = 0x0000_0001;

    pub(super) type Instance = *mut c_void;
    pub(super) type PhysicalDevice = *mut c_void;
    pub(super) type VoidFunction = unsafe extern "system" fn();
    pub(super) type GetInstanceProcAddr =
        unsafe extern "system" fn(Instance, *const c_char) -> Option<VoidFunction>;
    pub(super) type EnumerateInstanceVersion = unsafe extern "system" fn(*mut u32) -> i32;
    pub(super) type CreateInstance =
        unsafe extern "system" fn(*const InstanceCreateInfo, *const c_void, *mut Instance) -> i32;
    pub(super) type DestroyInstance = unsafe extern "system" fn(Instance, *const c_void);
    pub(super) type EnumeratePhysicalDevices =
        unsafe extern "system" fn(Instance, *mut u32, *mut PhysicalDevice) -> i32;
    pub(super) type GetPhysicalDeviceProperties =
        unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceProperties);
    pub(super) type EnumerateDeviceExtensionProperties = unsafe extern "system" fn(
        PhysicalDevice,
        *const c_char,
        *mut u32,
        *mut ExtensionProperties,
    ) -> i32;
    pub(super) type GetPhysicalDeviceQueueFamilyProperties =
        unsafe extern "system" fn(PhysicalDevice, *mut u32, *mut QueueFamilyProperties);
    pub(super) type GetPhysicalDeviceMemoryProperties =
        unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceMemoryProperties);
    pub(super) type GetPhysicalDeviceFeatures2 =
        unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceFeatures2);
    pub(super) type GetPhysicalDeviceProperties2 =
        unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceProperties2);

    #[repr(C)]
    pub(super) struct ApplicationInfo {
        pub(super) s_type: i32,
        pub(super) p_next: *const c_void,
        pub(super) p_application_name: *const c_char,
        pub(super) application_version: u32,
        pub(super) p_engine_name: *const c_char,
        pub(super) engine_version: u32,
        pub(super) api_version: u32,
    }

    #[repr(C)]
    pub(super) struct InstanceCreateInfo {
        pub(super) s_type: i32,
        pub(super) p_next: *const c_void,
        pub(super) flags: u32,
        pub(super) p_application_info: *const ApplicationInfo,
        pub(super) enabled_layer_count: u32,
        pub(super) pp_enabled_layer_names: *const *const c_char,
        pub(super) enabled_extension_count: u32,
        pub(super) pp_enabled_extension_names: *const *const c_char,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct PhysicalDeviceProperties {
        pub(super) api_version: u32,
        pub(super) driver_version: u32,
        pub(super) vendor_id: u32,
        pub(super) device_id: u32,
        pub(super) device_type: i32,
        pub(super) device_name: [c_char; 256],
        pub(super) pipeline_cache_uuid: [u8; 16],
        pub(super) limits: [u64; 63],
        pub(super) sparse_properties: [u32; 5],
    }

    impl Default for PhysicalDeviceProperties {
        fn default() -> Self {
            Self {
                api_version: 0,
                driver_version: 0,
                vendor_id: 0,
                device_id: 0,
                device_type: 0,
                device_name: [0; 256],
                pipeline_cache_uuid: [0; 16],
                limits: [0; 63],
                sparse_properties: [0; 5],
            }
        }
    }

    #[repr(C)]
    pub(super) struct PhysicalDeviceProperties2 {
        pub(super) s_type: i32,
        pub(super) p_next: *mut c_void,
        pub(super) properties: PhysicalDeviceProperties,
    }

    #[repr(C)]
    pub(super) struct PhysicalDeviceFeatures2 {
        pub(super) s_type: i32,
        pub(super) p_next: *mut c_void,
        pub(super) features: [u32; 55],
    }

    #[repr(C)]
    pub(super) struct PhysicalDeviceShaderFloat16Int8Features {
        pub(super) s_type: i32,
        pub(super) p_next: *mut c_void,
        pub(super) shader_float16: u32,
        pub(super) shader_int8: u32,
    }

    #[repr(C)]
    pub(super) struct PhysicalDeviceShaderIntegerDotProductFeatures {
        pub(super) s_type: i32,
        pub(super) p_next: *mut c_void,
        pub(super) shader_integer_dot_product: u32,
    }

    #[repr(C)]
    pub(super) struct PhysicalDeviceSubgroupProperties {
        pub(super) s_type: i32,
        pub(super) p_next: *mut c_void,
        pub(super) subgroup_size: u32,
        pub(super) supported_stages: u32,
        pub(super) supported_operations: u32,
        pub(super) quad_operations_in_all_stages: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub(super) struct ExtensionProperties {
        pub(super) extension_name: [c_char; 256],
        pub(super) spec_version: u32,
    }

    impl Default for ExtensionProperties {
        fn default() -> Self {
            Self {
                extension_name: [0; 256],
                spec_version: 0,
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct Extent3D {
        pub(super) width: u32,
        pub(super) height: u32,
        pub(super) depth: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct QueueFamilyProperties {
        pub(super) queue_flags: u32,
        pub(super) queue_count: u32,
        pub(super) timestamp_valid_bits: u32,
        pub(super) min_image_transfer_granularity: Extent3D,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct MemoryType {
        pub(super) property_flags: u32,
        pub(super) heap_index: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct MemoryHeap {
        pub(super) size: u64,
        pub(super) flags: u32,
        pub(super) reserved: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct PhysicalDeviceMemoryProperties {
        pub(super) type_count: u32,
        pub(super) types: [MemoryType; 32],
        pub(super) heap_count: u32,
        pub(super) heaps: [MemoryHeap; 16],
    }

    const _: () = {
        assert!(size_of::<ApplicationInfo>() == 48);
        assert!(size_of::<InstanceCreateInfo>() == 64);
        assert!(size_of::<PhysicalDeviceProperties>() == 824);
        assert!(size_of::<PhysicalDeviceProperties2>() == 840);
        assert!(size_of::<PhysicalDeviceFeatures2>() == 240);
        assert!(size_of::<PhysicalDeviceShaderFloat16Int8Features>() == 24);
        assert!(size_of::<PhysicalDeviceShaderIntegerDotProductFeatures>() == 24);
        assert!(size_of::<PhysicalDeviceSubgroupProperties>() == 32);
        assert!(size_of::<ExtensionProperties>() == 260);
        assert!(size_of::<QueueFamilyProperties>() == 24);
        assert!(size_of::<MemoryHeap>() == 16);
        assert!(size_of::<PhysicalDeviceMemoryProperties>() == 520);
    };
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "unsupported Vulkan inventory")
}

fn failure(
    stage: &'static str,
    vulkan_result: Option<i32>,
    loader_api_version: Option<u32>,
    application_api_version: Option<u32>,
    kind: io::ErrorKind,
) -> io::Error {
    io::Error::new(
        kind,
        VulkanQueryFailure {
            stage,
            vulkan_result,
            loader_api_version,
            application_api_version,
            instance_modules: None,
            graphics_adapter_count: None,
        },
    )
}

fn module_name(buffer: &[u16], count: usize) -> Option<String> {
    // A full buffer may be truncated. Reject malformed UTF-16 and names which
    // could expose paths, embed controls, or collapse distinct observations.
    if count == 0 || count >= buffer.len() {
        return None;
    }
    let name = String::from_utf16(buffer.get(..count)?).ok()?;
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn file_candidate_indices<T>(before: Option<&[String]>, modules: &[(String, T)]) -> Vec<usize> {
    modules
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| name == "vulkan-1.dll")
        .chain(modules.iter().enumerate().filter(|(_, (name, _))| {
            name != "vulkan-1.dll"
                && before.is_some_and(|before| before.binary_search(name).is_err())
        }))
        .take(16)
        .map(|(index, _)| index)
        .collect()
}

fn module_snapshot(
    before: Option<&[String]>,
    capture_paths: bool,
) -> Option<(Vec<String>, Vec<VulkanModuleFileCandidate>)> {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32EnumProcessModules, K32GetModuleBaseNameW, K32GetModuleFileNameExW},
        Threading::GetCurrentProcess,
    };

    let mut modules = [null_mut(); MAX_MODULES];
    let bytes = u32::try_from(size_of_val(&modules)).ok()?;
    let mut needed = 0_u32;
    // SAFETY: This pseudo-handle denotes only the current child, needs no close,
    // and is never exposed. Both output buffers remain writable for the call.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: The initialized module array has exactly the advertised byte size;
    // enumeration writes no more than that size and retains no storage pointers.
    if unsafe { K32EnumProcessModules(process, modules.as_mut_ptr(), bytes, &raw mut needed) } == 0
        || needed == 0
        || needed > bytes
        || !usize::try_from(needed)
            .ok()?
            .is_multiple_of(size_of_val(&modules[0]))
    {
        return None;
    }
    let count = usize::try_from(needed).ok()? / size_of_val(&modules[0]);
    let mut names = Vec::with_capacity(count);
    for module in modules.get(..count)? {
        if module.is_null() {
            return None;
        }
        let mut buffer = [0_u16; MAX_MODULE_NAME];
        // SAFETY: Module handles are opaque values from this process snapshot;
        // Win32 validates them. Unload races fail the entire optional snapshot.
        // The writable name buffer has the advertised UTF-16 element count.
        let length = unsafe { K32GetModuleBaseNameW(process, *module, buffer.as_mut_ptr(), 260) };
        names.push((
            module_name(&buffer, usize::try_from(length).ok()?)?,
            *module,
        ));
    }
    // The loader is audited first; remaining candidates are newly observed
    // basenames. Unknown before-snapshots permit only the known system loader.
    names.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut candidates = Vec::new();
    if capture_paths {
        for index in file_candidate_indices(before, &names) {
            let (name, module) = &names[index];
            let mut buffer = vec![0_u16; 32768];
            // SAFETY: The process/opaque module handle came from this snapshot;
            // Win32 validates unload races. The buffer has its advertised size.
            let length =
                unsafe { K32GetModuleFileNameExW(process, *module, buffer.as_mut_ptr(), 32768) };
            let path = usize::try_from(length)
                .ok()
                .filter(|length| *length > 0 && *length < buffer.len())
                .and_then(|length| {
                    let path = PathBuf::from(OsString::from_wide(&buffer[..length]));
                    path.is_absolute().then_some(path)
                });
            candidates.push(VulkanModuleFileCandidate {
                name: name.clone(),
                path,
            });
        }
    }
    let mut basenames: Vec<_> = names.into_iter().map(|(name, _)| name).collect();
    basenames.dedup();
    Some((basenames, candidates))
}

fn check(result: i32) -> io::Result<()> {
    if result == vk::SUCCESS {
        Ok(())
    } else {
        Err(failure(
            "device-query",
            Some(result),
            None,
            None,
            io::ErrorKind::Other,
        ))
    }
}

fn text(bytes: &[c_char]) -> io::Result<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(invalid)?;
    let bytes: Vec<_> = bytes[..end]
        .iter()
        .map(|byte| byte.cast_unsigned())
        .collect();
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    if text.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(text)
}

struct Library(HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the LoadLibraryExW module handle and
        // drops only after any Vulkan instance and its function pointers are gone.
        let _ = unsafe { FreeLibrary(self.0) };
    }
}

struct Loader {
    get_instance_proc_addr: vk::GetInstanceProcAddr,
    enumerate_instance_version: Option<vk::EnumerateInstanceVersion>,
    create_instance: vk::CreateInstance,
    _library: Library,
}

impl Loader {
    fn load() -> io::Result<Self> {
        let mut buffer = vec![0_u16; 32768];
        // SAFETY: The writable buffer has the advertised size and remains live for
        // the call. GetSystemDirectoryW retains no pointers.
        let count = unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                buffer.as_mut_ptr(),
                32768,
            )
        };
        let count = usize::try_from(count).map_err(|_| invalid())?;
        if count == 0 || count >= buffer.len() {
            return Err(io::Error::last_os_error());
        }
        let path = PathBuf::from(OsString::from_wide(&buffer[..count])).join("vulkan-1.dll");
        let wide_path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: An absolute NUL-terminated system-directory path is selected,
        // with search flags restricting dependencies to the loader's directory and
        // System32. Installed graphics loader/ICDs remain the operator trust boundary.
        let handle = unsafe { LoadLibraryExW(wide_path.as_ptr(), null_mut(), 0x100 | 0x800) };
        if handle.is_null() {
            return Err(io::Error::other("system Vulkan loader unavailable"));
        }
        let library = Library(handle);
        // SAFETY: The module is owned and live; the fixed Vulkan loader symbol is
        // NUL-terminated and used only while `library` remains live.
        let proc_addr = unsafe { GetProcAddress(library.0, c"vkGetInstanceProcAddr".as_ptr().cast()) }
            .ok_or_else(|| io::Error::other("system Vulkan loader unavailable"))?;
        // SAFETY: `vkGetInstanceProcAddr` has the exact Khronos Vulkan 1.0 ABI.
        let get_instance_proc_addr = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, vk::GetInstanceProcAddr>(
                proc_addr,
            )
        };
        // SAFETY: Global loader commands use a null VkInstance and NUL-terminated name.
        let enumerate_instance_version = unsafe {
            get_instance_proc_addr(null_mut(), c"vkEnumerateInstanceVersion".as_ptr())
        }
        .map(|function| {
            // SAFETY: Exact `vkEnumerateInstanceVersion` symbol and Khronos ABI.
            unsafe {
                std::mem::transmute::<vk::VoidFunction, vk::EnumerateInstanceVersion>(function)
            }
        });
        // SAFETY: Global loader command lookup with null VkInstance.
        let create_fn =
            unsafe { get_instance_proc_addr(null_mut(), c"vkCreateInstance".as_ptr()) }
                .ok_or_else(|| io::Error::other("system Vulkan loader unavailable"))?;
        // SAFETY: Exact `vkCreateInstance` symbol and Khronos ABI.
        let create_instance =
            unsafe { std::mem::transmute::<vk::VoidFunction, vk::CreateInstance>(create_fn) };
        Ok(Self {
            get_instance_proc_addr,
            enumerate_instance_version,
            create_instance,
            _library: library,
        })
    }

    fn instance_version(&self) -> Result<u32, i32> {
        let Some(enumerate) = self.enumerate_instance_version else {
            return Ok(vk::API_VERSION_1_0);
        };
        let mut version = vk::API_VERSION_1_0;
        // SAFETY: Writable u32 output for the global instance version query.
        let status = unsafe { enumerate(&raw mut version) };
        if status == vk::SUCCESS {
            Ok(version)
        } else {
            Err(status)
        }
    }

    fn instance_symbol(
        &self,
        instance: vk::Instance,
        name: &CStr,
    ) -> io::Result<vk::VoidFunction> {
        // SAFETY: Valid live VkInstance and NUL-terminated Vulkan command name.
        unsafe { (self.get_instance_proc_addr)(instance, name.as_ptr()) }.ok_or_else(invalid)
    }
}

struct Instance {
    handle: vk::Instance,
    destroy: vk::DestroyInstance,
    enumerate_physical_devices: vk::EnumeratePhysicalDevices,
    get_physical_device_properties: vk::GetPhysicalDeviceProperties,
    enumerate_device_extension_properties: vk::EnumerateDeviceExtensionProperties,
    get_physical_device_queue_family_properties: vk::GetPhysicalDeviceQueueFamilyProperties,
    get_physical_device_memory_properties: vk::GetPhysicalDeviceMemoryProperties,
    get_physical_device_features2: Option<vk::GetPhysicalDeviceFeatures2>,
    get_physical_device_properties2: Option<vk::GetPhysicalDeviceProperties2>,
}

const fn graphics_count_result(status: i32, count: u32) -> WindowsGraphicsAdapterCount {
    WindowsGraphicsAdapterCount {
        stage: "count-query",
        windows_error: None,
        ntstatus: Some(status),
        // Microsoft specifies that NumAdapters is undefined on error. Retain
        // no output count unless the call returned exactly STATUS_SUCCESS.
        adapter_count: if status == 0 { Some(count) } else { None },
    }
}

fn graphics_adapter_count() -> WindowsGraphicsAdapterCount {
    use windows_sys::{
        Wdk::Graphics::Direct3D::D3DKMT_ENUMADAPTERS2,
        Win32::{
            Foundation::GetLastError,
            System::LibraryLoader::GetModuleHandleExW,
        },
    };

    type Count = unsafe extern "system" fn(*mut D3DKMT_ENUMADAPTERS2) -> i32;
    struct ExistingModule(HMODULE);
    impl Drop for ExistingModule {
        fn drop(&mut self) {
            // SAFETY: GetModuleHandleExW acquired exactly this one reference;
            // the count function pointer is no longer used when it is released.
            let _ = unsafe { FreeLibrary(self.0) };
        }
    }

    let mut report = WindowsGraphicsAdapterCount {
        stage: "existing-gdi32-module",
        windows_error: None,
        ntstatus: None,
        adapter_count: None,
    };
    let mut handle = null_mut();
    // SAFETY: The fixed name is NUL terminated and the handle output is live.
    // This takes a reference only to an existing module, without loading one.
    if unsafe { GetModuleHandleExW(0, windows_sys::w!("gdi32.dll"), &raw mut handle) } == 0 {
        // SAFETY: Capture this thread's lookup error immediately, before any
        // other native call can replace it. A failed lookup does not prove absence.
        report.windows_error = Some(unsafe { GetLastError() });
        return report;
    }
    let module = ExistingModule(handle);
    report.stage = "enum-adapters-entry-point";
    // SAFETY: The acquired reference keeps the existing module live; the fixed
    // symbol is NUL terminated and used only before that reference is released.
    let Some(function) =
        (unsafe { GetProcAddress(module.0, c"D3DKMTEnumAdapters2".as_ptr().cast()) })
    else {
        // SAFETY: Capture the export lookup's error before releasing the module
        // reference or making any other native call.
        report.windows_error = Some(unsafe { GetLastError() });
        return report;
    };
    // SAFETY: The fixed Windows export has this documented system ABI, using
    // the generated SDK structure and the NTSTATUS i32 return representation.
    let count =
        unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Count>(function) };
    let mut request = D3DKMT_ENUMADAPTERS2::default();
    // SAFETY: This live writable structure has a null pAdapters field: only
    // the supported count is requested, without an adapter array or handles.
    // One call is made under the same owning child deadline and mitigations.
    let status = unsafe { count(&raw mut request) };
    graphics_count_result(status, request.NumAdapters)
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the instance, creates no children,
        // and is dropped before the Loader providing its function pointers.
        unsafe { (self.destroy)(self.handle, null()) };
    }
}

fn create_instance(
    loader: &Loader,
    loader_api: u32,
    application_api: u32,
) -> io::Result<(Instance, VulkanInstanceModules)> {
    let application = vk::ApplicationInfo {
        s_type: vk::STRUCTURE_TYPE_APPLICATION_INFO,
        p_next: null(),
        p_application_name: c"bonsai-inventory".as_ptr(),
        application_version: 0,
        p_engine_name: null(),
        engine_version: 0,
        api_version: application_api,
    };
    let create = vk::InstanceCreateInfo {
        s_type: vk::STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        p_next: null(),
        flags: 0,
        p_application_info: &raw const application,
        enabled_layer_count: 0,
        pp_enabled_layer_names: null(),
        enabled_extension_count: 0,
        pp_enabled_extension_names: null(),
    };
    let before = module_snapshot(None, false).map(|(names, _)| names);
    let mut raw_instance: vk::Instance = null_mut();
    // SAFETY: Initialized create structures have live pointers; no layers or
    // extensions are requested. The instance is destroyed before its Loader.
    let status = unsafe { (loader.create_instance)(&raw const create, null(), &raw mut raw_instance) };
    let after = module_snapshot(before.as_deref(), true);
    let (after, file_candidates) = after.map_or((None, None), |(names, candidates)| {
        (Some(names), Some(candidates))
    });
    let modules = VulkanInstanceModules {
        before,
        after,
        file_candidates,
    };
    if status != vk::SUCCESS || raw_instance.is_null() {
        return Err(io::Error::other(VulkanQueryFailure {
            stage: "instance-creation",
            vulkan_result: Some(status),
            loader_api_version: Some(loader_api),
            application_api_version: Some(application_api),
            instance_modules: Some(modules),
            graphics_adapter_count: Some(graphics_adapter_count()),
        }));
    }
    // SAFETY: Exact `vkDestroyInstance` symbol and Khronos ABI.
    let destroy = unsafe {
        std::mem::transmute::<vk::VoidFunction, vk::DestroyInstance>(
            loader.instance_symbol(raw_instance, c"vkDestroyInstance")?,
        )
    };
    let mut instance = Instance {
        handle: raw_instance,
        destroy,
        enumerate_physical_devices: unsafe {
            // SAFETY: Exact `vkEnumeratePhysicalDevices` symbol and Khronos ABI.
            std::mem::transmute::<vk::VoidFunction, vk::EnumeratePhysicalDevices>(
                loader.instance_symbol(raw_instance, c"vkEnumeratePhysicalDevices")?,
            )
        },
        get_physical_device_properties: unsafe {
            // SAFETY: Exact `vkGetPhysicalDeviceProperties` symbol and Khronos ABI.
            std::mem::transmute::<vk::VoidFunction, vk::GetPhysicalDeviceProperties>(
                loader.instance_symbol(raw_instance, c"vkGetPhysicalDeviceProperties")?,
            )
        },
        enumerate_device_extension_properties: unsafe {
            // SAFETY: Exact `vkEnumerateDeviceExtensionProperties` symbol and Khronos ABI.
            std::mem::transmute::<vk::VoidFunction, vk::EnumerateDeviceExtensionProperties>(
                loader.instance_symbol(raw_instance, c"vkEnumerateDeviceExtensionProperties")?,
            )
        },
        get_physical_device_queue_family_properties: unsafe {
            // SAFETY: Exact `vkGetPhysicalDeviceQueueFamilyProperties` symbol and Khronos ABI.
            std::mem::transmute::<vk::VoidFunction, vk::GetPhysicalDeviceQueueFamilyProperties>(
                loader
                    .instance_symbol(raw_instance, c"vkGetPhysicalDeviceQueueFamilyProperties")?,
            )
        },
        get_physical_device_memory_properties: unsafe {
            // SAFETY: Exact `vkGetPhysicalDeviceMemoryProperties` symbol and Khronos ABI.
            std::mem::transmute::<vk::VoidFunction, vk::GetPhysicalDeviceMemoryProperties>(
                loader.instance_symbol(raw_instance, c"vkGetPhysicalDeviceMemoryProperties")?,
            )
        },
        get_physical_device_features2: None,
        get_physical_device_properties2: None,
    };
    if application_api >= vk::API_VERSION_1_1 {
        // SAFETY: Exact Vulkan 1.1 `vkGetPhysicalDeviceFeatures2` and `vkGetPhysicalDeviceProperties2` ABIs.
        instance.get_physical_device_features2 = Some(unsafe {
            std::mem::transmute::<vk::VoidFunction, vk::GetPhysicalDeviceFeatures2>(
                loader.instance_symbol(raw_instance, c"vkGetPhysicalDeviceFeatures2")?,
            )
        });
        // SAFETY: Exact Vulkan 1.1 `vkGetPhysicalDeviceProperties2` symbol and Khronos ABI.
        instance.get_physical_device_properties2 = Some(unsafe {
            std::mem::transmute::<vk::VoidFunction, vk::GetPhysicalDeviceProperties2>(
                loader.instance_symbol(raw_instance, c"vkGetPhysicalDeviceProperties2")?,
            )
        });
    }
    Ok((instance, modules))
}

const fn device_type(kind: i32) -> (&'static str, bool) {
    match kind {
        vk::PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU => ("integrated-gpu", true),
        vk::PHYSICAL_DEVICE_TYPE_DISCRETE_GPU => ("discrete-gpu", true),
        vk::PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU => ("virtual-gpu", false),
        vk::PHYSICAL_DEVICE_TYPE_CPU => ("cpu", false),
        _ => ("other", false),
    }
}

#[derive(Default)]
struct Features {
    float16: Option<bool>,
    integer_dot: Option<bool>,
    subgroup_size: Option<u32>,
    subgroup_operations: Option<u32>,
    subgroup_stages: Option<u32>,
}

fn feature_structures(
    application_api: u32,
    device_api: u32,
    extensions: &[String],
) -> Option<(bool, bool)> {
    let effective_api = application_api.min(device_api);
    (effective_api >= vk::API_VERSION_1_1).then(|| {
        (
            effective_api >= vk::API_VERSION_1_2
                || extensions
                    .iter()
                    .any(|name| name == "VK_KHR_shader_float16_int8"),
            effective_api >= vk::API_VERSION_1_3
                || extensions
                    .iter()
                    .any(|name| name == "VK_KHR_shader_integer_dot_product"),
        )
    })
}

fn features(
    instance: &Instance,
    device: vk::PhysicalDevice,
    api: u32,
    extensions: &[String],
    application_api: u32,
) -> Features {
    let Some((supports_half, supports_dot)) = feature_structures(application_api, api, extensions)
    else {
        return Features::default();
    };
    let (Some(get_features2), Some(get_properties2)) = (
        instance.get_physical_device_features2,
        instance.get_physical_device_properties2,
    ) else {
        return Features::default();
    };
    let mut half = vk::PhysicalDeviceShaderFloat16Int8Features {
        s_type: vk::STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES,
        p_next: null_mut(),
        shader_float16: 0,
        shader_int8: 0,
    };
    let mut dot = vk::PhysicalDeviceShaderIntegerDotProductFeatures {
        s_type: vk::STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES,
        p_next: null_mut(),
        shader_integer_dot_product: 0,
    };
    let mut features = vk::PhysicalDeviceFeatures2 {
        s_type: vk::STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
        p_next: null_mut(),
        features: [0; 55],
    };
    if supports_half {
        half.p_next = features.p_next;
        features.p_next = (&raw mut half).cast();
    }
    if supports_dot {
        dot.p_next = features.p_next;
        features.p_next = (&raw mut dot).cast();
    }
    // SAFETY: Vulkan 1.1 query with a valid live device. Only supported core
    // or advertised extension feature structures are chained and stay live.
    unsafe { get_features2(device, &raw mut features) };
    let mut subgroup = vk::PhysicalDeviceSubgroupProperties {
        s_type: vk::STRUCTURE_TYPE_PHYSICAL_DEVICE_SUBGROUP_PROPERTIES,
        p_next: null_mut(),
        subgroup_size: 0,
        supported_stages: 0,
        supported_operations: 0,
        quad_operations_in_all_stages: 0,
    };
    let mut properties2 = vk::PhysicalDeviceProperties2 {
        s_type: vk::STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,
        p_next: (&raw mut subgroup).cast(),
        properties: vk::PhysicalDeviceProperties::default(),
    };
    // SAFETY: Both structures are supported core 1.1 structures and live
    // throughout the read-only properties query against this valid device.
    unsafe { get_properties2(device, &raw mut properties2) };
    Features {
        float16: supports_half.then_some(half.shader_float16 != 0),
        integer_dot: supports_dot.then_some(dot.shader_integer_dot_product != 0),
        subgroup_size: Some(subgroup.subgroup_size),
        subgroup_operations: Some(subgroup.supported_operations),
        subgroup_stages: Some(subgroup.supported_stages),
    }
}

fn adapter(
    instance: &Instance,
    device: vk::PhysicalDevice,
    application_api: u32,
) -> io::Result<VulkanAdapter> {
    let mut properties = vk::PhysicalDeviceProperties::default();
    // SAFETY: device was enumerated from this live instance; the read-only
    // query initializes `properties` and retains no borrowed storage.
    unsafe { (instance.get_physical_device_properties)(device, &raw mut properties) };
    let mut extensions = vec![vk::ExtensionProperties::default(); MAX_EXTENSIONS];
    let mut count = u32::try_from(extensions.len()).map_err(|_| invalid())?;
    // SAFETY: Valid instance/device, no layer filter, initialized output storage
    // for count entries. The driver may only write that many entries.
    check(unsafe {
        (instance.enumerate_device_extension_properties)(
            device,
            null(),
            &raw mut count,
            extensions.as_mut_ptr(),
        )
    })?;
    let count = usize::try_from(count).map_err(|_| invalid())?;
    if count > extensions.len() {
        return Err(invalid());
    }
    extensions.truncate(count);
    let mut extensions: Vec<_> = extensions
        .iter()
        .map(|extension| text(&extension.extension_name))
        .collect::<io::Result<_>>()?;
    extensions.sort_unstable();

    let mut queues = [vk::QueueFamilyProperties::default(); MAX_QUEUES];
    let mut queue_count = 0;
    // SAFETY: Valid device and writable count; null storage requests only count.
    unsafe {
        (instance.get_physical_device_queue_family_properties)(
            device,
            &raw mut queue_count,
            null_mut(),
        );
    };
    if usize::try_from(queue_count).map_err(|_| invalid())? > queues.len() {
        return Err(invalid());
    }
    // SAFETY: The bounded initialized output array accommodates the queried
    // count. Vulkan writes at most the input count and retains no pointers.
    unsafe {
        (instance.get_physical_device_queue_family_properties)(
            device,
            &raw mut queue_count,
            queues.as_mut_ptr(),
        );
    };
    let queue_count = usize::try_from(queue_count).map_err(|_| invalid())?;
    let compute_queues = queues
        .get(..queue_count)
        .ok_or_else(invalid)?
        .iter()
        .enumerate()
        .filter(|(_, queue)| queue.queue_flags & vk::QUEUE_COMPUTE_BIT != 0)
        .map(|(index, queue)| u32::try_from(index).map(|index| (index, queue.queue_count)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid())?;

    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    // SAFETY: Valid device from this live instance. Returned fixed-size memory
    // properties are fully initialized by Vulkan and do not borrow driver data.
    unsafe { (instance.get_physical_device_memory_properties)(device, &raw mut memory) };
    let heaps = usize::try_from(memory.heap_count).map_err(|_| invalid())?;
    let memory_heaps = memory
        .heaps
        .get(..heaps)
        .ok_or_else(invalid)?
        .iter()
        .map(|heap| (heap.size, heap.flags & vk::MEMORY_HEAP_DEVICE_LOCAL_BIT != 0))
        .collect();

    let features = features(
        instance,
        device,
        properties.api_version,
        &extensions,
        application_api,
    );
    let (device_type, hardware_gpu) = device_type(properties.device_type);
    Ok(VulkanAdapter {
        name: text(&properties.device_name)?,
        vendor_id: properties.vendor_id,
        device_id: properties.device_id,
        driver_version: properties.driver_version,
        api_version: properties.api_version,
        device_type,
        hardware_gpu,
        compute_queues,
        extensions,
        memory_heaps,
        shader_float16: features.float16,
        shader_integer_dot_product: features.integer_dot,
        subgroup_size: features.subgroup_size,
        subgroup_operations: features.subgroup_operations,
        subgroup_stages: features.subgroup_stages,
    })
}

pub(super) fn query() -> io::Result<VulkanInventory> {
    if !cfg!(target_arch = "aarch64")
        || std::env::vars_os().any(|(name, _)| {
            name.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("VK_")
        })
    {
        return Err(failure(
            "eligibility",
            None,
            None,
            None,
            io::ErrorKind::Unsupported,
        ));
    }
    let loader =
        Loader::load().map_err(|error| failure("system-loader", None, None, None, error.kind()))?;
    let loader_api_version = loader.instance_version().map_err(|code| {
        failure(
            "loader-version",
            Some(code),
            None,
            None,
            io::ErrorKind::Other,
        )
    })?;
    let application_api = loader_api_version.min(vk::API_VERSION_1_3);
    let (instance, instance_modules) =
        create_instance(&loader, loader_api_version, application_api)?;
    let mut devices: [vk::PhysicalDevice; MAX_ADAPTERS] = [null_mut(); MAX_ADAPTERS];
    let mut count = u32::try_from(devices.len()).map_err(|_| invalid())?;
    // SAFETY: Valid instance and initialized storage for the advertised count.
    // SUCCESS is required; INCOMPLETE rejects rather than growing without bound.
    let result = unsafe {
        (instance.enumerate_physical_devices)(
            instance.handle,
            &raw mut count,
            devices.as_mut_ptr(),
        )
    };
    if result != vk::SUCCESS {
        return Err(failure(
            "physical-device-enumeration",
            Some(result),
            Some(loader_api_version),
            Some(application_api),
            io::ErrorKind::Other,
        ));
    }
    let count = usize::try_from(count).map_err(|_| invalid())?;
    let adapters = devices
        .get(..count)
        .ok_or_else(invalid)?
        .iter()
        .map(|device| {
            adapter(&instance, *device, application_api).map_err(|error| {
                let result = error
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<VulkanQueryFailure>())
                    .and_then(|failure| failure.vulkan_result);
                failure(
                    "adapter-properties",
                    result,
                    Some(loader_api_version),
                    Some(application_api),
                    error.kind(),
                )
            })
        })
        .collect::<io::Result<_>>()?;
    Ok(VulkanInventory {
        loader_api_version,
        application_api_version: application_api,
        instance_modules,
        adapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_file_candidates_prioritize_loader_and_bound_new_images() {
        let mut modules: Vec<_> = (0..32)
            .map(|index| (format!("driver-{index:02}.dll"), ()))
            .collect();
        modules.push(("vulkan-1.dll".into(), ()));
        let before = vec!["driver-00.dll".into(), "vulkan-1.dll".into()];
        let selected = file_candidate_indices(Some(&before), &modules);
        assert_eq!(selected.len(), 16);
        assert_eq!(selected[0], 32);
        assert!(!selected.contains(&0));
        assert_eq!(file_candidate_indices(None, &modules), vec![32]);
        assert_eq!(
            file_candidate_indices::<()>(Some(&[]), &[]),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn module_snapshots_reject_truncation_paths_controls_and_invalid_utf16() {
        assert_eq!(
            module_name(&[65, 46, 68, 76, 76, 0], 5),
            Some("a.dll".into())
        );
        for buffer in [
            vec![0],
            vec![65],
            vec![0xD800, 0],
            vec![10, 0],
            vec![92, 0],
            vec![47, 0],
            vec![58, 0],
        ] {
            assert!(module_name(&buffer, buffer.len().saturating_sub(1)).is_none());
        }
        assert!(module_name(&[65, 66], 2).is_none());
        assert!(module_name(&[65, 0], 3).is_none());
    }

    #[test]
    fn vulkan_failure_payload_preserves_raw_results_and_unknown_versions() -> io::Result<()> {
        let error = check(vk::ERROR_INCOMPATIBLE_DRIVER)
            .err()
            .ok_or_else(invalid)?;
        let context = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<VulkanQueryFailure>())
            .ok_or_else(invalid)?;
        assert_eq!(context.stage, "device-query");
        assert_eq!(context.vulkan_result, Some(-9));
        assert_eq!(context.loader_api_version, None);
        assert_eq!(context.application_api_version, None);
        assert_eq!(context.graphics_adapter_count, None);
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.to_string(),
            "Vulkan inventory failed at device-query (VkResult -9)"
        );
        Ok(())
    }

    #[test]
    fn graphics_count_failure_never_exposes_undefined_adapter_count() {
        let success = graphics_count_result(0, 2);
        assert_eq!(success.ntstatus, Some(0));
        assert_eq!(success.windows_error, None);
        assert_eq!(success.adapter_count, Some(2));
        assert_eq!(graphics_count_result(0, 0).adapter_count, Some(0));
        for status in [-1, i32::MIN, 1] {
            let failure = graphics_count_result(status, u32::MAX);
            assert_eq!(failure.stage, "count-query");
            assert_eq!(failure.windows_error, None);
            assert_eq!(failure.ntstatus, Some(status));
            assert_eq!(failure.adapter_count, None);
        }
    }

    #[test]
    fn vulkan_names_and_device_types_preserve_unqualified_boundaries() -> io::Result<()> {
        assert!(check(vk::SUCCESS).is_ok());
        assert!(check(vk::INCOMPLETE).is_err());
        assert!(check(vk::ERROR_INCOMPATIBLE_DRIVER).is_err());
        assert_eq!(text(&[65, 0, 66])?, "A");
        assert!(text(&[65, 66]).is_err());
        assert!(text(&[10, 0]).is_err());
        assert_eq!(device_type(vk::PHYSICAL_DEVICE_TYPE_CPU), ("cpu", false));
        assert_eq!(
            device_type(vk::PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU),
            ("virtual-gpu", false)
        );
        assert_eq!(
            device_type(vk::PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU),
            ("integrated-gpu", true)
        );
        Ok(())
    }

    #[test]
    fn vulkan_feature_structures_require_application_and_device_eligibility() {
        assert_eq!(
            feature_structures(vk::API_VERSION_1_1, vk::API_VERSION_1_3, &[]),
            Some((false, false))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_3, vk::API_VERSION_1_1, &[]),
            Some((false, false))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_3, vk::API_VERSION_1_3, &[]),
            Some((true, true))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_0, vk::API_VERSION_1_3, &[]),
            None
        );
        assert_eq!(
            feature_structures(
                vk::API_VERSION_1_1,
                vk::API_VERSION_1_1,
                &[
                    "VK_KHR_shader_float16_int8".into(),
                    "VK_KHR_shader_integer_dot_product".into()
                ]
            ),
            Some((true, true))
        );
    }
}