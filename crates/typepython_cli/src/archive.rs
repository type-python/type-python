use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    str,
};

const DEFAULT_ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    max_archive_bytes: 256 * 1024 * 1024,
    max_members: 100_000,
    max_entry_bytes: 64 * 1024 * 1024,
    max_total_bytes: 512 * 1024 * 1024,
    max_tar_stream_bytes: 640 * 1024 * 1024,
    max_central_directory_bytes: 64 * 1024 * 1024,
    max_compression_ratio: 200,
    compression_ratio_min_bytes: 16 * 1024 * 1024,
    compression_ratio_slack: 1024 * 1024,
};

#[derive(Clone, Copy)]
struct ArchiveLimits {
    max_archive_bytes: u64,
    max_members: usize,
    max_entry_bytes: u64,
    max_total_bytes: u64,
    max_tar_stream_bytes: u64,
    max_central_directory_bytes: u64,
    max_compression_ratio: u64,
    compression_ratio_min_bytes: u64,
    compression_ratio_slack: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchivePathKind {
    Wheel,
    Sdist,
}

impl ArchivePathKind {
    fn label(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::Sdist => "sdist",
        }
    }
}

pub(crate) struct ArchiveMemberPaths {
    kind: ArchivePathKind,
    seen: BTreeSet<String>,
    files: BTreeSet<String>,
}

pub(crate) struct ArchiveReadBudget {
    kind: ArchivePathKind,
    limits: ArchiveLimits,
    archive_bytes: u64,
    members: usize,
    declared_bytes: u64,
    read_bytes: u64,
}

pub(crate) struct BoundedArchiveReader<R> {
    inner: R,
    remaining: u64,
    kind: ArchivePathKind,
    limit_name: &'static str,
}

impl<R> BoundedArchiveReader<R> {
    pub(crate) fn compressed(inner: R, kind: ArchivePathKind) -> Self {
        Self::with_limit(inner, kind, "compressed input", DEFAULT_ARCHIVE_LIMITS.max_archive_bytes)
    }

    pub(crate) fn tar_stream(inner: R, kind: ArchivePathKind) -> Self {
        Self::with_limit(
            inner,
            kind,
            "decoded tar stream",
            DEFAULT_ARCHIVE_LIMITS.max_tar_stream_bytes,
        )
    }

    fn with_limit(inner: R, kind: ArchivePathKind, limit_name: &'static str, limit: u64) -> Self {
        Self { inner, remaining: limit, kind, limit_name }
    }

    pub(crate) fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for BoundedArchiveReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0_u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{} archive exceeded its {} limit", self.kind.label(), self.limit_name),
                )),
            };
        }
        let allowed = usize::try_from(self.remaining.min(buffer.len() as u64))
            .map_err(|_| io::Error::other("archive stream read limit does not fit usize"))?;
        let read = self.inner.read(&mut buffer[..allowed])?;
        self.remaining = self.remaining.saturating_sub(read as u64);
        Ok(read)
    }
}

impl ArchiveReadBudget {
    pub(crate) fn open(path: &Path, kind: ArchivePathKind) -> Result<(fs::File, Self), String> {
        let path_metadata = fs::metadata(path)
            .map_err(|error| format!("unable to inspect archive input: {error}"))?;
        if !path_metadata.file_type().is_file() {
            return Err(format!("{} archive input must be a regular file", kind.label()));
        }
        let file = fs::File::open(path)
            .map_err(|error| format!("unable to open {} archive: {error}", kind.label()))?;
        let budget = Self::for_file(&file, kind)?;
        if path_metadata.len() != budget.archive_bytes {
            return Err(format!("{} archive changed while it was being opened", kind.label()));
        }
        Ok((file, budget))
    }

    pub(crate) fn for_file(file: &fs::File, kind: ArchivePathKind) -> Result<Self, String> {
        let metadata =
            file.metadata().map_err(|error| format!("unable to inspect archive size: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err(format!("{} archive input must be a regular file", kind.label()));
        }
        let archive_bytes = metadata.len();
        Self::with_limits(kind, archive_bytes, DEFAULT_ARCHIVE_LIMITS)
    }

    pub(crate) fn verify_file_unchanged(&self, file: &fs::File) -> Result<(), String> {
        let metadata = file
            .metadata()
            .map_err(|error| format!("unable to re-inspect archive size: {error}"))?;
        if !metadata.file_type().is_file() || metadata.len() != self.archive_bytes {
            return Err(format!(
                "{} archive changed while it was being inspected",
                self.kind.label()
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_zip_directory(&self, file: &mut fs::File) -> Result<(), String> {
        let directory = zip_directory_info(file, self.archive_bytes)?;
        if directory.members > self.limits.max_members as u64 {
            return Err(format!(
                "{} archive contains {} members, exceeding the {} member limit",
                self.kind.label(),
                directory.members,
                self.limits.max_members
            ));
        }
        if directory.bytes > self.limits.max_central_directory_bytes {
            return Err(format!(
                "{} archive central directory is {} bytes, exceeding the {} byte limit",
                self.kind.label(),
                directory.bytes,
                self.limits.max_central_directory_bytes
            ));
        }
        Ok(())
    }

    fn with_limits(
        kind: ArchivePathKind,
        archive_bytes: u64,
        limits: ArchiveLimits,
    ) -> Result<Self, String> {
        if archive_bytes > limits.max_archive_bytes {
            return Err(format!(
                "{} archive is {archive_bytes} bytes, exceeding the {} byte input limit",
                kind.label(),
                limits.max_archive_bytes
            ));
        }
        Ok(Self { kind, limits, archive_bytes, members: 0, declared_bytes: 0, read_bytes: 0 })
    }

    pub(crate) fn register_member(&mut self) -> Result<(), String> {
        self.members = self
            .members
            .checked_add(1)
            .ok_or_else(|| format!("{} archive member count overflowed", self.kind.label()))?;
        if self.members > self.limits.max_members {
            return Err(format!(
                "{} archive contains more than {} members",
                self.kind.label(),
                self.limits.max_members
            ));
        }
        Ok(())
    }

    pub(crate) fn register_payload(
        &mut self,
        path: &str,
        declared_bytes: u64,
        compressed_bytes: Option<u64>,
    ) -> Result<(), String> {
        if declared_bytes > self.limits.max_entry_bytes {
            return Err(format!(
                "{} archive member `{path}` declares {declared_bytes} uncompressed bytes, exceeding the {} byte per-member limit",
                self.kind.label(),
                self.limits.max_entry_bytes
            ));
        }
        self.declared_bytes = self
            .declared_bytes
            .checked_add(declared_bytes)
            .ok_or_else(|| format!("{} archive size accounting overflowed", self.kind.label()))?;
        if self.declared_bytes > self.limits.max_total_bytes {
            return Err(format!(
                "{} archive declares {} total uncompressed bytes, exceeding the {} byte limit",
                self.kind.label(),
                self.declared_bytes,
                self.limits.max_total_bytes
            ));
        }
        if compressed_bytes
            .is_some_and(|compressed| self.exceeds_compression_ratio(declared_bytes, compressed))
        {
            return Err(format!(
                "{} archive member `{path}` exceeds the allowed compression ratio",
                self.kind.label()
            ));
        }
        if self.exceeds_compression_ratio(self.declared_bytes, self.archive_bytes) {
            return Err(format!(
                "{} archive exceeds the allowed total compression ratio",
                self.kind.label()
            ));
        }
        Ok(())
    }

    pub(crate) fn read_entry(
        &mut self,
        reader: &mut impl Read,
        path: &str,
        declared_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        let read_limit = self
            .limits
            .max_entry_bytes
            .checked_add(1)
            .ok_or_else(|| String::from("archive per-member read limit overflowed"))?;
        let mut bytes = Vec::new();
        reader.take(read_limit).read_to_end(&mut bytes).map_err(|error| {
            format!("unable to read {} archive member `{path}`: {error}", self.kind.label())
        })?;
        let actual_bytes = u64::try_from(bytes.len())
            .map_err(|_| format!("{} archive member `{path}` is too large", self.kind.label()))?;
        if actual_bytes > self.limits.max_entry_bytes {
            return Err(format!(
                "{} archive member `{path}` exceeds the {} byte per-member limit while reading",
                self.kind.label(),
                self.limits.max_entry_bytes
            ));
        }
        if actual_bytes != declared_bytes {
            return Err(format!(
                "{} archive member `{path}` declared {declared_bytes} bytes but yielded {actual_bytes}",
                self.kind.label()
            ));
        }
        self.read_bytes = self
            .read_bytes
            .checked_add(actual_bytes)
            .ok_or_else(|| format!("{} archive read accounting overflowed", self.kind.label()))?;
        if self.read_bytes > self.limits.max_total_bytes {
            return Err(format!(
                "{} archive exceeded the {} byte total read limit",
                self.kind.label(),
                self.limits.max_total_bytes
            ));
        }
        Ok(bytes)
    }

    fn exceeds_compression_ratio(&self, uncompressed: u64, compressed: u64) -> bool {
        uncompressed >= self.limits.compression_ratio_min_bytes
            && uncompressed
                > compressed
                    .saturating_mul(self.limits.max_compression_ratio)
                    .saturating_add(self.limits.compression_ratio_slack)
    }
}

struct ZipDirectoryInfo {
    members: u64,
    bytes: u64,
}

fn zip_directory_info(
    file: &mut (impl Read + Seek),
    archive_bytes: u64,
) -> Result<ZipDirectoryInfo, String> {
    const EOCD_BYTES: usize = 22;
    const MAX_COMMENT_BYTES: u64 = u16::MAX as u64;
    if archive_bytes < EOCD_BYTES as u64 {
        return Err(String::from("zip archive is too short to contain an end record"));
    }
    let search_bytes = archive_bytes.min(MAX_COMMENT_BYTES + EOCD_BYTES as u64);
    let search_len = usize::try_from(search_bytes)
        .map_err(|_| String::from("zip end-record search window is too large"))?;
    let search_offset = archive_bytes - search_bytes;
    file.seek(SeekFrom::Start(search_offset))
        .map_err(|error| format!("unable to seek to zip end record: {error}"))?;
    let mut tail = vec![0_u8; search_len];
    file.read_exact(&mut tail)
        .map_err(|error| format!("unable to read zip end record: {error}"))?;

    let relative_offset = (0..=tail.len() - EOCD_BYTES)
        .rev()
        .find(|offset| {
            tail[*offset..].starts_with(b"PK\x05\x06")
                && *offset + EOCD_BYTES + usize::from(le_u16(&tail[*offset + 20..])) == tail.len()
        })
        .ok_or_else(|| String::from("zip archive has no valid end-of-central-directory record"))?;
    let eocd = &tail[relative_offset..relative_offset + EOCD_BYTES];
    let eocd_offset = search_offset
        .checked_add(relative_offset as u64)
        .ok_or_else(|| String::from("zip end-record offset overflowed"))?;

    let disk = le_u16(&eocd[4..]);
    let directory_disk = le_u16(&eocd[6..]);
    let disk_members = le_u16(&eocd[8..]);
    let members = le_u16(&eocd[10..]);
    let directory_bytes = le_u32(&eocd[12..]);
    let directory_offset = le_u32(&eocd[16..]);
    if disk != 0 || directory_disk != 0 {
        return Err(String::from("multi-disk zip archives are not supported"));
    }

    let saturated = disk_members == u16::MAX
        || members == u16::MAX
        || directory_bytes == u32::MAX
        || directory_offset == u32::MAX;
    if saturated {
        return zip64_directory_info(file, archive_bytes, eocd_offset);
    }
    if disk_members != members {
        return Err(String::from("zip end record has inconsistent member counts"));
    }
    validate_zip_directory_bounds(
        u64::from(directory_offset),
        u64::from(directory_bytes),
        eocd_offset,
        archive_bytes,
    )?;
    Ok(ZipDirectoryInfo { members: u64::from(members), bytes: u64::from(directory_bytes) })
}

fn zip64_directory_info(
    file: &mut (impl Read + Seek),
    archive_bytes: u64,
    eocd_offset: u64,
) -> Result<ZipDirectoryInfo, String> {
    const LOCATOR_BYTES: u64 = 20;
    const ZIP64_EOCD_BYTES: usize = 56;
    let locator_offset = eocd_offset
        .checked_sub(LOCATOR_BYTES)
        .ok_or_else(|| String::from("zip64 archive is missing its end-record locator"))?;
    file.seek(SeekFrom::Start(locator_offset))
        .map_err(|error| format!("unable to seek to zip64 locator: {error}"))?;
    let mut locator = [0_u8; LOCATOR_BYTES as usize];
    file.read_exact(&mut locator)
        .map_err(|error| format!("unable to read zip64 locator: {error}"))?;
    if !locator.starts_with(b"PK\x06\x07") {
        return Err(String::from("zip64 archive has no valid end-record locator"));
    }
    if le_u32(&locator[4..]) != 0 || le_u32(&locator[16..]) != 1 {
        return Err(String::from("multi-disk zip64 archives are not supported"));
    }
    let zip64_offset = le_u64(&locator[8..]);
    file.seek(SeekFrom::Start(zip64_offset))
        .map_err(|error| format!("unable to seek to zip64 end record: {error}"))?;
    let mut eocd = [0_u8; ZIP64_EOCD_BYTES];
    file.read_exact(&mut eocd)
        .map_err(|error| format!("unable to read zip64 end record: {error}"))?;
    if !eocd.starts_with(b"PK\x06\x06") {
        return Err(String::from("zip64 archive has no valid end record"));
    }
    let record_size = le_u64(&eocd[4..]);
    if record_size < 44 {
        return Err(String::from("zip64 end record is too short"));
    }
    let record_end = zip64_offset
        .checked_add(12)
        .and_then(|offset| offset.checked_add(record_size))
        .ok_or_else(|| String::from("zip64 end-record size overflowed"))?;
    if record_end != locator_offset || record_end > archive_bytes {
        return Err(String::from("zip64 end record has inconsistent bounds"));
    }
    if le_u32(&eocd[16..]) != 0 || le_u32(&eocd[20..]) != 0 {
        return Err(String::from("multi-disk zip64 archives are not supported"));
    }
    let disk_members = le_u64(&eocd[24..]);
    let members = le_u64(&eocd[32..]);
    if disk_members != members {
        return Err(String::from("zip64 end record has inconsistent member counts"));
    }
    let directory_bytes = le_u64(&eocd[40..]);
    let directory_offset = le_u64(&eocd[48..]);
    validate_zip_directory_bounds(directory_offset, directory_bytes, zip64_offset, archive_bytes)?;
    Ok(ZipDirectoryInfo { members, bytes: directory_bytes })
}

fn validate_zip_directory_bounds(
    offset: u64,
    bytes: u64,
    end_limit: u64,
    archive_bytes: u64,
) -> Result<(), String> {
    let end = offset
        .checked_add(bytes)
        .ok_or_else(|| String::from("zip central-directory bounds overflowed"))?;
    if end > end_limit || end > archive_bytes {
        return Err(String::from("zip central directory lies outside the archive"));
    }
    Ok(())
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

impl ArchiveMemberPaths {
    pub(crate) fn new(kind: ArchivePathKind) -> Self {
        Self { kind, seen: BTreeSet::new(), files: BTreeSet::new() }
    }

    pub(crate) fn register(
        &mut self,
        raw_path: &[u8],
        is_directory: bool,
    ) -> Result<String, String> {
        let path = validate_archive_member_path(raw_path, self.kind, is_directory)?;
        if self.seen.contains(&path) {
            return Err(format!(
                "{} archive contains duplicate member path `{path}`",
                self.kind.label()
            ));
        }

        let mut ancestor = String::new();
        let components = path.split('/').collect::<Vec<_>>();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            if !ancestor.is_empty() {
                ancestor.push('/');
            }
            ancestor.push_str(component);
            if self.files.contains(&ancestor) {
                return Err(format!(
                    "{} archive member `{path}` is nested below file member `{ancestor}`",
                    self.kind.label()
                ));
            }
        }

        if !is_directory {
            let descendant_prefix = format!("{path}/");
            if let Some(descendant) = self.seen.range(descendant_prefix.clone()..).next()
                && descendant.starts_with(&descendant_prefix)
            {
                return Err(format!(
                    "{} archive file member `{path}` conflicts with member `{descendant}`",
                    self.kind.label()
                ));
            }
        }

        self.seen.insert(path.clone());
        if !is_directory {
            self.files.insert(path.clone());
        }
        Ok(path)
    }
}

pub(crate) fn validate_wheel_record_path(path: &str) -> Result<String, String> {
    validate_archive_member_path(path.as_bytes(), ArchivePathKind::Wheel, false)
}

fn validate_archive_member_path(
    raw_path: &[u8],
    kind: ArchivePathKind,
    is_directory: bool,
) -> Result<String, String> {
    let rendered = str::from_utf8(raw_path).map_err(|error| {
        format!("{} archive member path is not valid UTF-8: {error}", kind.label())
    })?;
    if rendered.contains('\0') {
        return Err(format!("{} archive member path contains a NUL byte", kind.label()));
    }
    if rendered.contains('\\') {
        return Err(format!(
            "{} archive member path `{rendered}` uses a backslash separator",
            kind.label()
        ));
    }

    if rendered.starts_with('/') {
        return Err(format!("{} archive member path `{rendered}` must be relative", kind.label()));
    }
    let mut path = rendered;
    if path.starts_with('/') {
        return Err(format!(
            "{} archive member path `{rendered}` contains an empty component",
            kind.label()
        ));
    }
    if path.ends_with('/') {
        if !is_directory {
            return Err(format!(
                "{} archive file member path `{rendered}` has a trailing separator",
                kind.label()
            ));
        }
        path = path.strip_suffix('/').unwrap_or(path);
    }
    if path.is_empty() {
        return Err(format!("{} archive member path is empty", kind.label()));
    }

    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(format!(
            "{} archive member path `{rendered}` contains a Windows drive prefix",
            kind.label()
        ));
    }
    for component in path.split('/') {
        match component {
            "" => {
                return Err(format!(
                    "{} archive member path `{rendered}` contains an empty component",
                    kind.label()
                ));
            }
            "." | ".." => {
                return Err(format!(
                    "{} archive member path `{rendered}` contains forbidden component `{component}`",
                    kind.label()
                ));
            }
            _ => {}
        }
    }

    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn validates_strict_posix_archive_paths() {
        let invalid = [
            b"../outside.py".as_slice(),
            b"pkg/./module.py".as_slice(),
            b"pkg//module.py".as_slice(),
            b"pkg\\module.py".as_slice(),
            b"C:/module.py".as_slice(),
            b"pkg/module.py\0ignored".as_slice(),
            &[0xff, b'.', b'p', b'y'],
        ];
        for path in invalid {
            assert!(
                validate_archive_member_path(path, ArchivePathKind::Wheel, false).is_err(),
                "path should be rejected: {path:?}"
            );
        }
        assert!(
            validate_archive_member_path(b"/pkg/module.py", ArchivePathKind::Wheel, false).is_err()
        );
        assert!(
            validate_archive_member_path(b"/pkg/module.py", ArchivePathKind::Sdist, false).is_err()
        );
        assert_eq!(
            validate_archive_member_path(b"pkg/", ArchivePathKind::Wheel, true),
            Ok(String::from("pkg"))
        );
    }

    #[test]
    fn rejects_duplicate_and_file_directory_conflicts() {
        let mut paths = ArchiveMemberPaths::new(ArchivePathKind::Wheel);
        assert_eq!(paths.register(b"pkg/module.py", false), Ok(String::from("pkg/module.py")));
        assert!(paths.register(b"pkg/module.py", false).is_err());

        let mut ancestor_file = ArchiveMemberPaths::new(ArchivePathKind::Wheel);
        assert!(ancestor_file.register(b"pkg", false).is_ok());
        assert!(ancestor_file.register(b"pkg/module.py", false).is_err());

        let mut descendant_file = ArchiveMemberPaths::new(ArchivePathKind::Wheel);
        assert!(descendant_file.register(b"pkg/module.py", false).is_ok());
        assert!(descendant_file.register(b"pkg", false).is_err());

        let mut directory_file = ArchiveMemberPaths::new(ArchivePathKind::Wheel);
        assert!(directory_file.register(b"pkg/", true).is_ok());
        assert!(directory_file.register(b"pkg", false).is_err());

        let mut relative_sdist = ArchiveMemberPaths::new(ArchivePathKind::Sdist);
        assert!(relative_sdist.register(b"/pkg/module.py", false).is_err());
        assert!(relative_sdist.register(b"pkg/module.py", false).is_ok());
    }

    #[test]
    fn enforces_archive_resource_limits_and_bounded_reads() {
        let limits = ArchiveLimits {
            max_archive_bytes: 10,
            max_members: 2,
            max_entry_bytes: 4,
            max_total_bytes: 6,
            max_tar_stream_bytes: 8,
            max_central_directory_bytes: 5,
            max_compression_ratio: 2,
            compression_ratio_min_bytes: 0,
            compression_ratio_slack: 0,
        };
        assert!(ArchiveReadBudget::with_limits(ArchivePathKind::Wheel, 11, limits).is_err());

        let mut member_limit = ArchiveReadBudget::with_limits(ArchivePathKind::Wheel, 10, limits)
            .expect("test archive budget should be valid");
        assert!(member_limit.register_member().is_ok());
        assert!(member_limit.register_member().is_ok());
        assert!(member_limit.register_member().is_err());

        let mut entry_limit = ArchiveReadBudget::with_limits(ArchivePathKind::Wheel, 10, limits)
            .expect("test archive budget should be valid");
        assert!(entry_limit.register_payload("large", 5, Some(5)).is_err());

        let mut ratio_limit = ArchiveReadBudget::with_limits(ArchivePathKind::Wheel, 10, limits)
            .expect("test archive budget should be valid");
        assert!(ratio_limit.register_payload("bomb", 4, Some(1)).is_err());

        let mut read_limit = ArchiveReadBudget::with_limits(ArchivePathKind::Wheel, 10, limits)
            .expect("test archive budget should be valid");
        assert!(read_limit.register_payload("forged", 4, Some(4)).is_ok());
        assert!(read_limit.read_entry(&mut b"12345".as_slice(), "forged", 4).is_err());

        let mut stream = BoundedArchiveReader::with_limit(
            b"12345".as_slice(),
            ArchivePathKind::Sdist,
            "test stream",
            4,
        );
        let mut decoded = Vec::new();
        assert!(stream.read_to_end(&mut decoded).is_err());
    }

    #[test]
    fn preflights_standard_and_zip64_member_counts() {
        let mut standard = vec![0_u8; 22];
        standard[0..4].copy_from_slice(b"PK\x05\x06");
        standard[8..10].copy_from_slice(&3_u16.to_le_bytes());
        standard[10..12].copy_from_slice(&3_u16.to_le_bytes());
        let standard_info = zip_directory_info(&mut Cursor::new(standard), 22)
            .expect("standard zip end record should parse");
        assert_eq!(standard_info.members, 3);

        let mut zip64 = vec![0_u8; 98];
        zip64[0..4].copy_from_slice(b"PK\x06\x06");
        zip64[4..12].copy_from_slice(&44_u64.to_le_bytes());
        zip64[24..32].copy_from_slice(&4_u64.to_le_bytes());
        zip64[32..40].copy_from_slice(&4_u64.to_le_bytes());
        zip64[56..60].copy_from_slice(b"PK\x06\x07");
        zip64[72..76].copy_from_slice(&1_u32.to_le_bytes());
        zip64[76..80].copy_from_slice(b"PK\x05\x06");
        zip64[84..86].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64[86..88].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64[88..92].copy_from_slice(&u32::MAX.to_le_bytes());
        zip64[92..96].copy_from_slice(&u32::MAX.to_le_bytes());
        let zip64_info = zip_directory_info(&mut Cursor::new(zip64), 98)
            .expect("zip64 end records should parse");
        assert_eq!(zip64_info.members, 4);
    }

    #[test]
    fn bounds_tar_extension_processing_before_entries_are_returned() {
        let mut encoded = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut encoded);
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(1);
            header.set_cksum();
            let long_path = format!("{}/module.py", "long-component".repeat(20));
            builder
                .append_data(&mut header, long_path, b"x".as_slice())
                .expect("long-path tar entry should be written");
            builder.finish().expect("tar archive should finish");
        }

        let bounded = BoundedArchiveReader::with_limit(
            Cursor::new(encoded),
            ArchivePathKind::Sdist,
            "decoded tar test stream",
            1024,
        );
        let mut archive = tar::Archive::new(bounded);
        let mut entries = archive.entries().expect("tar entry iterator should initialize");
        assert!(
            entries.next().expect("tar iterator should report the bounded stream failure").is_err()
        );
    }
}
