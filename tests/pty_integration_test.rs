use std::io::Read;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

#[test]
fn test_pty_spawn_echo() {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("Failed to open PTY");

    // Clone reader BEFORE spawning so we have it ready
    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("Failed to clone reader");

    let mut cmd = CommandBuilder::new("echo");
    cmd.arg("hello from pty");

    let mut child = pair.slave.spawn_command(cmd).expect("Failed to spawn");
    drop(pair.slave);

    // Read output — reader must be obtained before child finishes
    let mut output = Vec::new();
    let mut buf = [0u8; 1024];
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => output.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
        if output.windows(14).any(|w| w == b"hello from pty") {
            break;
        }
    }

    let _ = child.wait();

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("hello from pty"),
        "Expected 'hello from pty' in output, got: {:?}",
        output_str
    );
}

#[test]
fn test_pty_vt100_captures_output() {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("Failed to open PTY");

    // Clone reader BEFORE spawning
    let mut reader = pair
        .master
        .try_clone_reader()
        .expect("Failed to clone reader");

    let mut cmd = CommandBuilder::new("printf");
    cmd.arg("Line1\\nLine2\\nLine3");

    let mut child = pair.slave.spawn_command(cmd).expect("Failed to spawn");
    drop(pair.slave);

    let mut parser = vt100::Parser::new(24, 80, 0);
    let mut buf = [0u8; 4096];
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(3) {
            break;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => parser.process(&buf[..n]),
            Err(_) => break,
        }
        let contents = parser.screen().contents();
        if contents.contains("Line1") {
            break;
        }
    }

    let _ = child.wait();

    let contents = parser.screen().contents();
    assert!(
        contents.contains("Line1"),
        "Expected 'Line1' in screen contents: {:?}",
        contents
    );
}
