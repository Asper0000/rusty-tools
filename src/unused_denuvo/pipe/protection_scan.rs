//! Denuvo-specific module scanner. Source:
//! `Pipe/Features/DenuvoAuth/ProtectionScan.cpp`.
//!
//! ProcessInspector stays generic; this file owns only the current
//! "is this process protected" policy.

use std::path::PathBuf;

use crate::platform::byte_search;
use crate::platform::pe::Image;
use crate::platform::process;
use crate::platform::stopwatch::Stopwatch;
use crate::steam::types::AppId_t;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DetectionMethod {
    None,
    LegacySectionString,
    OepPattern,
    ProtectedBlobSection,
}

pub fn to_string(method: DetectionMethod) -> &'static str {
    match method {
        DetectionMethod::None => "None",
        DetectionMethod::LegacySectionString => "LegacySectionString",
        DetectionMethod::OepPattern => "OepPattern",
        DetectionMethod::ProtectedBlobSection => "ProtectedBlobSection",
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProtectionScanReport {
    pub denuvo_detected: bool,
    pub method: Option<DetectionMethod>,
    pub module_path: String,
    pub section_name: String,
    pub pid: u32,
    pub module_size: u32,
    pub entry_point_rva: u32,
    pub match_rva: u32,
    pub match_raw_offset: usize,
    pub scanned_modules: usize,
    pub elapsed_ms: f64,
}

impl ProtectionScanReport {
    pub fn debug_string(&self) -> String {
        format!(
            "[pid={}] denuvoDetected={} method={} module={} section={} moduleSize={} entryPointRva=0x{:X} matchRva=0x{:X} matchRaw=0x{:X} scannedModules={} elapsedMs={:.3}",
            self.pid,
            self.denuvo_detected,
            to_string(self.method.unwrap_or(DetectionMethod::None)),
            if self.module_path.is_empty() { "-" } else { &self.module_path },
            if self.section_name.is_empty() { "-" } else { &self.section_name },
            self.module_size,
            self.entry_point_rva,
            self.match_rva,
            self.match_raw_offset,
            self.scanned_modules,
            self.elapsed_ms,
        )
    }
}

// ── Constants ───────────────────────────────────────────────────────

const K_LEGACY_DENUVO_SECTIONS: [&str; 5] =
    [".arch", ".srdata", ".xpdata", ".xdata", ".xtls"];

const K_LEGACY_DENUVO_STRING: [u8; 6] = *b"DENUVO";

/// `48 B9` = mov rcx, imm64, immediately followed by "DENUVO\0\0".
const K_DENUVO_OEP_PATTERN: [u8; 10] =
    [0x48, 0xB9, 0x44, 0x4F, 0x44, 0x45, 0x4E, 0x55, 0x56, 0x4F];

const K_MIN_PACKED_MODULE_BYTES: u32 = 80 * 1024 * 1024;
const K_LEGACY_SCAN_CHUNK_BYTES: usize = 8 * 1024 * 1024;
const K_OEP_SCAN_CHUNK_BYTES: usize = 8 * 1024 * 1024;

const K_PROTECTOR_BLOB_MIN_BYTES: u32 = 4 * 1024 * 1024;
const K_PROTECTOR_BLOB_MIN_ENTROPY: f64 = 6.0;
const K_PROTECTOR_BLOB_HIGH_CONFIDENCE_ENTROPY: f64 = 7.0;
const K_PROTECTOR_BLOB_ENTROPY_SAMPLE_BYTES: usize = 8 * 1024 * 1024;

const K_STEAM_RUNTIME_MODULE_NAMES: [&str; 10] = [
    "steamclient.dll",
    "steamclient64.dll",
    "steam_api.dll",
    "steam_api64.dll",
    "tier0_s.dll",
    "tier0_s64.dll",
    "vstdlib_s.dll",
    "vstdlib_s64.dll",
    "gameoverlayrenderer.dll",
    "gameoverlayrenderer64.dll",
];

// ── Internal types ──────────────────────────────────────────────────

struct ModuleCandidate {
    path: String,
    native_path: PathBuf,
    size: u32,
    executable: bool,
    in_game_tree: bool,
    order: usize,
}

impl ModuleCandidate {
    fn debug_string(&self) -> String {
        format!(
            "path={} size={} executable={} inGameTree={} order={}",
            self.path, self.size, self.executable, self.in_game_tree, self.order,
        )
    }
}

#[derive(Clone, Debug, Default)]
struct DetectionMatch {
    method: Option<DetectionMethod>,
    section_name: String,
    entry_point_rva: u32,
    match_rva: u32,
    match_raw_offset: usize,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn section_entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() { return 0.0; }
    let mut counts = [0u64; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let n = bytes.len() as f64;
    let mut e = 0.0f64;
    for &c in counts.iter() {
        if c > 0 {
            let p = c as f64 / n;
            e -= p * p.log2();
        }
    }
    e
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn ends_with_insensitive(value: &str, suffix: &str) -> bool {
    if value.len() < suffix.len() { return false; }
    value.as_bytes()[value.len() - suffix.len()..]
        .eq_ignore_ascii_case(suffix.as_bytes())
}

fn base_name_from_path(path: &str) -> String {
    match path.rfind(|c| c == '\\' || c == '/') {
        Some(i) => path[i + 1..].to_string(),
        None => path.to_string(),
    }
}

fn directory_from_path(path: &str) -> String {
    match path.rfind(|c| c == '\\' || c == '/') {
        Some(i) => path[..=i].to_string(),
        None => String::new(),
    }
}

fn normalize_directory_prefix(path: &str) -> String {
    let mut s = path.to_ascii_lowercase().replace('/', "\\");
    if !s.is_empty() && !s.ends_with('\\') { s.push('\\'); }
    s
}

fn path_starts_with_insensitive(path: &str, prefix: &str) -> bool {
    if prefix.is_empty() { return false; }
    let p = path.to_ascii_lowercase().replace('/', "\\");
    p.starts_with(prefix)
}

fn is_steam_runtime_module_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    K_STEAM_RUNTIME_MODULE_NAMES.iter().any(|n| *n == lower)
}

fn find_legacy_denuvo_section(image: &Image) -> Option<&crate::platform::pe::Section> {
    for name in K_LEGACY_DENUVO_SECTIONS {
        for s in image.sections() {
            if s.name == name { return Some(s); }
        }
    }
    None
}

// ── Detection ───────────────────────────────────────────────────────

fn try_legacy_section_string(
    module: &ModuleCandidate,
    image: &Image,
    section: &crate::platform::pe::Section,
) -> Option<DetectionMatch> {
    let match_raw = byte_search::find_in_file(
        &module.native_path,
        &K_LEGACY_DENUVO_STRING,
        K_LEGACY_SCAN_CHUNK_BYTES,
    )?;
    let mut m = DetectionMatch::default();
    m.method = Some(DetectionMethod::LegacySectionString);
    m.section_name = section.name.clone();
    m.entry_point_rva = image.entry_point_rva();
    m.match_raw_offset = match_raw as usize;
    m.match_rva = image.raw_offset_to_rva(m.match_raw_offset).unwrap_or(0);
    Some(m)
}

fn try_oep_pattern(module: &ModuleCandidate, image: &Image) -> Option<DetectionMatch> {
    let oep = image.entry_point_rva();
    if oep == 0 { return None; }
    let section = image.section_containing_rva(oep)?;
    if section.raw_size == 0 { return None; }
    let match_raw = byte_search::find_in_file_range(
        &module.native_path,
        section.raw_offset as u64,
        section.raw_size as u64,
        &K_DENUVO_OEP_PATTERN,
        K_OEP_SCAN_CHUNK_BYTES,
    )?;
    let local_match = (match_raw - section.raw_offset as u64) as u32;
    let mut m = DetectionMatch::default();
    m.method = Some(DetectionMethod::OepPattern);
    m.section_name = section.name.clone();
    m.entry_point_rva = oep;
    m.match_raw_offset = match_raw as usize;
    m.match_rva = section.virtual_address.wrapping_add(local_match);
    Some(m)
}

fn try_protected_blob_section(
    module: &ModuleCandidate,
    image: &Image,
) -> Option<DetectionMatch> {
    for section in image.sections() {
        if !(section.is_executable() && section.is_writable()) { continue; }
        if section.raw_size < K_PROTECTOR_BLOB_MIN_BYTES { continue; }

        let sample_size =
            (section.raw_size as usize).min(K_PROTECTOR_BLOB_ENTROPY_SAMPLE_BYTES);
        let sample = image.read_raw_bytes(section.raw_offset as usize, sample_size);
        if sample.is_empty() { continue; }

        let entropy = section_entropy(sample.as_slice());
        if entropy < K_PROTECTOR_BLOB_MIN_ENTROPY { continue; }

        let mut m = DetectionMatch::default();
        m.method = Some(DetectionMethod::ProtectedBlobSection);
        m.section_name = section.name.clone();
        m.entry_point_rva = image.entry_point_rva();
        m.match_raw_offset = section.raw_offset as usize;
        m.match_rva = section.virtual_address;

        let confidence = if entropy >= K_PROTECTOR_BLOB_HIGH_CONFIDENCE_ENTROPY {
            "high(encrypted)"
        } else { "elevated" };
        LOG_PIPE_INFO!(
            "DenuvoAuth: protector blob section path={} section={} raw_size={} ({:.2} MB) entropy={:.3} flags=RWX confidence={}",
            module.path, section.name, section.raw_size,
            bytes_to_mib(section.raw_size as u64), entropy, confidence,
        );
        return Some(m);
    }
    None
}

fn detect_module(module: &ModuleCandidate, image: &Image) -> Option<DetectionMatch> {
    if let Some(m) = try_oep_pattern(module, image) { return Some(m); }
    if let Some(section) = find_legacy_denuvo_section(image) {
        if let Some(m) = try_legacy_section_string(module, image, section) { return Some(m); }
    }
    try_protected_blob_section(module, image)
}

// ── Module enumeration ──────────────────────────────────────────────

fn enumerate_modules(pid: u32) -> Vec<ModuleCandidate> {
    let timer = Stopwatch::new();
    let mut modules: Vec<ModuleCandidate> = Vec::new();
    let mut order = 0usize;

    for m in process::enumerate_modules(pid) {
        if m.path.is_empty() { continue; }
        let executable = ends_with_insensitive(&m.path, ".exe");
        let dll = ends_with_insensitive(&m.path, ".dll");
        if !executable && !dll { continue; }
        if m.size < K_MIN_PACKED_MODULE_BYTES { continue; }
        if !executable && process::is_system_module_path(&m.path) { continue; }
        if !executable && is_steam_runtime_module_name(&base_name_from_path(&m.path)) {
            continue;
        }
        modules.push(ModuleCandidate {
            path: m.path,
            native_path: m.native_path,
            size: m.size,
            executable,
            in_game_tree: false,
            order,
        });
        order += 1;
    }

    // The first executable module in the list defines the game tree.
    let mut game_directory = String::new();
    for m in &modules {
        if m.executable {
            game_directory = normalize_directory_prefix(&directory_from_path(&m.path));
            break;
        }
    }
    for m in modules.iter_mut() {
        m.in_game_tree = !m.executable
            && path_starts_with_insensitive(&m.path, &game_directory);
    }

    // Sort: .exe first, then in-game-tree DLLs, then by size descending,
    // ties broken by discovery order.
    modules.sort_by(|a, b| {
        if a.executable != b.executable {
            return b.executable.cmp(&a.executable);
        }
        if !a.executable && a.in_game_tree != b.in_game_tree {
            return b.in_game_tree.cmp(&a.in_game_tree);
        }
        if !a.executable && a.size != b.size {
            return b.size.cmp(&a.size);
        }
        a.order.cmp(&b.order)
    });

    LOG_PIPE_DEBUG!(
        "DenuvoAuth: pid={} enumerated {} candidate module(s) elapsed_ms={:.3}",
        pid, modules.len(), timer.elapsed_ms(),
    );
    modules
}

// ── Entry point ─────────────────────────────────────────────────────

pub fn scan_protection(pid: u32) -> ProtectionScanReport {
    let mut report = ProtectionScanReport::default();
    if pid == 0 { return report; }

    let total_timer = Stopwatch::new();
    let modules = enumerate_modules(pid);
    LOG_PIPE_DEBUG!(
        "DenuvoAuth: pid={} scanning {} candidate module(s)", pid, modules.len(),
    );

    for module in &modules {
        report.scanned_modules += 1;
        LOG_PIPE_DEBUG!(
            "DenuvoAuth: module candidate {}: {}",
            report.scanned_modules, module.debug_string(),
        );

        let image = Image::new(&module.native_path);
        if !image.is_valid() {
            continue;
        }

        let Some(m) = detect_module(module, &image) else { continue; };

        report.pid = pid;
        report.denuvo_detected = true;
        report.method = m.method;
        report.module_path = module.path.clone();
        report.section_name = m.section_name;
        report.module_size = module.size;
        report.entry_point_rva = m.entry_point_rva;
        report.match_rva = m.match_rva;
        report.match_raw_offset = m.match_raw_offset;
        report.elapsed_ms = total_timer.elapsed_ms();

        LOG_PIPE_INFO!(
            "DenuvoAuth: success to detect Denuvo in module {}", report.debug_string(),
        );
        return report;
    }

    report.elapsed_ms = total_timer.elapsed_ms();
    LOG_PIPE_DEBUG!(
        "DenuvoAuth: pid={} no Denuvo match scanned={} elapsed_ms={:.3}",
        pid, report.scanned_modules, report.elapsed_ms,
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_of_empty_is_zero() {
        assert_eq!(section_entropy(&[]), 0.0);
    }

    #[test]
    fn entropy_of_uniform_is_zero() {
        let bytes = vec![0xAA; 1024];
        assert_eq!(section_entropy(&bytes), 0.0);
    }

    #[test]
    fn entropy_of_all_distinct_is_near_eight() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let e = section_entropy(&bytes);
        assert!((e - 8.0).abs() < 0.001, "entropy was {}", e);
    }

    #[test]
    fn ends_with_insensitive_case_folding() {
        assert!(ends_with_insensitive("Kernel32.DLL", ".dll"));
        assert!(ends_with_insensitive("Kernel32.dll", ".DLL"));
        assert!(!ends_with_insensitive("Kernel32.dll", ".exe"));
        assert!(!ends_with_insensitive("x.dll", ".verylong"));
    }

    #[test]
    fn normalize_directory_prefix_appends_backslash() {
        assert_eq!(normalize_directory_prefix("C:\\Steam"), "c:\\steam\\");
        assert_eq!(normalize_directory_prefix("C:/Steam/"), "c:\\steam\\");
        assert_eq!(normalize_directory_prefix(""), "");
    }

    #[test]
    fn path_starts_with_insensitive_case_and_sep() {
        assert!(path_starts_with_insensitive("C:\\Steam\\x.dll", "c:\\steam\\"));
        assert!(path_starts_with_insensitive("C:/Steam/x.dll", "c:\\steam\\"));
        assert!(!path_starts_with_insensitive("C:\\Other\\x.dll", "c:\\steam\\"));
        assert!(!path_starts_with_insensitive("x", ""));
    }

    #[test]
    fn base_name_variants() {
        assert_eq!(base_name_from_path("C:\\a\\b.dll"), "b.dll");
        assert_eq!(base_name_from_path("b.dll"), "b.dll");
    }

    #[test]
    fn to_string_covers_all_variants() {
        assert_eq!(to_string(DetectionMethod::None), "None");
        assert_eq!(to_string(DetectionMethod::OepPattern), "OepPattern");
        assert_eq!(to_string(DetectionMethod::ProtectedBlobSection), "ProtectedBlobSection");
        assert_eq!(to_string(DetectionMethod::LegacySectionString), "LegacySectionString");
    }

    #[test]
    fn scan_zero_pid_returns_empty() {
        let r = scan_protection(0);
        assert!(!r.denuvo_detected);
        assert_eq!(r.scanned_modules, 0);
    }
}
