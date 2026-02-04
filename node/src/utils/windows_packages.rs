#[cfg(windows)]
use windows::{
    Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS},
    Win32::Storage::Packaging::Appx::{FindPackagesByPackageFamily, GetPackagePathByFullName},
    core::{PCWSTR, PWSTR},
};

#[cfg(windows)]
pub fn get_package_install_path(package_family_name: &str) -> Option<String> {
    let wide_family_name: Vec<u16> = package_family_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut count: u32 = 0;
    let mut buffer_length: u32 = 0;

    unsafe {
        let result = FindPackagesByPackageFamily(
            PCWSTR(wide_family_name.as_ptr()),
            //
            // PACKAGE_FILTER_HEAD.
            //
            0x00000010,
            &mut count,
            None,
            &mut buffer_length,
            None,
            None,
        );

        //
        // ERROR_INSUFFICIENT_BUFFER is expected on first call.
        //
        if result != ERROR_INSUFFICIENT_BUFFER && count == 0 {
            return None;
        }

        let mut package_full_names: Vec<PWSTR> = vec![PWSTR::null(); count as usize];
        let mut buffer: Vec<u16> = vec![0u16; buffer_length as usize];

        let result = FindPackagesByPackageFamily(
            PCWSTR(wide_family_name.as_ptr()),
            //
            // PACKAGE_FILTER_HEAD.
            //
            0x00000010,
            &mut count,
            Some(package_full_names.as_mut_ptr()),
            &mut buffer_length,
            Some(PWSTR(buffer.as_mut_ptr())),
            None,
        );

        if result.is_err() {
            return None;
        }

        let full_name_ptr = package_full_names[0];

        let mut path_length: u32 = 0;
        let result =
            GetPackagePathByFullName(PCWSTR(full_name_ptr.as_ptr()), &mut path_length, None);

        if result != ERROR_INSUFFICIENT_BUFFER || path_length == 0 {
            return None;
        }

        let mut path_buffer: Vec<u16> = vec![0u16; path_length as usize];

        //
        // Second call to get the actual path.
        //
        let result = GetPackagePathByFullName(
            PCWSTR(full_name_ptr.as_ptr()),
            &mut path_length,
            Some(PWSTR(path_buffer.as_mut_ptr())),
        );

        if result != ERROR_SUCCESS {
            return None;
        }

        //
        // Convert wide string to Rust String (exclude null terminator).
        //
        let path = String::from_utf16_lossy(&path_buffer[..path_length.saturating_sub(1) as usize]);
        return Some(path);
    }

    #[allow(unreachable_code)]
    None
}
