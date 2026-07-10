use std::str;

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
}

impl ArchiveMemberPaths {
    pub(crate) fn new(kind: ArchivePathKind) -> Self {
        Self { kind }
    }

    pub(crate) fn register(
        &mut self,
        raw_path: &[u8],
        is_directory: bool,
    ) -> Result<String, String> {
        validate_archive_member_path(raw_path, self.kind, is_directory)
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

    let mut path = match kind {
        ArchivePathKind::Wheel if rendered.starts_with('/') => {
            return Err(format!("wheel archive member path `{rendered}` must be relative"));
        }
        ArchivePathKind::Sdist => rendered.strip_prefix('/').unwrap_or(rendered),
        ArchivePathKind::Wheel => rendered,
    };
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
        assert_eq!(
            validate_archive_member_path(b"/pkg/module.py", ArchivePathKind::Sdist, false),
            Ok(String::from("pkg/module.py"))
        );
        assert_eq!(
            validate_archive_member_path(b"pkg/", ArchivePathKind::Wheel, true),
            Ok(String::from("pkg"))
        );
    }
}
