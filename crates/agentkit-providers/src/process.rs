use anyhow::{bail, Context, Result};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};

pub struct Process {
    pub child: Child,
    pub stderr: JoinHandle<String>,
    #[cfg(unix)]
    group: i32,
    #[cfg(windows)]
    job: usize,
}
impl Process {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        #[cfg(windows)]
        command.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        let mut child = command
            .spawn()
            .context("Unable to start provider executable")?;
        #[cfg(unix)]
        let group = child.id().context("Provider process has no ID")? as i32;
        #[cfg(windows)]
        let job = attach_job(&child)?;
        let mut stderr = child.stderr.take().context("Provider stderr missing")?;
        let stderr = tokio::spawn(async move {
            let mut tail = std::collections::VecDeque::with_capacity(16 * 1024);
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        for b in &buf[..n] {
                            if tail.len() == 16 * 1024 {
                                tail.pop_front();
                            }
                            tail.push_back(*b);
                        }
                    }
                }
            }
            String::from_utf8_lossy(&tail.into_iter().collect::<Vec<_>>()).into_owned()
        });
        Ok(Self {
            child,
            stderr,
            #[cfg(unix)]
            group,
            #[cfg(windows)]
            job,
        })
    }
    pub async fn finish(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-self.group, libc::SIGTERM);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job as _, 1);
        }
        if tokio::time::timeout(std::time::Duration::from_millis(800), self.child.wait())
            .await
            .is_err()
        {
            #[cfg(unix)]
            unsafe {
                libc::kill(-self.group, libc::SIGKILL);
            }
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        // Reap descendants that ignored SIGTERM even if the immediate process exited.
        #[cfg(unix)]
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-self.group, libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job as _);
        }
        self.stderr.abort();
    }
}

#[cfg(windows)]
fn attach_job(child: &Child) -> Result<usize> {
    use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as _,
            std::mem::size_of_val(&info) as u32,
        ) == 0
            || AssignProcessToJobObject(
                job,
                child.raw_handle().context("Missing process handle")? as _,
            ) == 0
        {
            let error = std::io::Error::last_os_error();
            CloseHandle(job);
            return Err(error.into());
        }
        Ok(job as usize)
    }
}

pub async fn line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    const LIMIT: usize = 8 * 1024 * 1024;
    let mut data = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if data.is_empty() {
                return Ok(None);
            } else {
                break;
            }
        }
        let n = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if data.len() + n > LIMIT {
            bail!("Provider event exceeds 8 MiB limit");
        }
        let done = available[n - 1] == b'\n';
        data.extend_from_slice(&available[..n]);
        reader.consume(n);
        if done {
            break;
        }
    }
    Ok(Some(
        String::from_utf8(data).context("Provider output is not UTF-8")?,
    ))
}
pub fn reader(child: &mut Child) -> Result<BufReader<tokio::process::ChildStdout>> {
    Ok(BufReader::new(
        child.stdout.take().context("Provider stdout missing")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounded_lines_preserve_unicode_and_final_line() {
        let mut r = BufReader::new("فارسی\nlast".as_bytes());
        assert_eq!(line(&mut r).await.unwrap().unwrap(), "فارسی\n");
        assert_eq!(line(&mut r).await.unwrap().unwrap(), "last");
        assert!(line(&mut r).await.unwrap().is_none());
    }
}
