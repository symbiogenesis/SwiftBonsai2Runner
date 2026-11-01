//! Read-only, bounded `SetupAPI` inventories; presence does not establish signed deployment.

use std::io;
use std::ptr;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DICS_FLAG_GLOBAL, DIGCF_PRESENT, DIREG_DRV, GUID_DEVCLASS_COMPUTEACCELERATOR,
    GUID_DEVCLASS_DISPLAY, GUID_DEVCLASS_MEDIA, HDEVINFO, SP_DEVINFO_DATA, SPDRP_DEVICEDESC,
    SPDRP_MFG, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceRegistryPropertyW, SetupDiOpenDevRegKey,
};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Registry::{
    HKEY, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW,
};

struct Set(HDEVINFO);
impl Drop for Set {
    fn drop(&mut self) {
        // SAFETY: The SetupAPI set is owned exactly once and outlives every use.
        let _ = unsafe { SetupDiDestroyDeviceInfoList(self.0) };
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: The registry handle is owned exactly once and is no longer borrowed.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}
fn text(buffer: &[u16]) -> io::Result<String> {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| super::invalid("Unterminated device property."))?;
    String::from_utf16(&buffer[..end])
        .map_err(|_| super::invalid("Invalid device property UTF-16."))
}
fn property(set: &Set, entry: &SP_DEVINFO_DATA, property: u32) -> Option<String> {
    let mut buffer = [0_u16; 2048];
    let mut kind = 0;
    // SAFETY: The live set/entry are borrowed and the property destination is
    // writable for exactly the bounded UTF-16 array's byte size.
    let read = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set.0,
            entry,
            property,
            &raw mut kind,
            buffer.as_mut_ptr().cast(),
            4096,
            ptr::null_mut(),
        )
    };
    (read != 0 && kind == 1)
        .then(|| text(&buffer).ok())
        .flatten()
}
fn registry(key: &Key, name: &str) -> Option<String> {
    let name: Vec<_> = name.encode_utf16().chain([0]).collect();
    let mut buffer = [0_u16; 2048];
    let mut size = 4096_u32;
    // SAFETY: The owned key and terminated value name remain live; the writable
    // destination has the declared byte capacity. RegGetValue ensures termination.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    (status == 0).then(|| text(&buffer).ok()).flatten()
}

pub(crate) fn inventory(class: &str) -> io::Result<Vec<crate::DeviceInventoryEntry>> {
    let guid = match class {
        "ComputeAccelerator" => GUID_DEVCLASS_COMPUTEACCELERATOR,
        "Display" => GUID_DEVCLASS_DISPLAY,
        "Media" => GUID_DEVCLASS_MEDIA,
        _ => return Err(super::invalid("Unsupported read-only device class.")),
    };
    // SAFETY: The static GUID is read-only and no optional enumerator/window is supplied.
    let raw = unsafe {
        SetupDiGetClassDevsW(&raw const guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT)
    };
    if raw == -1 {
        return Err(io::Error::last_os_error());
    }
    let set = Set(raw);
    let mut devices = Vec::new();
    for index in 0..=16 {
        let mut entry = SP_DEVINFO_DATA {
            cbSize: u32::try_from(size_of::<SP_DEVINFO_DATA>())
                .map_err(|_| super::invalid("Device entry overflow."))?,
            ..SP_DEVINFO_DATA::default()
        };
        // SAFETY: set is live and entry has its initialized exact structure size.
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &raw mut entry) } == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(259) {
                Ok(devices)
            } else {
                Err(error)
            };
        }
        if index == 16 {
            return Err(super::invalid(
                "Device inventory exceeds sixteen present entries.",
            ));
        }
        let mut id = [0_u16; 2048];
        // SAFETY: The live set/entry and writable bounded UTF-16 output remain allocated.
        super::check(unsafe {
            SetupDiGetDeviceInstanceIdW(
                set.0,
                &raw const entry,
                id.as_mut_ptr(),
                2048,
                ptr::null_mut(),
            )
        })?;
        // SAFETY: The selected live device's driver key is opened read-only.
        let raw_key = unsafe {
            SetupDiOpenDevRegKey(
                set.0,
                &raw const entry,
                DICS_FLAG_GLOBAL,
                0,
                DIREG_DRV,
                KEY_READ,
            )
        };
        let key = (raw_key != INVALID_HANDLE_VALUE && !raw_key.is_null()).then(|| Key(raw_key));
        devices.push(crate::DeviceInventoryEntry {
            name: property(&set, &entry, SPDRP_DEVICEDESC),
            manufacturer: property(&set, &entry, SPDRP_MFG),
            device_id: text(&id)?,
            driver_version: key.as_ref().and_then(|key| registry(key, "DriverVersion")),
            driver_date: key.as_ref().and_then(|key| registry(key, "DriverDate")),
            inf_name: key.as_ref().and_then(|key| registry(key, "InfPath")),
        });
    }
    Err(super::invalid("Device inventory bound was exceeded."))
}
