//! Bounds and prompt formatting for Friendly's normalized PNG attachments.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_IMAGES: usize = 8;

/// Validate the envelope before storage; the frontend decodes and re-encodes pixels as PNG.
/// This does not claim to decode the compressed pixels.
pub fn validate_png(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_IMAGE_BYTES || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Attach a PNG image of at most 5 MiB.".into());
    }
    let mut at = 8;
    let mut header = false;
    let mut pixels = false;
    while at + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let end = at.checked_add(12).and_then(|n| n.checked_add(len)).ok_or("Invalid PNG size.")?;
        if end > bytes.len() {
            return Err("Incomplete PNG image.".into());
        }
        let kind = &bytes[at + 4..at + 8];
        if !header {
            if kind != b"IHDR" || len != 13 {
                return Err("Invalid PNG header.".into());
            }
            let width = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap());
            if width == 0 || height == 0 || width > 4096 || height > 4096 {
                return Err("Image dimensions must be between 1 and 4096 pixels.".into());
            }
            header = true;
        } else if kind == b"IHDR" {
            return Err("Duplicate PNG header.".into());
        }
        if kind == b"IDAT" && len > 0 {
            pixels = true;
        }
        if kind == b"IEND" {
            return if pixels && len == 0 && end == bytes.len() { Ok(()) } else { Err("Invalid PNG ending.".into()) };
        }
        at = end;
    }
    Err("Incomplete PNG image.".into())
}

pub fn image_prompt(text: &str, paths: &[String]) -> Result<String, String> {
    if paths.is_empty() {
        return crate::actions::clean_prompt(text);
    }
    let mut out = if text.trim().is_empty() { "Please examine the attached images.".to_owned() } else { text.to_owned() };
    out.push_str("\n\nAttached images (open these local image files to view them):");
    for (i, path) in paths.iter().enumerate() {
        if path.chars().any(|c| c.is_control() || c == '"') {
            return Err("Image path contains unsupported characters.".into());
        }
        out.push_str(&format!("\nImage {}: \"{}\"", i + 1, path.replace('\\', "/")));
    }
    crate::actions::clean_prompt(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_images_truncation_and_oversized_dimensions() {
        assert!(validate_png(b"<svg onload='bad'>").is_err());
        assert!(validate_png(b"\x89PNG\r\n\x1a\n").is_err());
        let png = include_bytes!("../../../src-tauri/icons/32x32.png");
        validate_png(png).unwrap();
        assert!(validate_png(&png[..png.len() - 2]).is_err());
        let mut huge = png.to_vec();
        huge[16..20].copy_from_slice(&5000u32.to_be_bytes());
        assert!(validate_png(&huge).is_err());
    }
    #[test]
    fn image_only_prompts_and_paths_are_explicit_without_terminal_controls() {
        let p = image_prompt("", &[r"C:\my images\1.png".into()]).unwrap();
        assert!(p.contains("Please examine"));
        assert!(p.contains("Image 1: \"C:/my images/1.png\""));
        assert!(image_prompt("look", &["bad\rpath".into()]).is_err());
        assert!(image_prompt(&"x".repeat(16384), &["a.png".into()]).is_err());
    }
}
