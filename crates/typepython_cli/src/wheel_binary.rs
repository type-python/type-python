use std::collections::{BTreeMap, BTreeSet};

use goblin::{
    Object,
    elf::header::{
        EM_386, EM_AARCH64, EM_ARM, EM_LOONGARCH, EM_PPC, EM_PPC64, EM_RISCV, EM_S390, EM_X86_64,
    },
    mach::{
        Mach, SingleArch,
        constants::cputype::{
            CPU_TYPE_ARM, CPU_TYPE_ARM64, CPU_TYPE_POWERPC, CPU_TYPE_POWERPC64, CPU_TYPE_X86,
            CPU_TYPE_X86_64,
        },
        load_command::{CommandVariant, PLATFORM_MACOS},
    },
    pe::header::{
        COFF_MACHINE_ARM, COFF_MACHINE_ARM64, COFF_MACHINE_ARMNT, COFF_MACHINE_X86,
        COFF_MACHINE_X86_64,
    },
};

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum NativeArchitecture {
    Aarch64,
    Arm,
    LoongArch64,
    PowerPc,
    PowerPc64,
    PowerPc64Le,
    RiscV64,
    S390x,
    X86,
    X86_64,
    Unknown(u32),
}

impl NativeArchitecture {
    fn label(self) -> &'static str {
        match self {
            Self::Aarch64 => "arm64/aarch64",
            Self::Arm => "arm",
            Self::LoongArch64 => "loongarch64",
            Self::PowerPc => "ppc",
            Self::PowerPc64 => "ppc64",
            Self::PowerPc64Le => "ppc64le",
            Self::RiscV64 => "riscv64",
            Self::S390x => "s390x",
            Self::X86 => "x86/i386",
            Self::X86_64 => "x86_64/amd64",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum NativeFormat {
    Elf,
    MachO,
    Pe,
}

impl NativeFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Elf => "ELF",
            Self::MachO => "Mach-O",
            Self::Pe => "PE",
        }
    }
}

#[derive(Debug)]
struct NativeBinary {
    format: NativeFormat,
    architectures: BTreeSet<NativeArchitecture>,
    macos_minimums: BTreeMap<NativeArchitecture, (u16, u8, u8)>,
    non_macos_architectures: BTreeSet<NativeArchitecture>,
}

#[derive(Debug)]
enum PlatformClaim {
    Any,
    Elf(NativeArchitecture),
    MacOS { minimum: (u16, u8), architectures: BTreeSet<NativeArchitecture> },
    Windows(NativeArchitecture),
}

const MAX_NATIVE_BINARIES: usize = 256;
const MAX_NATIVE_ERRORS: usize = 100;
const MAX_PLATFORM_CLAIMS: usize = 256;
const MAX_PLATFORM_TAG_LENGTH: usize = 256;
const MAX_MACHO_SLICES: usize = 32;

pub(crate) fn wheel_native_binary_errors(
    entries: &BTreeMap<String, Vec<u8>>,
    tags: &BTreeSet<String>,
) -> Vec<String> {
    let platforms = tags
        .iter()
        .map(|tag| tag.rsplit_once('-').map_or(tag.as_str(), |(_, platform)| platform))
        .collect::<BTreeSet<_>>();
    if let Some(platform) =
        platforms.iter().find(|platform| platform.len() > MAX_PLATFORM_TAG_LENGTH)
    {
        return vec![format!(
            "wheel platform claim is {} bytes; the verification limit is {MAX_PLATFORM_TAG_LENGTH}",
            platform.len()
        )];
    }
    if platforms.len() > MAX_PLATFORM_CLAIMS {
        return vec![format!(
            "wheel declares {} unique native platform claims; the verification limit is {MAX_PLATFORM_CLAIMS}",
            platforms.len()
        )];
    }
    let claims =
        platforms.iter().filter_map(|platform| platform_claim(platform)).collect::<Vec<_>>();
    let unsupported_platforms = platforms
        .iter()
        .filter(|platform| platform_claim(platform).is_none())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut errors = BTreeSet::new();
    let mut native_binary_count = 0_usize;
    let mut truncated_errors = false;

    for (path, bytes) in entries {
        let binary = match inspect_native_binary(bytes) {
            Ok(Some(binary)) => binary,
            Ok(None) => continue,
            Err(error) => {
                if is_native_candidate_path(path) {
                    errors.insert(format!("native payload `{path}` is malformed: {error}"));
                    if errors.len() >= MAX_NATIVE_ERRORS {
                        truncated_errors = true;
                        break;
                    }
                }
                continue;
            }
        };
        native_binary_count += 1;
        if native_binary_count > MAX_NATIVE_BINARIES {
            return vec![format!(
                "wheel contains more than {MAX_NATIVE_BINARIES} native payloads; refusing unbounded platform validation"
            )];
        }

        for platform in &unsupported_platforms {
            errors.insert(format!(
                "native payload `{path}` cannot be checked against unsupported declared platform tag `{platform}`"
            ));
            if errors.len() >= MAX_NATIVE_ERRORS {
                truncated_errors = true;
                break;
            }
        }
        if truncated_errors {
            break;
        }
        for claim in &claims {
            if let Some(error) = binary_claim_error(&binary, claim) {
                errors.insert(format!("native payload `{path}` {error}"));
            }
            if errors.len() >= MAX_NATIVE_ERRORS {
                truncated_errors = true;
                break;
            }
        }
        if truncated_errors {
            break;
        }
    }

    let mut errors = errors.into_iter().collect::<Vec<_>>();
    if truncated_errors {
        errors.push(format!(
            "native payload validation stopped after {MAX_NATIVE_ERRORS} unique errors"
        ));
    }
    errors
}

fn binary_claim_error(binary: &NativeBinary, claim: &PlatformClaim) -> Option<String> {
    let (expected_format, expected_architectures, macos_minimum) = match claim {
        PlatformClaim::Any => {
            return Some(format!(
                "is a {} binary, but the wheel declares platform tag `any`",
                binary.format.label()
            ));
        }
        PlatformClaim::Elf(architecture) => {
            (NativeFormat::Elf, BTreeSet::from([*architecture]), None)
        }
        PlatformClaim::MacOS { minimum, architectures } => {
            (NativeFormat::MachO, architectures.clone(), Some(*minimum))
        }
        PlatformClaim::Windows(architecture) => {
            (NativeFormat::Pe, BTreeSet::from([*architecture]), None)
        }
    };

    if binary.format != expected_format {
        return Some(format!(
            "uses {} format, which is incompatible with the declared {} platform",
            binary.format.label(),
            expected_format.label()
        ));
    }

    let missing = expected_architectures
        .difference(&binary.architectures)
        .map(|architecture| architecture.label())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        let actual = binary
            .architectures
            .iter()
            .map(|architecture| architecture.label())
            .collect::<Vec<_>>()
            .join(", ");
        return Some(format!(
            "is missing declared architecture(s) {}; found {}",
            missing.join(", "),
            if actual.is_empty() { "none" } else { &actual }
        ));
    }

    if let Some(claimed_minimum) = macos_minimum {
        let non_macos = expected_architectures
            .intersection(&binary.non_macos_architectures)
            .map(|architecture| architecture.label())
            .collect::<Vec<_>>();
        if !non_macos.is_empty() {
            return Some(format!(
                "targets a non-macOS Apple platform for declared architecture(s) {}",
                non_macos.join(", ")
            ));
        }

        let missing_minimums = expected_architectures
            .iter()
            .filter(|architecture| !binary.macos_minimums.contains_key(architecture))
            .map(|architecture| architecture.label())
            .collect::<Vec<_>>();
        if !missing_minimums.is_empty() {
            return Some(format!(
                "does not declare a macOS deployment target for architecture(s) {}",
                missing_minimums.join(", ")
            ));
        }

        if let Some(actual_minimum) = effective_macos_minimum(binary, &expected_architectures) {
            let actual_minimum = normalized_macos_floor(actual_minimum);
            if actual_minimum > claimed_minimum {
                return Some(format!(
                    "requires macOS {}.{} but the wheel declares macOS {}.{}",
                    actual_minimum.0, actual_minimum.1, claimed_minimum.0, claimed_minimum.1
                ));
            }
        }
    }

    None
}

fn effective_macos_minimum(
    binary: &NativeBinary,
    expected_architectures: &BTreeSet<NativeArchitecture>,
) -> Option<(u16, u8, u8)> {
    let mut versions = expected_architectures
        .iter()
        .filter_map(|architecture| {
            binary.macos_minimums.get(architecture).map(|version| (*architecture, *version))
        })
        .collect::<Vec<_>>();
    let original = versions.clone();
    if expected_architectures.len() > 1 {
        versions.retain(|(architecture, version)| {
            !(*architecture == NativeArchitecture::Aarch64 && *version == (11, 0, 0))
        });
    }
    versions
        .into_iter()
        .map(|(_, version)| version)
        .max()
        .or_else(|| original.into_iter().map(|(_, version)| version).max())
}

fn normalized_macos_floor(version: (u16, u8, u8)) -> (u16, u8) {
    if version.0 > 10 { (version.0, 0) } else { (version.0, version.1) }
}

fn inspect_native_binary(bytes: &[u8]) -> Result<Option<NativeBinary>, String> {
    if !has_native_magic(bytes) {
        return Ok(None);
    }
    if let Some(slice_count) = fat_macho_slice_count(bytes)
        && slice_count > MAX_MACHO_SLICES
    {
        return Err(format!(
            "fat Mach-O declares {slice_count} slices; the verification limit is {MAX_MACHO_SLICES}"
        ));
    }
    let object = Object::parse(bytes).map_err(|error| error.to_string())?;
    match object {
        Object::Elf(elf) => {
            let architecture =
                elf_architecture(elf.header.e_machine, elf.is_64, elf.little_endian)?;
            Ok(Some(NativeBinary {
                format: NativeFormat::Elf,
                architectures: BTreeSet::from([architecture]),
                macos_minimums: BTreeMap::new(),
                non_macos_architectures: BTreeSet::new(),
            }))
        }
        Object::Mach(Mach::Binary(binary)) => {
            let mut result = NativeBinary {
                format: NativeFormat::MachO,
                architectures: BTreeSet::new(),
                macos_minimums: BTreeMap::new(),
                non_macos_architectures: BTreeSet::new(),
            };
            record_macho(&mut result, &binary);
            Ok(Some(result))
        }
        Object::Mach(Mach::Fat(fat)) => {
            let mut result = NativeBinary {
                format: NativeFormat::MachO,
                architectures: BTreeSet::new(),
                macos_minimums: BTreeMap::new(),
                non_macos_architectures: BTreeSet::new(),
            };
            for entry in &fat {
                match entry.map_err(|error| error.to_string())? {
                    SingleArch::MachO(binary) => record_macho(&mut result, &binary),
                    SingleArch::Archive(_) => {
                        return Err(String::from(
                            "fat Mach-O payload contains a static archive instead of a binary",
                        ));
                    }
                }
            }
            Ok(Some(result))
        }
        Object::PE(pe) => {
            if pe.header.optional_header.is_none() {
                return Err(String::from("PE image is missing its required optional header"));
            }
            let architecture = pe_architecture(pe.header.coff_header.machine, pe.is_64)?;
            Ok(Some(NativeBinary {
                format: NativeFormat::Pe,
                architectures: BTreeSet::from([architecture]),
                macos_minimums: BTreeMap::new(),
                non_macos_architectures: BTreeSet::new(),
            }))
        }
        Object::Archive(_) | Object::COFF(_) | Object::TE(_) | Object::Unknown(_) => {
            Err(String::from("recognized native magic has an unsupported object format"))
        }
        _ => Err(String::from("recognized native magic has an unsupported object format")),
    }
}

fn record_macho(result: &mut NativeBinary, binary: &goblin::mach::MachO<'_>) {
    let architecture = match (binary.header.cputype, binary.header.cpusubtype()) {
        (CPU_TYPE_ARM64, 2) | (CPU_TYPE_X86_64, 8) => {
            NativeArchitecture::Unknown(binary.header.cputype)
        }
        _ => macho_architecture(binary.header.cputype),
    };
    result.architectures.insert(architecture);
    let mut targets_non_macos = false;
    if let Some(minimum) = binary
        .load_commands
        .iter()
        .filter_map(|command| match command.command {
            CommandVariant::BuildVersion(command) if command.platform == PLATFORM_MACOS => {
                Some(decode_apple_version(command.minos))
            }
            CommandVariant::BuildVersion(_) => {
                targets_non_macos = true;
                None
            }
            CommandVariant::VersionMinMacosx(command) => {
                Some(decode_apple_version(command.version))
            }
            CommandVariant::VersionMinIphoneos(_)
            | CommandVariant::VersionMinTvos(_)
            | CommandVariant::VersionMinWatchos(_) => {
                targets_non_macos = true;
                None
            }
            _ => None,
        })
        .max()
    {
        result
            .macos_minimums
            .entry(architecture)
            .and_modify(|current| *current = (*current).max(minimum))
            .or_insert(minimum);
    }
    if targets_non_macos {
        result.non_macos_architectures.insert(architecture);
    }
}

fn decode_apple_version(version: u32) -> (u16, u8, u8) {
    ((version >> 16) as u16, ((version >> 8) & 0xff) as u8, (version & 0xff) as u8)
}

fn has_native_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(b"MZ")
        || bytes.get(..4).is_some_and(|magic| {
            matches!(
                magic,
                [0xfe, 0xed, 0xfa, 0xce]
                    | [0xce, 0xfa, 0xed, 0xfe]
                    | [0xfe, 0xed, 0xfa, 0xcf]
                    | [0xcf, 0xfa, 0xed, 0xfe]
                    | [0xca, 0xfe, 0xba, 0xbe]
                    | [0xbe, 0xba, 0xfe, 0xca]
                    | [0xca, 0xfe, 0xba, 0xbf]
                    | [0xbf, 0xba, 0xfe, 0xca]
            )
        })
}

fn fat_macho_slice_count(bytes: &[u8]) -> Option<usize> {
    let magic: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let count: [u8; 4] = bytes.get(4..8)?.try_into().ok()?;
    let count = match magic {
        [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf] => u32::from_be_bytes(count),
        [0xbe, 0xba, 0xfe, 0xca] | [0xbf, 0xba, 0xfe, 0xca] => u32::from_le_bytes(count),
        _ => return None,
    };
    usize::try_from(count).ok()
}

fn is_native_candidate_path(path: &str) -> bool {
    let lowercase = path.to_ascii_lowercase();
    let leaf = lowercase.rsplit('/').next().unwrap_or(&lowercase);
    lowercase.split('/').any(|part| part == "bin")
        || lowercase.contains(".data/scripts/")
        || [".dll", ".dylib", ".exe", ".node", ".pyd", ".so"]
            .iter()
            .any(|suffix| leaf.ends_with(suffix))
        || leaf.contains(".so.")
}

fn elf_architecture(
    machine: u16,
    is_64: bool,
    little_endian: bool,
) -> Result<NativeArchitecture, String> {
    match machine {
        EM_386 if !is_64 && little_endian => Ok(NativeArchitecture::X86),
        EM_X86_64 if is_64 && little_endian => Ok(NativeArchitecture::X86_64),
        EM_ARM if !is_64 && little_endian => Ok(NativeArchitecture::Arm),
        EM_AARCH64 if is_64 && little_endian => Ok(NativeArchitecture::Aarch64),
        EM_PPC if !is_64 && !little_endian => Ok(NativeArchitecture::PowerPc),
        EM_PPC64 if is_64 && little_endian => Ok(NativeArchitecture::PowerPc64Le),
        EM_PPC64 if is_64 => Ok(NativeArchitecture::PowerPc64),
        EM_S390 if is_64 && !little_endian => Ok(NativeArchitecture::S390x),
        EM_RISCV if is_64 && little_endian => Ok(NativeArchitecture::RiscV64),
        EM_LOONGARCH if is_64 && little_endian => Ok(NativeArchitecture::LoongArch64),
        EM_386 | EM_X86_64 | EM_ARM | EM_AARCH64 | EM_PPC | EM_PPC64 | EM_S390 | EM_RISCV
        | EM_LOONGARCH => {
            Err(format!("ELF machine {machine} has an incompatible class or byte order"))
        }
        other => Ok(NativeArchitecture::Unknown(u32::from(other))),
    }
}

fn macho_architecture(cpu_type: u32) -> NativeArchitecture {
    match cpu_type {
        CPU_TYPE_X86 => NativeArchitecture::X86,
        CPU_TYPE_X86_64 => NativeArchitecture::X86_64,
        CPU_TYPE_ARM => NativeArchitecture::Arm,
        CPU_TYPE_ARM64 => NativeArchitecture::Aarch64,
        CPU_TYPE_POWERPC => NativeArchitecture::PowerPc,
        CPU_TYPE_POWERPC64 => NativeArchitecture::PowerPc64,
        other => NativeArchitecture::Unknown(other),
    }
}

fn pe_architecture(machine: u16, is_64: bool) -> Result<NativeArchitecture, String> {
    match machine {
        COFF_MACHINE_X86 if !is_64 => Ok(NativeArchitecture::X86),
        COFF_MACHINE_X86_64 if is_64 => Ok(NativeArchitecture::X86_64),
        COFF_MACHINE_ARM | COFF_MACHINE_ARMNT if !is_64 => Ok(NativeArchitecture::Arm),
        COFF_MACHINE_ARM64 if is_64 => Ok(NativeArchitecture::Aarch64),
        COFF_MACHINE_X86 | COFF_MACHINE_X86_64 | COFF_MACHINE_ARM | COFF_MACHINE_ARMNT
        | COFF_MACHINE_ARM64 => {
            Err(format!("PE machine {machine:#06x} has an incompatible optional-header width"))
        }
        other => Ok(NativeArchitecture::Unknown(u32::from(other))),
    }
}

fn platform_claim(platform: &str) -> Option<PlatformClaim> {
    if platform == "any" {
        return Some(PlatformClaim::Any);
    }
    if let Some(remainder) = platform.strip_prefix("macosx_") {
        return macos_platform_claim(remainder);
    }
    match platform {
        "win32" => return Some(PlatformClaim::Windows(NativeArchitecture::X86)),
        "win_amd64" => return Some(PlatformClaim::Windows(NativeArchitecture::X86_64)),
        "win_arm64" => return Some(PlatformClaim::Windows(NativeArchitecture::Aarch64)),
        _ => {}
    }
    if platform.starts_with("linux_")
        || platform.starts_with("manylinux")
        || platform.starts_with("musllinux_")
        || platform.starts_with("android_")
    {
        return platform_architecture(platform).map(PlatformClaim::Elf);
    }
    None
}

fn macos_platform_claim(remainder: &str) -> Option<PlatformClaim> {
    let mut parts = remainder.split('_');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let architecture = parts.collect::<Vec<_>>().join("_");
    let architectures = match architecture.as_str() {
        "arm64" => BTreeSet::from([NativeArchitecture::Aarch64]),
        "x86_64" => BTreeSet::from([NativeArchitecture::X86_64]),
        "i386" => BTreeSet::from([NativeArchitecture::X86]),
        "ppc" => BTreeSet::from([NativeArchitecture::PowerPc]),
        "ppc64" => BTreeSet::from([NativeArchitecture::PowerPc64]),
        "universal2" => BTreeSet::from([NativeArchitecture::Aarch64, NativeArchitecture::X86_64]),
        "intel" => BTreeSet::from([NativeArchitecture::X86, NativeArchitecture::X86_64]),
        "fat32" | "fat" => BTreeSet::from([NativeArchitecture::PowerPc, NativeArchitecture::X86]),
        "fat64" => BTreeSet::from([NativeArchitecture::PowerPc64, NativeArchitecture::X86_64]),
        "fat3" => BTreeSet::from([
            NativeArchitecture::PowerPc,
            NativeArchitecture::X86,
            NativeArchitecture::X86_64,
        ]),
        "universal" => BTreeSet::from([
            NativeArchitecture::PowerPc,
            NativeArchitecture::PowerPc64,
            NativeArchitecture::X86,
            NativeArchitecture::X86_64,
        ]),
        _ => return None,
    };
    Some(PlatformClaim::MacOS { minimum: (major, minor), architectures })
}

fn platform_architecture(platform: &str) -> Option<NativeArchitecture> {
    [
        ("_loongarch64", NativeArchitecture::LoongArch64),
        ("_arm64_v8a", NativeArchitecture::Aarch64),
        ("_armeabi_v7a", NativeArchitecture::Arm),
        ("_ppc64le", NativeArchitecture::PowerPc64Le),
        ("_aarch64", NativeArchitecture::Aarch64),
        ("_x86_64", NativeArchitecture::X86_64),
        ("_riscv64", NativeArchitecture::RiscV64),
        ("_ppc64", NativeArchitecture::PowerPc64),
        ("_s390x", NativeArchitecture::S390x),
        ("_armv7l", NativeArchitecture::Arm),
        ("_i686", NativeArchitecture::X86),
        ("_i386", NativeArchitecture::X86),
        ("_x86", NativeArchitecture::X86),
    ]
    .into_iter()
    .find_map(|(suffix, architecture)| platform.ends_with(suffix).then_some(architecture))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thin_arm64_macho_does_not_satisfy_universal2() {
        let entries = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho(CPU_TYPE_ARM64, (11, 0, 0)),
        )]);
        let errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-macosx_10_9_universal2")]),
        );

        assert!(errors.iter().any(|error| error.contains("x86_64/amd64")), "{errors:?}");
    }

    #[test]
    fn macho_deployment_target_cannot_exceed_wheel_tag() {
        let entries = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho(CPU_TYPE_ARM64, (13, 0, 0)),
        )]);
        let errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-macosx_11_0_arm64")]),
        );

        assert!(errors.iter().any(|error| error.contains("requires macOS 13.0")), "{errors:?}");
    }

    #[test]
    fn modern_macos_minor_versions_normalize_to_the_major_floor() {
        let entries = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho(CPU_TYPE_ARM64, (11, 2, 0)),
        )]);
        let tags = BTreeSet::from([String::from("py3-none-macosx_11_0_arm64")]);

        assert!(wheel_native_binary_errors(&entries, &tags).is_empty());
    }

    #[test]
    fn macos_claim_requires_a_macos_deployment_command() {
        let tags = BTreeSet::from([String::from("py3-none-macosx_11_0_arm64")]);
        let missing = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho_without_commands(CPU_TYPE_ARM64),
        )]);
        let errors = wheel_native_binary_errors(&missing, &tags);
        assert!(errors.iter().any(|error| error.contains("does not declare")), "{errors:?}");

        let ios = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho_for_platform(CPU_TYPE_ARM64, (17, 0, 0), 2),
        )]);
        let errors = wheel_native_binary_errors(&ios, &tags);
        assert!(errors.iter().any(|error| error.contains("non-macOS")), "{errors:?}");
    }

    #[test]
    fn macos_uses_the_highest_version_command_and_accepts_legacy_commands() {
        let tags = BTreeSet::from([String::from("py3-none-macosx_10_9_x86_64")]);
        let conflicting = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho_with_build_versions(
                CPU_TYPE_X86_64,
                &[(PLATFORM_MACOS, (10, 9, 0)), (PLATFORM_MACOS, (13, 0, 0))],
            ),
        )]);
        let errors = wheel_native_binary_errors(&conflicting, &tags);
        assert!(errors.iter().any(|error| error.contains("requires macOS 13.0")), "{errors:?}");

        let legacy = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho_with_legacy_minimum(CPU_TYPE_X86_64, (10, 9, 0)),
        )]);
        assert!(wheel_native_binary_errors(&legacy, &tags).is_empty());
    }

    #[test]
    fn universal2_uses_both_slices_and_ignores_the_inherent_arm64_floor() {
        let tags = BTreeSet::from([String::from("py3-none-macosx_10_9_universal2")]);
        let entries = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            fat_macho([(CPU_TYPE_X86_64, (10, 9, 0)), (CPU_TYPE_ARM64, (11, 0, 0))]),
        )]);

        assert!(wheel_native_binary_errors(&entries, &tags).is_empty());

        let too_new = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            fat_macho([(CPU_TYPE_X86_64, (10, 9, 0)), (CPU_TYPE_ARM64, (13, 0, 0))]),
        )]);
        let errors = wheel_native_binary_errors(&too_new, &tags);
        assert!(errors.iter().any(|error| error.contains("requires macOS 13.0")), "{errors:?}");
    }

    #[test]
    fn native_binary_cannot_use_any_platform_tag() {
        let entries = BTreeMap::from([(
            String::from("typepython/bin/typepython"),
            thin_macho(CPU_TYPE_ARM64, (11, 0, 0)),
        )]);
        let errors =
            wheel_native_binary_errors(&entries, &BTreeSet::from([String::from("py3-none-any")]));

        assert!(errors.iter().any(|error| error.contains("platform tag `any`")), "{errors:?}");
    }

    #[test]
    fn matching_elf_and_pe_architectures_are_accepted() {
        let elf_entries =
            BTreeMap::from([(String::from("package/bin/tool"), minimal_elf64(EM_X86_64))]);
        assert!(
            wheel_native_binary_errors(
                &elf_entries,
                &BTreeSet::from([String::from("py3-none-manylinux_2_17_x86_64")]),
            )
            .is_empty()
        );

        let pe_entries = BTreeMap::from([(
            String::from("package/bin/tool.exe"),
            minimal_pe(COFF_MACHINE_ARM64),
        )]);
        assert!(
            wheel_native_binary_errors(
                &pe_entries,
                &BTreeSet::from([String::from("py3-none-win_arm64")]),
            )
            .is_empty()
        );

        let android_entries =
            BTreeMap::from([(String::from("package/bin/tool"), minimal_elf64(EM_AARCH64))]);
        assert!(
            wheel_native_binary_errors(
                &android_entries,
                &BTreeSet::from([String::from("py3-none-android_24_arm64_v8a")]),
            )
            .is_empty()
        );
    }

    #[test]
    fn mismatched_native_format_and_architecture_are_rejected() {
        let entries =
            BTreeMap::from([(String::from("package/bin/tool"), minimal_elf64(EM_AARCH64))]);
        let format_errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-win_arm64")]),
        );
        assert!(format_errors.iter().any(|error| error.contains("ELF format")));

        let arch_errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-manylinux_2_17_x86_64")]),
        );
        assert!(arch_errors.iter().any(|error| error.contains("x86_64/amd64")));
    }

    #[test]
    fn unsupported_platforms_fail_closed_for_native_payloads() {
        let entries =
            BTreeMap::from([(String::from("package/bin/tool"), minimal_elf64(EM_X86_64))]);
        let errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-freebsd_14_x86_64")]),
        );

        assert!(errors.iter().any(|error| error.contains("unsupported")), "{errors:?}");
    }

    #[test]
    fn malformed_magic_only_blocks_native_candidate_paths() {
        let bytes = b"MZnot really a PE file".to_vec();
        let tags = BTreeSet::from([String::from("py3-none-win_amd64")]);
        let data = BTreeMap::from([(String::from("package/data/fixture.bin"), bytes.clone())]);
        assert!(wheel_native_binary_errors(&data, &tags).is_empty());

        let executable = BTreeMap::from([(String::from("package/bin/tool.exe"), bytes)]);
        let errors = wheel_native_binary_errors(&executable, &tags);
        assert!(errors.iter().any(|error| error.contains("malformed")), "{errors:?}");
    }

    #[test]
    fn native_headers_must_match_their_declared_width_and_byte_order() {
        let mut elf32_x86_64 = minimal_elf64(EM_X86_64);
        elf32_x86_64[4] = 1;
        assert!(inspect_native_binary(&elf32_x86_64).is_err());

        let mut pe32_arm64 = minimal_pe(COFF_MACHINE_X86);
        pe32_arm64[0x44..0x46].copy_from_slice(&COFF_MACHINE_ARM64.to_le_bytes());
        let error = inspect_native_binary(&pe32_arm64).expect_err("PE32 ARM64 must be rejected");
        assert!(error.contains("optional-header width"), "{error}");

        let mut arm64e = thin_macho(CPU_TYPE_ARM64, (13, 0, 0));
        arm64e[8..12].copy_from_slice(&2_u32.to_le_bytes());
        let entries = BTreeMap::from([(String::from("package/bin/tool"), arm64e)]);
        let errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-macosx_13_0_arm64")]),
        );
        assert!(errors.iter().any(|error| error.contains("missing declared")), "{errors:?}");
    }

    #[test]
    fn native_validation_has_multiplication_and_fat_slice_limits() {
        let tags = (0..=MAX_PLATFORM_CLAIMS)
            .map(|index| format!("py3-none-unsupported_{index}"))
            .collect::<BTreeSet<_>>();
        let errors = wheel_native_binary_errors(&BTreeMap::new(), &tags);
        assert!(errors.iter().any(|error| error.contains("platform claims")), "{errors:?}");

        let entries = (0..=MAX_NATIVE_BINARIES)
            .map(|index| (format!("package/bin/tool-{index}"), minimal_elf64(EM_X86_64)))
            .collect::<BTreeMap<_, _>>();
        let errors = wheel_native_binary_errors(
            &entries,
            &BTreeSet::from([String::from("py3-none-linux_x86_64")]),
        );
        assert!(errors.iter().any(|error| error.contains("native payloads")), "{errors:?}");

        let mut oversized_fat = Vec::new();
        oversized_fat.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        oversized_fat.extend_from_slice(&((MAX_MACHO_SLICES + 1) as u32).to_be_bytes());
        let fat = BTreeMap::from([(String::from("package/bin/tool"), oversized_fat)]);
        let errors = wheel_native_binary_errors(
            &fat,
            &BTreeSet::from([String::from("py3-none-macosx_11_0_arm64")]),
        );
        assert!(errors.iter().any(|error| error.contains("fat Mach-O declares")), "{errors:?}");
    }

    fn thin_macho(cpu_type: u32, minimum: (u16, u8, u8)) -> Vec<u8> {
        thin_macho_for_platform(cpu_type, minimum, PLATFORM_MACOS)
    }

    fn thin_macho_for_platform(cpu_type: u32, minimum: (u16, u8, u8), platform: u32) -> Vec<u8> {
        thin_macho_with_build_versions(cpu_type, &[(platform, minimum)])
    }

    fn thin_macho_with_build_versions(cpu_type: u32, versions: &[(u32, (u16, u8, u8))]) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32_le(&mut bytes, 0xfeedfacf);
        push_u32_le(&mut bytes, cpu_type);
        push_u32_le(&mut bytes, 0);
        push_u32_le(&mut bytes, 2);
        push_u32_le(&mut bytes, versions.len() as u32);
        push_u32_le(&mut bytes, (versions.len() * 24) as u32);
        push_u32_le(&mut bytes, 0);
        push_u32_le(&mut bytes, 0);
        for (platform, minimum) in versions {
            push_u32_le(&mut bytes, 0x32);
            push_u32_le(&mut bytes, 24);
            push_u32_le(&mut bytes, *platform);
            push_u32_le(&mut bytes, encoded_apple_version(*minimum));
            push_u32_le(&mut bytes, 0);
            push_u32_le(&mut bytes, 0);
        }
        bytes
    }

    fn thin_macho_without_commands(cpu_type: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [0xfeedfacf, cpu_type, 0, 2, 0, 0, 0, 0] {
            push_u32_le(&mut bytes, value);
        }
        bytes
    }

    fn thin_macho_with_legacy_minimum(cpu_type: u32, minimum: (u16, u8, u8)) -> Vec<u8> {
        let mut bytes = thin_macho_without_commands(cpu_type);
        bytes[16..20].copy_from_slice(&1_u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&16_u32.to_le_bytes());
        for value in [0x24, 16, encoded_apple_version(minimum), 0] {
            push_u32_le(&mut bytes, value);
        }
        bytes
    }

    fn encoded_apple_version(version: (u16, u8, u8)) -> u32 {
        (u32::from(version.0) << 16) | (u32::from(version.1) << 8) | u32::from(version.2)
    }

    fn fat_macho(slices: [(u32, (u16, u8, u8)); 2]) -> Vec<u8> {
        let slices = slices.map(|(cpu_type, minimum)| (cpu_type, thin_macho(cpu_type, minimum)));
        let header_size = 8 + 20 * slices.len();
        let mut offsets = Vec::new();
        let mut next_offset = header_size;
        for (_, slice) in &slices {
            offsets.push(next_offset);
            next_offset += slice.len();
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        bytes.extend_from_slice(&(slices.len() as u32).to_be_bytes());
        for ((cpu_type, slice), offset) in slices.iter().zip(&offsets) {
            bytes.extend_from_slice(&cpu_type.to_be_bytes());
            bytes.extend_from_slice(&0_u32.to_be_bytes());
            bytes.extend_from_slice(&(*offset as u32).to_be_bytes());
            bytes.extend_from_slice(&(slice.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&0_u32.to_be_bytes());
        }
        for (_, slice) in slices {
            bytes.extend_from_slice(&slice);
        }
        bytes
    }

    fn minimal_elf64(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2_u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        bytes
    }

    fn minimal_pe(machine: u16) -> Vec<u8> {
        let is_64 = matches!(machine, COFF_MACHINE_X86_64 | COFF_MACHINE_ARM64);
        let optional_header_size = if is_64 { 112_u16 } else { 96_u16 };
        let mut bytes = vec![0_u8; 88 + usize::from(optional_header_size)];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&machine.to_le_bytes());
        bytes[0x54..0x56].copy_from_slice(&optional_header_size.to_le_bytes());
        bytes[0x58..0x5a]
            .copy_from_slice(&(if is_64 { 0x20b_u16 } else { 0x10b_u16 }).to_le_bytes());
        bytes
    }

    fn push_u32_le(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
