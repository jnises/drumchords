#[cfg(target_arch = "wasm32")]
fn save_midi_file(data: &[u8]) {
    // TODO generate blob and use it to download the file    
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_midi_file(data: &[u8]) {
    use std::io::Write;
    use rfd::{FileDialog, MessageDialog, MessageLevel};
    use anyhow::Result;

    if let Err(e) = (|| -> Result<_> {
        if let Some(path) = FileDialog::new()
            .add_filter("Standard MIDI File", &["mid", "midi"])
            .save_file()
        {
            let mut f = std::fs::File::create(path)?;
            f.write_all(data)?;
        }
        Ok(())
    })() {
        let _ =  MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("error")
            .set_description(&e.to_string())
            .show();
    }
}
