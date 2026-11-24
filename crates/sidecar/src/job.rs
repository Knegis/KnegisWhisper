//! Keeping Windows model processes within the lifetime of the application.
//!
//! One process-wide Job Object has only `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
//! set. Its non-inheritable handle is kept open for the life of this process;
//! Windows closes it on any exit, including a crash or forced termination, and
//! kills the children in the job. No destructor or exit cleanup is involved.
//! Only children join: putting the application in the job would also capture
//! Explorer and browsers it opens, which must survive Quit.
//!
//! Assignment failure is reported to the caller to warn and carry on: a model
//! can still run, and PID-based reaping remains the fallback. The short window
//! between spawn and assignment is not covered. Neither are macOS and Linux,
//! where assignment is a no-op. No UI or other limits are set, so this plain
//! job can nest inside a job already containing the application.

/// Why a child could not be tied to the application's lifetime.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("could not create the process job: {reason}")]
    Create { reason: String },

    #[error("could not assign the child to the process job: {reason}")]
    Assign { reason: String },
}

/// Assign a child to the process-wide kill-on-close job on Windows.
///
/// Other platforms leave the child alone and return success.
pub fn assign(child: &tokio::process::Child) -> Result<(), JobError> {
    #[cfg(windows)]
    {
        windows_job::assign(child)
    }
    #[cfg(not(windows))]
    {
        let _ = child;
        Ok(())
    }
}

#[cfg(windows)]
mod windows_job {
    use std::sync::OnceLock;

    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    use super::JobError;

    #[derive(Debug)]
    struct Job(HANDLE);

    // SAFETY: A kernel job handle has no thread affinity. It is written once
    // during initialization, then only read, and is never closed by this program.
    unsafe impl Send for Job {}
    // SAFETY: Concurrent assignment only reads this immutable kernel handle;
    // Windows synchronizes operations on the job itself.
    unsafe impl Sync for Job {}

    // Remember failures too: repeatedly asking Windows cannot repair a failed
    // initialization, and every caller still needs to know it is unprotected.
    static JOB: OnceLock<Result<Job, String>> = OnceLock::new();

    fn create() -> Result<Job, String> {
        // SAFETY: No name or security attributes are passed. The returned handle
        // is owned here and, with default security attributes, is not inheritable.
        let handle = unsafe { CreateJobObjectW(None, None) }.map_err(|err| err.to_string())?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: The handle is a live job and the pointer and size describe the
        // initialized limit structure, which remains alive throughout the call.
        let result = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
        };
        if let Err(err) = result {
            // SAFETY: Initialization failed before any child was assigned. We
            // exclusively own this handle and close it exactly once here.
            let _ = unsafe { CloseHandle(handle) };
            return Err(err.to_string());
        }
        // Intentionally no Drop: only process termination closes this handle.
        Ok(Job(handle))
    }

    pub(super) fn assign(child: &tokio::process::Child) -> Result<(), JobError> {
        let job = JOB
            .get_or_init(create)
            .as_ref()
            .map_err(|reason| JobError::Create {
                reason: reason.clone(),
            })?;
        let process = child.raw_handle().ok_or_else(|| JobError::Assign {
            reason: "the child has no process handle; it may already have been reaped".to_owned(),
        })?;
        // SAFETY: The job handle is kept open for this process's lifetime. The
        // borrowed child owns its process handle for the duration of this call.
        unsafe { AssignProcessToJobObject(job.0, HANDLE(process)) }.map_err(|err| {
            JobError::Assign {
                reason: err.to_string(),
            }
        })
    }
}
