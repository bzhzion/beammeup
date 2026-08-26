//! Remote file operations (SFTP equivalent), on the same principle as the existing SSH sessions:
//! the system client (`ssh`), never a dedicated SSH library nor a stored secret (authentication
//! comes from the SSH agent/keys already configured on the machine). Unlike interactive sessions,
//! these operations are one-off and windowless: listing, reading, writing, renaming, deleting, or
//! creating a remote folder does not warrant a visible tab.
//!
//! `target` reuses exactly the format already used by `beammeup open --ssh` (e.g.
//! "kao@axolotl -i C:\path\key"), passed as-is to `ssh` after splitting on whitespace.

use std::io::Write as _;
use std::process::{Command, Stdio};

/// Simple POSIX escaping (single quotes): good enough for file paths, which normally do not
/// themselves contain a single quote.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn run(target: &str, remote_cmd: &str) -> Result<(Vec<u8>, String, i32), String> {
    let mut cmd = Command::new("ssh");
    for part in target.split_whitespace() {
        cmd.arg(part);
    }
    cmd.arg("--").arg(remote_cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .map_err(|e| format!("failed to launch ssh: {e}"))?;
    Ok((
        output.stdout,
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    ))
}

pub fn list(target: &str, path: &str) -> Result<String, String> {
    let (out, err, code) = run(target, &format!("ls -la -- {}", quote(path)))?;
    if code == 0 {
        Ok(String::from_utf8_lossy(&out).to_string())
    } else {
        Err(if err.is_empty() {
            format!("exit code {code}")
        } else {
            err
        })
    }
}

pub fn mkdir(target: &str, path: &str) -> Result<(), String> {
    let (_, err, code) = run(target, &format!("mkdir -p -- {}", quote(path)))?;
    if code == 0 {
        Ok(())
    } else {
        Err(err)
    }
}

pub fn delete(target: &str, path: &str) -> Result<(), String> {
    let (_, err, code) = run(target, &format!("rm -f -- {}", quote(path)))?;
    if code == 0 {
        Ok(())
    } else {
        Err(err)
    }
}

pub fn rename(target: &str, from: &str, to: &str) -> Result<(), String> {
    let (_, err, code) = run(target, &format!("mv -- {} {}", quote(from), quote(to)))?;
    if code == 0 {
        Ok(())
    } else {
        Err(err)
    }
}

/// Raw content (no text decoding): the remote file's bytes travel without going through a PTY, so
/// without the encoding quirks encountered elsewhere on interactive sessions.
pub fn read(target: &str, path: &str) -> Result<Vec<u8>, String> {
    let (out, err, code) = run(target, &format!("cat -- {}", quote(path)))?;
    if code == 0 {
        Ok(out)
    } else {
        Err(if err.is_empty() {
            format!("exit code {code}")
        } else {
            err
        })
    }
}

pub fn write(target: &str, path: &str, content: &[u8]) -> Result<(), String> {
    let mut cmd = Command::new("ssh");
    for part in target.split_whitespace() {
        cmd.arg(part);
    }
    cmd.arg("--").arg(format!("cat > {}", quote(path)));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to launch ssh: {e}"))?;
    // stdin is closed (dropped) at the end of this block, signaling the EOF that `cat` waits for
    // to finish and let `wait_with_output` collect a complete output.
    {
        let mut stdin = child.stdin.take().expect("stdin piped");
        stdin
            .write_all(content)
            .map_err(|e| format!("failed to write to stdin: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for ssh: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
