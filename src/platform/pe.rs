//! PE parsing. Source: `OSTPlatform/Windows/PE.cpp`
//!
//! We define our own `IMAGE_*` structs rather than using
//! `windows-sys`'s, because the Win32 headers place them in several
//! different feature-gated modules (`SystemServices`, `Diagnostics::Debug`)
//! and the paths differ across crate versions. The PE format is a
//! documented, stable ABI; declaring the layouts locally is the honest
//! translation and removes a class of feature-gate errors.

use std::path::{Path, PathBuf};

// ─── PE constants ───────────────────────────────────────────────────

const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D;      // "MZ"
const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550;  // "PE\0\0"
const IMAGE_NT_OPTIONAL_HDR32_MAGIC: u16 = 0x010B;
const IMAGE_NT_OPTIONAL_HDR64_MAGIC: u16 = 0x020B;
const IMAGE_DIRECTORY_ENTRY_EXPORT: usize = 0;

const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_WRITE:   u32 = 0x8000_0000;

/// Header read chunk, matching the C++ `kHeaderReadBytes`.
const HEADER_READ_BYTES: usize = 4 * 1024 * 1024;

// ─── PE structs ─────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageDosHeader {
    e_magic: u16,
    e_cblp: u16,
    e_cp: u16,
    e_crlc: u16,
    e_cparhdr: u16,
    e_minalloc: u16,
    e_maxalloc: u16,
    e_ss: u16,
    e_sp: u16,
    e_csum: u16,
    e_ip: u16,
    e_cs: u16,
    e_lfarlc: u16,
    e_ovno: u16,
    e_res: [u16; 4],
    e_oemid: u16,
    e_oeminfo: u16,
    e_res2: [u16; 10],
    e_lfanew: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageFileHeader {
    machine: u16,
    number_of_sections: u16,
    time_date_stamp: u32,
    pointer_to_symbol_table: u32,
    number_of_symbols: u32,
    size_of_optional_header: u16,
    characteristics: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageDataDirectory {
    virtual_address: u32,
    size: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageOptionalHeader64 {
    magic: u16,
    major_linker_version: u8,
    minor_linker_version: u8,
    size_of_code: u32,
    size_of_initialized_data: u32,
    size_of_uninitialized_data: u32,
    address_of_entry_point: u32,
    base_of_code: u32,
    image_base: u64,
    section_alignment: u32,
    file_alignment: u32,
    major_operating_system_version: u16,
    minor_operating_system_version: u16,
    major_image_version: u16,
    minor_image_version: u16,
    major_subsystem_version: u16,
    minor_subsystem_version: u16,
    win32_version_value: u32,
    size_of_image: u32,
    size_of_headers: u32,
    checksum: u32,
    subsystem: u16,
    dll_characteristics: u16,
    size_of_stack_reserve: u64,
    size_of_stack_commit: u64,
    size_of_heap_reserve: u64,
    size_of_heap_commit: u64,
    loader_flags: u32,
    number_of_rva_and_sizes: u32,
    // We only need the first entry; the actual array is larger.
    data_directory: [ImageDataDirectory; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageOptionalHeader32 {
    magic: u16,
    major_linker_version: u8,
    minor_linker_version: u8,
    size_of_code: u32,
    size_of_initialized_data: u32,
    size_of_uninitialized_data: u32,
    address_of_entry_point: u32,
    base_of_code: u32,
    base_of_data: u32,
    image_base: u32,
    section_alignment: u32,
    file_alignment: u32,
    major_operating_system_version: u16,
    minor_operating_system_version: u16,
    major_image_version: u16,
    minor_image_version: u16,
    major_subsystem_version: u16,
    minor_subsystem_version: u16,
    win32_version_value: u32,
    size_of_image: u32,
    size_of_headers: u32,
    checksum: u32,
    subsystem: u16,
    dll_characteristics: u16,
    size_of_stack_reserve: u32,
    size_of_stack_commit: u32,
    size_of_heap_reserve: u32,
    size_of_heap_commit: u32,
    loader_flags: u32,
    number_of_rva_and_sizes: u32,
    data_directory: [ImageDataDirectory; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageSectionHeader {
    name: [u8; 8],
    virtual_size: u32,
    virtual_address: u32,
    size_of_raw_data: u32,
    pointer_to_raw_data: u32,
    pointer_to_relocations: u32,
    pointer_to_linenumbers: u32,
    number_of_relocations: u16,
    number_of_linenumbers: u16,
    characteristics: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct ImageExportDirectory {
    characteristics: u32,
    time_date_stamp: u32,
    major_version: u16,
    minor_version: u16,
    name: u32,
    base: u32,
    number_of_functions: u32,
    number_of_names: u32,
    address_of_functions: u32,
    address_of_names: u32,
    address_of_name_ordinals: u32,
}

// ─── Safe helpers for reading unaligned POD from a byte slice ──────

/// Read a `T` from `bytes[offset..]`. Returns `None` if the slice is
/// too short. `T: Copy` with no references inside; the source bytes
/// come from a heap buffer that may not be aligned for `T`, so this
/// uses `read_unaligned`.
unsafe fn pod_at<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    if offset > bytes.len() || std::mem::size_of::<T>() > bytes.len() - offset {
        return None;
    }
    let p = bytes.as_ptr().add(offset) as *const T;
    Some(std::ptr::read_unaligned(p))
}

// ─── ByteBuffer ─────────────────────────────────────────────────────

/// Owning byte buffer. `Vec<u8>` is zero-filled at allocation; the C++
/// uses `make_unique_for_overwrite` for uninitialized storage. The
/// difference is a `memset` at allocation; the read path immediately
/// overwrites every byte.
#[derive(Clone, Debug, Default)]
pub struct ByteBuffer {
    data: Vec<u8>,
}

impl ByteBuffer {
    pub fn new() -> Self { Self::default() }
    pub fn with_size(size: usize) -> Self { Self { data: vec![0u8; size] } }
    pub fn as_slice(&self) -> &[u8] { &self.data }
    pub fn as_mut_slice(&mut self) -> &mut [u8] { &mut self.data }
    pub fn len(&self) -> usize { self.data.len() }
    pub fn is_empty(&self) -> bool { self.data.is_empty() }
    pub fn resize(&mut self, size: usize) { self.data.resize(size, 0); }
}

impl AsRef<[u8]> for ByteBuffer {
    fn as_ref(&self) -> &[u8] { &self.data }
}

// ─── Section / Export ───────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct Section {
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn contains_rva(&self, rva: u32) -> bool {
        let size = self.virtual_size.max(self.raw_size);
        let end = self.virtual_address.wrapping_add(size);
        end >= self.virtual_address && rva >= self.virtual_address && rva < end
    }
    pub fn is_executable(&self) -> bool { (self.characteristics & IMAGE_SCN_MEM_EXECUTE) != 0 }
    pub fn is_writable(&self) -> bool { (self.characteristics & IMAGE_SCN_MEM_WRITE) != 0 }
}

#[derive(Clone, Debug, Default)]
pub struct Export {
    pub rva: u32,
    pub forwarder: String,
}

impl Export {
    pub fn is_forwarder(&self) -> bool { !self.forwarder.is_empty() }
}

// ─── Image ──────────────────────────────────────────────────────────

pub struct Image {
    path: PathBuf,
    header_bytes: ByteBuffer,
    sections: Vec<Section>,
    file_size: u64,
    entry_point_rva: u32,
    export_directory_rva: u32,
    export_directory_size: u32,
    valid: bool,
}

impl Image {
    pub fn new(path: &Path) -> Self {
        let mut img = Self {
            path: path.to_path_buf(),
            header_bytes: ByteBuffer::new(),
            sections: Vec::new(),
            file_size: 0,
            entry_point_rva: 0,
            export_directory_rva: 0,
            export_directory_size: 0,
            valid: false,
        };
        img.parse_header();
        img
    }

    fn parse_header(&mut self) {
        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return,
        };
        self.file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        if self.file_size == 0 { return; }

        // Read at most HEADER_READ_BYTES.
        use std::io::Read;
        let read_len = (self.file_size as usize).min(HEADER_READ_BYTES);
        self.header_bytes.resize(read_len);
        let mut f = file;
        if f.read_exact(self.header_bytes.as_mut_slice()).is_err() {
            return;
        }

        let bytes = self.header_bytes.as_slice();

        // DOS header
        let dos: ImageDosHeader = match unsafe { pod_at(bytes, 0) } {
            Some(d) => d,
            None => return,
        };
        if dos.e_magic != IMAGE_DOS_SIGNATURE || dos.e_lfanew < 0 { return; }

        let nt_offset = dos.e_lfanew as usize;
        let sig: u32 = match unsafe { pod_at(bytes, nt_offset) } {
            Some(s) => s,
            None => return,
        };
        if sig != IMAGE_NT_SIGNATURE { return; }

        let fh_off = nt_offset + 4;
        let fh: ImageFileHeader = match unsafe { pod_at(bytes, fh_off) } {
            Some(f) => f,
            None => return,
        };
        if fh.number_of_sections == 0 { return; }

        let opt_off = fh_off + std::mem::size_of::<ImageFileHeader>();
        let magic: u16 = match unsafe { pod_at(bytes, opt_off) } {
            Some(m) => m,
            None => return,
        };

        let export_dir: ImageDataDirectory;
        if magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC {
            let opt: ImageOptionalHeader64 = match unsafe { pod_at(bytes, opt_off) } {
                Some(o) => o,
                None => return,
            };
            self.entry_point_rva = opt.address_of_entry_point;
            export_dir = opt.data_directory[IMAGE_DIRECTORY_ENTRY_EXPORT];
        } else if magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC {
            let opt: ImageOptionalHeader32 = match unsafe { pod_at(bytes, opt_off) } {
                Some(o) => o,
                None => return,
            };
            self.entry_point_rva = opt.address_of_entry_point;
            export_dir = opt.data_directory[IMAGE_DIRECTORY_ENTRY_EXPORT];
        } else {
            return;
        }

        let sec_off = opt_off + fh.size_of_optional_header as usize;
        let sec_end = sec_off + (fh.number_of_sections as usize) * std::mem::size_of::<ImageSectionHeader>();
        if sec_end > bytes.len() { return; }

        self.sections.reserve(fh.number_of_sections as usize);
        for i in 0..fh.number_of_sections as usize {
            let off = sec_off + i * std::mem::size_of::<ImageSectionHeader>();
            let sh: ImageSectionHeader = match unsafe { pod_at(bytes, off) } {
                Some(s) => s,
                None => return,
            };
            // Section name is a fixed 8-byte array with optional NUL padding.
            let name_end = sh.name.iter().position(|&b| b == 0).unwrap_or(8);
            let name = String::from_utf8_lossy(&sh.name[..name_end]).into_owned();

            self.sections.push(Section {
                name,
                virtual_address: sh.virtual_address,
                virtual_size: sh.virtual_size,
                raw_offset: sh.pointer_to_raw_data,
                raw_size: sh.size_of_raw_data,
                characteristics: sh.characteristics,
            });
        }

        self.export_directory_rva = export_dir.virtual_address;
        self.export_directory_size = export_dir.size;
        self.valid = true;
    }

    pub fn is_valid(&self) -> bool { self.valid }
    pub fn path(&self) -> &Path { &self.path }
    pub fn file_size(&self) -> u64 { self.file_size }
    pub fn entry_point_rva(&self) -> u32 { self.entry_point_rva }
    pub fn sections(&self) -> &[Section] { &self.sections }

    pub fn has_section(&self, name: &str) -> bool {
        self.sections.iter().any(|s| s.name == name)
    }

    pub fn section_containing_rva(&self, rva: u32) -> Option<&Section> {
        self.sections.iter().find(|s| s.contains_rva(rva))
    }

    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        let s = self.section_containing_rva(rva)?;
        let delta = rva.wrapping_sub(s.virtual_address);
        if delta >= s.raw_size { return None; }
        let off = s.raw_offset as usize + delta as usize;
        if (off as u64) < self.file_size { Some(off) } else { None }
    }

    pub fn raw_offset_to_rva(&self, raw_offset: usize) -> Option<u32> {
        for s in &self.sections {
            let start = s.raw_offset as usize;
            let end = start.checked_add(s.raw_size as usize)?;
            if raw_offset < start || raw_offset >= end { continue; }
            let delta = (raw_offset - start) as u32;
            return s.virtual_address.checked_add(delta);
        }
        None
    }

    /// Read raw bytes from the file. Returns an empty buffer on failure.
    pub fn read_raw_bytes(&self, offset: usize, size: usize) -> ByteBuffer {
        use std::io::{Read, Seek, SeekFrom};
        let mut out = ByteBuffer::with_size(size);
        let mut f = match std::fs::File::open(&self.path) { Ok(f) => f, Err(_) => return ByteBuffer::new() };
        if f.seek(SeekFrom::Start(offset as u64)).is_err() { return ByteBuffer::new(); }
        let mut done = 0usize;
        while done < size {
            match f.read(&mut out.as_mut_slice()[done..]) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(_) => return ByteBuffer::new(),
            }
        }
        out.resize(done);
        out
    }

    pub fn read_all_bytes(&self) -> ByteBuffer {
        use std::io::Read;
        let mut out = ByteBuffer::new();
        let mut f = match std::fs::File::open(&self.path) { Ok(f) => f, Err(_) => return out };
        let _ = f.read_to_end(&mut out.data);
        out
    }

    pub fn find_export(&self, symbol_name: &str) -> Option<Export> {
        if !self.valid || self.export_directory_rva == 0 || self.export_directory_size == 0 {
            return None;
        }
        let export_sec = self.section_containing_rva(self.export_directory_rva)?;
        let export_bytes = self.read_raw_bytes(export_sec.raw_offset as usize, export_sec.raw_size as usize);
        if export_bytes.is_empty() { return None; }
        let view = export_bytes.as_slice();

        // Helper: view offset within the export section for a given RVA.
        let view_at_rva = |rva: u32, min_size: usize| -> Option<&[u8]> {
            let raw = self.rva_to_offset(rva)?;
            if raw < export_sec.raw_offset as usize { return None; }
            let local = raw - export_sec.raw_offset as usize;
            if local > view.len() || min_size > view.len() - local { return None; }
            Some(&view[local..])
        };

        let dir_local = {
            let raw = self.rva_to_offset(self.export_directory_rva)?;
            if raw < export_sec.raw_offset as usize { return None; }
            raw - export_sec.raw_offset as usize
        };
        let exports: ImageExportDirectory = unsafe { pod_at(view, dir_local) }?;
        if exports.number_of_names == 0 || exports.number_of_functions == 0 { return None; }

        let names_view = view_at_rva(exports.address_of_names,
            exports.number_of_names as usize * 4)?;
        let ordinals_view = view_at_rva(exports.address_of_name_ordinals,
            exports.number_of_names as usize * 2)?;
        let functions_view = view_at_rva(exports.address_of_functions,
            exports.number_of_functions as usize * 4)?;

        for i in 0..exports.number_of_names as usize {
            let name_rva: u32 = unsafe { pod_at(names_view, i * 4) }?;
            let name_bytes = view_at_rva(name_rva, 1)?;
            let nul = name_bytes.iter().position(|&b| b == 0)?;
            let name = std::str::from_utf8(&name_bytes[..nul]).ok()?;
            if name != symbol_name { continue; }

            let ord: u16 = unsafe { pod_at(ordinals_view, i * 2) }?;
            if ord as u32 >= exports.number_of_functions { return None; }
            let func_rva: u32 = unsafe { pod_at(functions_view, ord as usize * 4) }?;
            if func_rva == 0 { return None; }

            let mut result = Export { rva: func_rva, forwarder: String::new() };
            let export_end = self.export_directory_rva.checked_add(self.export_directory_size)?;
            if func_rva >= self.export_directory_rva && func_rva < export_end {
                let fwd_bytes = view_at_rva(func_rva, 1)?;
                let nul = fwd_bytes.iter().position(|&b| b == 0)?;
                result.forwarder = std::str::from_utf8(&fwd_bytes[..nul]).ok()?.to_string();
            }
            return Some(result);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let b = ByteBuffer::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn sized_buffer() {
        let b = ByteBuffer::with_size(64);
        assert_eq!(b.len(), 64);
        assert!(!b.is_empty());
    }

    #[test]
    fn section_rva_containment() {
        let s = Section {
            name: ".text".to_string(),
            virtual_address: 0x1000,
            virtual_size: 0x2000,
            raw_offset: 0x400,
            raw_size: 0x2000,
            characteristics: 0x6000_0020,
        };
        assert!(s.contains_rva(0x1000));
        assert!(s.contains_rva(0x2FFF));
        assert!(!s.contains_rva(0x3000));
        assert!(!s.contains_rva(0x0FFF));
        assert!(s.is_executable());
        assert!(!s.is_writable());
    }

    #[test]
    fn section_writable_flag() {
        let s = Section { characteristics: 0x8000_0000, ..Default::default() };
        assert!(s.is_writable());
        assert!(!s.is_executable());
    }

    #[test]
    fn invalid_image_path_is_not_valid() {
        let img = Image::new(Path::new("definitely-not-a-real-file.exe"));
        assert!(!img.is_valid());
        assert!(img.sections().is_empty());
    }

    #[test]
    fn export_forwarder_detection() {
        let e = Export { rva: 0x1234, forwarder: "KERNEL32.Sleep".to_string() };
        assert!(e.is_forwarder());
        let e2 = Export { rva: 0x1234, forwarder: String::new() };
        assert!(!e2.is_forwarder());
    }

    #[cfg(windows)]
    #[test]
    fn parses_kernel32() {
        // kernel32.dll is always present and has a full export table.
        let sysdir = crate::platform::dynamic_library::get_system_directory_path();
        if sysdir.is_empty() { return; }
        let path = Path::new(&sysdir).join("kernel32.dll");
        if !path.exists() { return; }

        let img = Image::new(&path);
        assert!(img.is_valid(), "kernel32.dll failed to parse");
        assert!(img.sections().len() > 1);
        assert!(img.has_section(".text"));

        let export = img.find_export("LoadLibraryW");
        assert!(export.is_some(), "LoadLibraryW not found in kernel32 exports");
        let e = export.unwrap();
        assert!(e.rva != 0);
    }

    #[cfg(windows)]
    #[test]
    fn rva_to_offset_round_trip() {
        let sysdir = crate::platform::dynamic_library::get_system_directory_path();
        if sysdir.is_empty() { return; }
        let path = Path::new(&sysdir).join("kernel32.dll");
        if !path.exists() { return; }
        let img = Image::new(&path);
        if !img.is_valid() { return; }

        // Pick the first section and a small offset inside it.
        let sec = &img.sections()[0];
        let rva = sec.virtual_address + 0x10;
        let raw = img.rva_to_offset(rva);
        assert!(raw.is_some());
        let back = img.raw_offset_to_rva(raw.unwrap());
        assert_eq!(back, Some(rva));
    }
}
