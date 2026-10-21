//! A Windows Job Object per run, so Close (or a crash of Chorus) kills the agent's whole process
//! tree (D13).
//!
//! The job is created with `KILL_ON_JOB_CLOSE`: dropping the last handle, including when the
//! Chorus process dies, terminates every process in it. Children an agent starts join the job
//! automatically.
//!
//! Two refinements from spike S1, decided in `chorus_core::tree`:
//! - **Close spares windows.** A browser an agent opened (a sign-in flow) is the agent's child and in
//!   its job; Close leaves processes with visible windows, and what runs under them, alive. A crash
//!   of Chorus still takes everything down (LESSONS L22).
//! - **Strays are adopted.** portable-pty can't put the child in a job at `CreateProcess` time, so
//!   the child is assigned a moment after it starts. Right after assigning, any process it started
//!   in that moment is found by parent id and assigned too (D41 records why this isn't atomic).

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::time::{Duration, Instant};

use chorus_core::tree::{Proc, spared_on_close, strays};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible};
use windows::core::{BOOL, PCWSTR};

pub struct Job(HANDLE);

// The handle is an opaque kernel object reference; Windows allows using it from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

/// What Close did.
#[derive(Debug, Default, Clone)]
pub struct CloseOutcome {
    pub killed: Vec<u32>,
    pub spared: Vec<u32>,
}

impl Job {
    pub fn new() -> windows::core::Result<Self> {
        let job = Job(
            // SAFETY: plain Win32 call; the handle is owned by the returned Job and closed on drop.
            unsafe { CreateJobObjectW(None, PCWSTR::null())? },
        );
        job.set_kill_on_close(true)?;
        Ok(job)
    }

    fn set_kill_on_close(&self, on: bool) -> windows::core::Result<()> {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if on {
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        // SAFETY: `info` is a correctly sized, initialised structure for this information class.
        unsafe {
            SetInformationJobObject(
                self.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
    }

    /// Puts a running process (and its future children) in the job.
    pub fn assign(&self, process: HANDLE) -> windows::core::Result<()> {
        // SAFETY: `process` is a live process handle owned by the caller for the duration.
        unsafe { AssignProcessToJobObject(self.0, process) }
    }

    fn assign_pid(&self, pid: u32) -> windows::core::Result<()> {
        let process = OwnedProcess::open(pid, PROCESS_SET_QUOTA | PROCESS_TERMINATE)?;
        self.assign(process.0)
    }

    /// Assigns any process `root` started before it joined the job. Returns the ids adopted.
    pub fn adopt_strays(&self, root: u32) -> Vec<u32> {
        let Ok(in_job) = self.process_ids() else { return Vec::new() };
        let in_job: HashSet<u32> = in_job.into_iter().collect();
        let found = strays(root, &snapshot(), &in_job);
        found.into_iter().filter(|pid| self.assign_pid(*pid).is_ok()).collect()
    }

    /// The ids of every process currently in the job.
    pub fn process_ids(&self) -> windows::core::Result<Vec<u32>> {
        // Room for 1024 ids, far beyond any agent tree.
        const CAP: usize = 1024;
        #[repr(C)]
        struct List {
            head: JOBOBJECT_BASIC_PROCESS_ID_LIST,
            rest: [usize; CAP - 1],
        }
        let mut list = List { head: JOBOBJECT_BASIC_PROCESS_ID_LIST::default(), rest: [0; CAP - 1] };
        // SAFETY: the buffer is sized for CAP ids and Windows writes at most that many.
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicProcessIdList,
                (&raw mut list).cast::<c_void>(),
                size_of::<List>() as u32,
                None,
            )?;
            let n = (list.head.NumberOfProcessIdsInList as usize).min(CAP);
            let ids = std::slice::from_raw_parts(list.head.ProcessIdList.as_ptr(), n);
            Ok(ids.iter().map(|&id| id as u32).collect())
        }
    }

    /// Kills every process in the job now.
    pub fn terminate(&self, exit_code: u32) -> windows::core::Result<()> {
        // SAFETY: valid job handle.
        unsafe { TerminateJobObject(self.0, exit_code) }
    }

    /// Ends the session's processes but leaves windowed ones (and what runs under them) alive.
    /// After this the job no longer kills on close, so dropping it leaves the spared ones running.
    pub fn close_sparing_windows(&self, root: u32, exit_code: u32) -> windows::core::Result<CloseOutcome> {
        let job = self.process_ids()?;
        let spared = spared_on_close(root, &job, &snapshot(), &pids_with_windows());
        if spared.is_empty() {
            self.terminate(exit_code)?;
            return Ok(CloseOutcome { killed: job, spared: Vec::new() });
        }
        self.set_kill_on_close(false)?;
        let mut killed = Vec::new();
        // A few passes catch processes started while the first pass ran.
        for _ in 0..3 {
            let targets: Vec<u32> = self.process_ids()?.into_iter().filter(|p| !spared.contains(p)).collect();
            if targets.is_empty() {
                break;
            }
            for pid in targets {
                if let Ok(process) = OwnedProcess::open(pid, PROCESS_TERMINATE) {
                    // SAFETY: a live handle opened with PROCESS_TERMINATE.
                    if unsafe { TerminateProcess(process.0, exit_code) }.is_ok() && !killed.contains(&pid) {
                        killed.push(pid);
                    }
                }
            }
        }
        Ok(CloseOutcome { killed, spared: spared.into_iter().collect() })
    }

    /// Waits up to `timeout` for these processes of the job to be gone. Ending a process only
    /// starts its exit, and until it's gone the folder it worked in can't be deleted. True if they
    /// all are.
    pub fn wait_gone(&self, pids: &[u32], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        pids.iter().all(|&pid| {
            // An id that can't be opened is gone; one outside the job is another process's now.
            let Ok(process) = OwnedProcess::open(pid, PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION) else {
                return true;
            };
            let mut in_job = BOOL(0);
            // SAFETY: live process and job handles, and a valid out-pointer.
            if unsafe { IsProcessInJob(process.0, Some(self.0), &mut in_job) }.is_err() || !in_job.as_bool() {
                return true;
            }
            let ms = deadline.saturating_duration_since(Instant::now()).as_millis() as u32;
            // SAFETY: a live handle opened with SYNCHRONIZE.
            let waited = unsafe { WaitForSingleObject(process.0, ms) };
            waited == WAIT_OBJECT_0
        })
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle. With KILL_ON_JOB_CLOSE set, this ends any process still in it.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct OwnedProcess(HANDLE);

impl OwnedProcess {
    fn open(pid: u32, access: windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS) -> windows::core::Result<Self> {
        // SAFETY: plain Win32 call; the handle is closed on drop.
        unsafe { OpenProcess(access, false, pid).map(Self) }
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        // SAFETY: we own the handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Every process on the machine with its parent and creation time. Processes that can't be
/// opened (other users', protected ones) get creation time 0, which only ever excludes them.
fn snapshot() -> Vec<Proc> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed below; the entry struct is initialised with its size.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            out.push(Proc { pid: entry.th32ProcessID, ppid: entry.th32ParentProcessID, created: created(entry.th32ProcessID) });
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn created(pid: u32) -> u64 {
    let Ok(process) = OwnedProcess::open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else { return 0 };
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: a live handle with query rights and four valid out-pointers.
    if unsafe { GetProcessTimes(process.0, &mut c, &mut e, &mut k, &mut u) }.is_err() {
        return 0;
    }
    (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime)
}

/// Processes that own a visible, unowned top-level window.
fn pids_with_windows() -> HashSet<u32> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `&mut HashSet` passed to EnumWindows below, alive for the call.
        let set = unsafe { &mut *(lparam.0 as *mut HashSet<u32>) };
        // SAFETY: plain queries on a window handle Windows just gave us.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 {
                    set.insert(pid);
                }
            }
        }
        BOOL(1)
    }
    let mut set = HashSet::new();
    // SAFETY: the callback only touches `set` through the pointer, during this call.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut set) as isize));
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[test]
    fn an_ended_process_is_waited_for_until_gone() {
        let job = Job::new().unwrap();
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 30 127.0.0.1 >nul"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        job.assign(HANDLE(child.as_raw_handle())).unwrap();
        let pid = child.id();
        assert!(!job.wait_gone(&[pid], Duration::from_millis(200)), "it's still running");
        job.terminate(1).unwrap();
        assert!(job.wait_gone(&[pid], Duration::from_secs(5)));
        // A process outside the job isn't waited for, whatever its id was.
        let t0 = Instant::now();
        assert!(job.wait_gone(&[std::process::id()], Duration::from_secs(5)));
        assert!(t0.elapsed() < Duration::from_secs(1));
        let _ = child.wait();
    }
}
