//! Native file picker integration; disk I/O remains in file_system.

use super::file_system::{load_file, save_file};
use super::types::{FileError, FileOpenResult, FileResult, FileSaveResult, SaveFileDialogOptions};
use std::{future::Future, path::PathBuf};

pub fn pick_file(window: &dyn iced::Window) -> impl Future<Output = FileResult<PathBuf>> + use<> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_title("Open a text file...")
        .set_parent(&window);

    async move {
        dialog
            .pick_file()
            .await
            .map(|picked_file| picked_file.path().to_owned())
            .ok_or(FileError::DialogClosed)
    }
}

pub async fn open_file(window: &dyn iced::Window) -> FileOpenResult {
    let path = pick_file(window).await?;

    load_file(path).await
}

pub fn save_file_as(
    window: &dyn iced::Window,
    contents: Vec<u8>,
) -> impl Future<Output = FileSaveResult> + use<> {
    save_file_as_with_options(window, contents, SaveFileDialogOptions::default())
}

pub fn save_file_as_with_options(
    window: &dyn iced::Window,
    contents: Vec<u8>,
    options: SaveFileDialogOptions,
) -> impl Future<Output = FileSaveResult> + use<> {
    save_file_with_dialog(window, contents, "Save text file...", options)
}

pub fn save_file_copy_as(
    window: &dyn iced::Window,
    contents: Vec<u8>,
) -> impl Future<Output = FileSaveResult> + use<> {
    save_file_copy_as_with_options(window, contents, SaveFileDialogOptions::default())
}

pub fn save_file_copy_as_with_options(
    window: &dyn iced::Window,
    contents: Vec<u8>,
    options: SaveFileDialogOptions,
) -> impl Future<Output = FileSaveResult> + use<> {
    save_file_with_dialog(window, contents, "Save a copy as...", options)
}

fn save_file_with_dialog(
    window: &dyn iced::Window,
    contents: Vec<u8>,
    title: &'static str,
    options: SaveFileDialogOptions,
) -> impl Future<Output = FileSaveResult> + use<> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_parent(&window);

    if let Some(file_name) = options.file_name {
        dialog = dialog.set_file_name(file_name);
    }
    if let Some(filter) = options.filter {
        dialog = dialog
            .add_filter(filter.name, &[filter.extension])
            .add_filter("All files", &["*"]);
    }

    async move {
        let picked_file = dialog.save_file().await.ok_or(FileError::DialogClosed)?;

        save_file(picked_file.path().to_owned(), contents).await
    }
}
