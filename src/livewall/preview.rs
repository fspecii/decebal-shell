//! One muted local preview decoder. Bounded frames; dropping the player kills and reaps it.
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;
pub struct Player {
    child: Child,
    reader: Option<JoinHandle<()>>,
    frames: mpsc::Receiver<Vec<u8>>,
}
impl Player {
    pub fn start(path: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let mut child = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-threads", "1", "-filter_threads", "1", "-protocol_whitelist", "file,pipe", "-stream_loop", "-1", "-re", "-i"])
            .arg(path)
            .args([
                "-an",
                "-sn",
                "-dn",
                "-vf",
                "fps=12,scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2",
                "-threads",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Video preview: {e}"))?;
        let mut output = child.stdout.take().ok_or("Video preview has no output")?;
        let (tx, frames) = mpsc::sync_channel(2);
        let reader = match std::thread::Builder::new().name("wallpaper-preview".into()).spawn(move || {
            loop {
                let mut frame = vec![0; (WIDTH * HEIGHT * 4) as usize];
                if output.read_exact(&mut frame).is_err() {
                    break;
                }
                match tx.try_send(frame) {
                    Ok(()) => wake(),
                    Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
            wake();
        }) {
            Ok(t) => t,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        };
        Ok(Self { child, reader: Some(reader), frames })
    }
    pub fn latest(&self) -> Option<Vec<u8>> {
        self.frames.try_iter().last()
    }
    pub fn ended(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_frames_are_bounded_and_decoder_is_reaped() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("skipped: ffmpeg is not installed, so there is no clip to decode");
            return;
        }
        let path = std::env::temp_dir().join(format!("decebal-preview-{}.mp4", std::process::id()));
        assert!(Command::new("ffmpeg").args(["-nostdin", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=12", "-t", "1", "-threads", "1", "-y"]).arg(&path).status().unwrap().success());
        let mut player = Player::start(&path, Arc::new(|| {})).unwrap();
        let pid = player.child.id();
        let mut seen = Vec::new();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while seen.len() < 2 && std::time::Instant::now() < end {
            if let Some(frame) = player.latest() {
                assert_eq!(frame.len(), (WIDTH * HEIGHT * 4) as usize);
                seen.push(frame);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(seen.len(), 2);
        assert_ne!(seen[0], seen[1]);
        assert!(!player.ended());
        drop(player);
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        let _ = std::fs::remove_file(path);
    }
}
