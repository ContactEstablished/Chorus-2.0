//! A Windows Job Object per session, so Close (or a crash of Chorus) kills the agent's whole
//! process tree (D13).
//!
//! The job is created with `KILL_ON_JOB_CLOSE`: dropping the last handle, including when the
//! Chorus process dies, terminates every process in it. Children an agent starts after it was
//! assigned join the job automatically.
//!
//! Known gap (S1): portable-pty doesn't expose `CreateProcess` flags, so the child is assigned just
//! after it starts rather than atomically. A grandchild spawned in that window (microseconds,
//! before the agent has run any code of its own) would escape. Closing it needs
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST` at spawn time.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows::core::PCWSTR;

pub struct Job(HANDLE);

// The handle is an opaque kernel object reference; Windows allows using it from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> windows::core::Result<Self> {
        // SAFETY: plain Win32 calls; the handle is owned by the returned Job and closed on drop.
        unsafe {
            let handle = CreateJobObjectW(None, PCWSTR::null())?;
            let job = Job(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(job)
        }
    }

    /// Puts a running process (and its future children) in the job.
    pub fn assign(&self, process: HANDLE) -> windows::core::Result<()> {
        // SAFETY: `process` is a live process handle owned by the caller for the duration.
        unsafe { AssignProcessToJobObject(self.0, process) }
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
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: we own the handle. KILL_ON_JOB_CLOSE ends any process still in the job.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
