use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Publish a complete artifact without truncating an input or existing output
/// on a write failure. The temporary file lives on the destination filesystem.
pub(super) fn publish(input: &Path, output: &Path, bytes: &[u8]) -> io::Result<()> {
    publish_with(input, output, |file| file.write_all(bytes))
}

fn check_distinct(input: &Path, output: &Path) -> io::Result<Option<fs::Permissions>> {
    let metadata = match fs::metadata(output) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if same_file::is_same_file(input, output)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input and output refer to the same file",
        ));
    }
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "output must be a regular file"));
    }
    Ok(Some(metadata.permissions()))
}

pub(super) fn publish_with(
    input: &Path,
    output: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let permissions = check_distinct(input, output)?;
    let directory = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temporary = tempfile::Builder::new().prefix(".whale-output-").tempfile_in(directory)?;
    write(temporary.as_file_mut())?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    // Recheck after writing too, in case a path changed while producing output.
    check_distinct(input, output)?;
    temporary.persist(output).map_err(|error| error.error)?;
    Ok(())
}

/// Publish an artifact while protecting every input, including hard/symlink aliases.
/// Executables receive an explicit 0755 mode on Unix; no input or previous mode
/// can propagate setuid/setgid bits into a newly linked artifact.
pub(super) fn publish_many(
    inputs: &[&Path],
    output: &Path,
    bytes: &[u8],
    executable: bool,
) -> io::Result<()> {
    for input in inputs {
        check_distinct(input, output)?;
    }
    let directory = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temporary = tempfile::Builder::new().prefix(".whale-output-").tempfile_in(directory)?;
    temporary.write_all(bytes)?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        temporary.as_file().set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    temporary.as_file().sync_all()?;
    for input in inputs {
        check_distinct(input, output)?;
    }
    temporary.persist(output).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_write_failure_preserves_output_and_removes_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("source");
        let output = directory.path().join("output with spaces");
        fs::write(&input, b"source").unwrap();
        for existing in [false, true] {
            if existing {
                fs::write(&output, b"old output").unwrap();
            }
            let error = publish_with(&input, &output, |file| {
                file.write_all(b"partial output")?;
                Err(io::Error::other("injected write failure"))
            })
            .unwrap_err();
            assert_eq!(error.to_string(), "injected write failure");
            if existing {
                assert_eq!(fs::read(&output).unwrap(), b"old output");
            } else {
                assert!(!output.exists());
            }
            assert_eq!(fs::read(&input).unwrap(), b"source");
            assert_eq!(
                fs::read_dir(directory.path()).unwrap().count(),
                if existing { 2 } else { 1 }
            );
        }
    }
}
