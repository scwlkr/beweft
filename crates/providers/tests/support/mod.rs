use std::{path::PathBuf, process::Command};
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
pub fn fixture_bytes(video: bool) -> Vec<u8> {
    custom_bytes(video, "640x480", "4")
}
pub fn custom_bytes(video: bool, dimensions: &str, seconds: &str) -> Vec<u8> {
    let path = std::env::temp_dir().join(format!(
        "beweft-media-fixture-{}.{}",
        uuid::Uuid::new_v4(),
        if video { "mp4" } else { "jpg" }
    ));
    let _remove = Remove(path.clone());
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-nostdin",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        &format!("color=c=blue:s={dimensions}:r=10"),
    ]);
    if video {
        cmd.args([
            "-t",
            seconds,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ]);
    } else {
        cmd.args(["-frames:v", "1"]);
    }
    assert!(
        cmd.arg(&path).status().unwrap().success(),
        "ffmpeg test fixture generation"
    );
    std::fs::read(&path).unwrap()
}
