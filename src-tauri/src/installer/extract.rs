use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    SevenZip,
    Rar,
}

impl ArchiveKind {
    pub fn label(self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::SevenZip => "7z",
            ArchiveKind::Rar => "rar",
        }
    }
}

/// Prefer magic bytes over the extension so a renamed archive still works.
pub fn detect_kind(path: &Path) -> AppResult<ArchiveKind> {
    if !path.is_file() {
        return Err(AppError::ArchiveUnsupported(format!(
            "File not found: {}",
            path.display()
        )));
    }

    let mut file = File::open(path)?;
    let mut magic = [0u8; 8];
    let read = file.read(&mut magic)?;
    let bytes = &magic[..read];

    let kind = if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        Some(ArchiveKind::Zip)
    } else if bytes.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        Some(ArchiveKind::SevenZip)
    } else if bytes.starts_with(b"Rar!\x1A\x07") {
        Some(ArchiveKind::Rar)
    } else {
        None
    };

    kind.ok_or_else(|| AppError::ArchiveUnsupported(path.display().to_string()))
}

pub fn extract_archive(
    path: &Path,
    destination: &Path,
    kind: ArchiveKind,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u8, String),
) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;
    fs::create_dir_all(destination)?;

    match kind {
        ArchiveKind::Zip => extract_zip(path, destination, cancel, progress),
        ArchiveKind::SevenZip => extract_seven_zip(path, destination, cancel, progress),
        ArchiveKind::Rar => extract_rar(path, destination, cancel, progress),
    }
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

fn extract_zip(
    path: &Path,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u8, String),
) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| AppError::ArchiveExtract(format!("Could not read zip: {error}")))?;

    let total = archive.len().max(1);

    for index in 0..archive.len() {
        ensure_not_cancelled(cancel)?;
        let mut entry = archive.by_index(index).map_err(|error| {
            AppError::ArchiveExtract(format!("Could not read zip entry: {error}"))
        })?;

        let entry_name = entry
            .name()
            .map_err(|error| {
                AppError::ArchiveExtract(format!("Could not read zip entry name: {error}"))
            })?
            .to_string();

        // `enclosed_name` rejects absolute paths and `..` traversal.
        let Some(relative) = entry.enclosed_name() else {
            return Err(AppError::ArchiveExtract(format!(
                "Zip entry has an unsafe path: {entry_name}"
            )));
        };

        let output = destination.join(&relative);

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output_file = File::create(&output).map_err(|error| {
                AppError::ArchiveExtract(format!("Could not write {entry_name}: {error}"))
            })?;
            io::copy(&mut entry, &mut output_file).map_err(|error| {
                AppError::ArchiveExtract(format!("Could not extract {entry_name}: {error}"))
            })?;
            ensure_not_cancelled(cancel)?;
        }

        let percent = 10 + (((index + 1) as u64 * 50) / total as u64) as u8;
        progress(percent.min(60), format!("Extracted {entry_name}"));
    }

    Ok(())
}

fn extract_seven_zip(
    path: &Path,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u8, String),
) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;
    progress(20, "Extracting 7z archive…".to_string());

    // The high-level helper performs traversal checks for every entry. It does
    // not expose a per-entry callback, so cancellation takes effect as soon as
    // this single extraction call returns.
    sevenz_rust2::decompress_file(path, destination)
        .map_err(|error| AppError::ArchiveExtract(format!("7z extraction failed: {error}")))?;

    ensure_not_cancelled(cancel)?;
    progress(60, "Extracted 7z archive".to_string());
    Ok(())
}

fn extract_rar(
    path: &Path,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u8, String),
) -> AppResult<()> {
    ensure_not_cancelled(cancel)?;
    // Validate entry names before asking the C library to write anything.
    validate_rar_entries(path)?;

    progress(20, "Extracting RAR archive…".to_string());

    let mut archive = unrar::Archive::new(path)
        .as_first_part()
        .open_for_processing()
        .map_err(|error| AppError::ArchiveExtract(format!("Could not open RAR: {error}")))?;

    let mut percent = 20u8;
    let mut extracted = 0u32;

    loop {
        ensure_not_cancelled(cancel)?;
        let Some(header) = archive.read_header().map_err(|error| {
            AppError::ArchiveExtract(format!("Could not read RAR entry: {error}"))
        })?
        else {
            break;
        };

        let entry_name = header.entry().filename.to_string_lossy().to_string();

        if header.entry().is_directory() {
            archive = header.skip().map_err(|error| {
                AppError::ArchiveExtract(format!("Could not skip RAR entry: {error}"))
            })?;
            continue;
        }

        archive = header.extract_with_base(destination).map_err(|error| {
            AppError::ArchiveExtract(format!("Could not extract {entry_name}: {error}"))
        })?;
        ensure_not_cancelled(cancel)?;

        extracted += 1;
        percent = (percent + 1).min(60);
        progress(percent, format!("Extracted {entry_name}"));
    }

    if extracted == 0 {
        return Err(AppError::ArchiveExtract(
            "The RAR archive contains no files.".to_string(),
        ));
    }

    Ok(())
}

fn validate_rar_entries(path: &Path) -> AppResult<()> {
    use std::path::Component;

    let archive = unrar::Archive::new(path)
        .as_first_part()
        .open_for_listing()
        .map_err(|error| AppError::ArchiveExtract(format!("Could not read RAR: {error}")))?;

    for entry in archive {
        let entry = entry.map_err(|error| {
            AppError::ArchiveExtract(format!("Could not read RAR entry: {error}"))
        })?;

        let unsafe_path = entry.filename.is_absolute()
            || entry
                .filename
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir));

        if unsafe_path {
            return Err(AppError::ArchiveExtract(format!(
                "RAR entry has an unsafe path: {}",
                entry.filename.display()
            )));
        }
    }

    Ok(())
}
