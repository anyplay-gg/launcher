//! Thin Win32 wrappers: job objects, window detection and placement, system information.
//! Everything unsafe in the core lives here.

use std::ffi::c_void;
use std::io;
use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentThread, GetCurrentThreadId,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_CREATION_FLAGS,
    PROCESS_SET_INFORMATION, PROCESS_SUSPEND_RESUME, SetPriorityClass, SetThreadPriority,
    THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GW_OWNER, GWL_EXSTYLE,
    GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, SW_RESTORE, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, WS_EX_TOOLWINDOW,
};
use windows::core::PCWSTR;

fn io_error(error: windows::core::Error) -> io::Error {
    io::Error::other(error.message())
}

/// A job object. Processes assigned to it (and their children) can be counted and ended together.
pub struct Job(HANDLE);

// SAFETY: a job handle may be used from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// `kill_on_close`: the processes end when the last handle to the job closes (tools yes,
    /// games no).
    pub fn new(kill_on_close: bool) -> io::Result<Job> {
        // SAFETY: plain Win32 calls with valid arguments.
        unsafe {
            let handle = CreateJobObjectW(None, PCWSTR::null()).map_err(io_error)?;
            let job = Job(handle);
            if kill_on_close {
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .map_err(io_error)?;
            }
            Ok(job)
        }
    }

    /// Puts a process (its raw handle) into the job.
    pub fn assign(&self, process: *mut c_void) -> io::Result<()> {
        // SAFETY: the caller passes a live process handle.
        unsafe { AssignProcessToJobObject(self.0, HANDLE(process)).map_err(io_error) }
    }

    pub fn terminate(&self, exit_code: u32) {
        // SAFETY: valid job handle.
        unsafe {
            let _ = TerminateJobObject(self.0, exit_code);
        }
    }

    /// Number of processes still running in the job.
    pub fn active_processes(&self) -> u32 {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the buffer matches the information class.
        let ok = unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut c_void,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        };
        if ok.is_ok() { info.ActiveProcesses } else { 0 }
    }

    /// Ids of the processes in the job.
    pub fn process_ids(&self) -> Vec<u32> {
        const MAX: usize = 256;
        // A u64 buffer keeps the structure aligned.
        let words =
            (size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() + MAX * size_of::<usize>()) / 8 + 1;
        let mut buffer = vec![0u64; words];
        // SAFETY: the buffer is large and aligned enough for the header plus MAX ids.
        unsafe {
            let ok = QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr() as *mut c_void,
                (buffer.len() * 8) as u32,
                None,
            );
            if ok.is_err() {
                return Vec::new();
            }
            let list = &*(buffer.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST);
            let count = (list.NumberOfProcessIdsInList as usize).min(MAX);
            let ids = list.ProcessIdList.as_ptr();
            (0..count).map(|i| *ids.add(i) as u32).collect()
        }
    }

    /// Freezes every process of the job (all their threads) until [`resume`](Self::resume).
    /// The relinker keeps its memory but takes no CPU while the player's game starts.
    pub fn suspend(&self) {
        self.each_process(|handle| {
            // SAFETY: `handle` is an open process handle with PROCESS_SUSPEND_RESUME.
            unsafe {
                let _ = NtSuspendProcess(handle);
            }
        });
    }

    pub fn resume(&self) {
        self.each_process(|handle| {
            // SAFETY: as in `suspend`.
            unsafe {
                let _ = NtResumeProcess(handle);
            }
        });
    }

    /// The CPU priority class of every process of the job.
    pub fn set_priority(&self, class: PROCESS_CREATION_FLAGS) {
        self.each_process(|handle| {
            // SAFETY: a process handle opened for this call.
            unsafe {
                let _ = SetPriorityClass(handle, class);
            }
        });
    }

    fn each_process(&self, action: impl Fn(HANDLE)) {
        for pid in self.process_ids() {
            // SAFETY: plain Win32 calls; the handle is closed right after.
            unsafe {
                if let Ok(handle) =
                    OpenProcess(PROCESS_SUSPEND_RESUME | PROCESS_SET_INFORMATION, false, pid)
                {
                    action(handle);
                    let _ = CloseHandle(handle);
                }
            }
        }
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}

/// The priority class flag for [`Job::set_priority`].
pub fn priority_class(low: bool, idle: bool) -> PROCESS_CREATION_FLAGS {
    if idle {
        IDLE_PRIORITY_CLASS
    } else if low {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        NORMAL_PRIORITY_CLASS
    }
}

/// Puts the calling thread into the system's background mode: low CPU, I/O and memory priority,
/// so a game or the desktop is not slowed by it. Ends with the thread.
pub fn enter_background_mode() {
    // SAFETY: the pseudo handle of the calling thread is always valid.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle is owned by this value.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Search<'a> {
    pids: &'a [u32],
    found: Option<isize>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: lparam is the address of the `Search` that `find_window` keeps alive.
    let search = unsafe { &mut *(lparam.0 as *mut Search) };
    if is_game_window(hwnd, search.pids) {
        search.found = Some(hwnd.0 as isize);
        return false.into();
    }
    true.into()
}

fn is_game_window(hwnd: HWND, pids: &[u32]) -> bool {
    // SAFETY: plain queries on a window handle from EnumWindows.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !pids.contains(&pid) || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.0.is_null()) {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        rect.right - rect.left > 200 && rect.bottom - rect.top > 150
    }
}

/// The first window of one of the processes that counts as a game window (PORT.md 6.10):
/// visible, not owned, not a tool window, not cloaked, larger than 200 x 150. Returns the
/// window handle as an integer.
pub fn find_window(pids: &[u32]) -> Option<isize> {
    if pids.is_empty() {
        return None;
    }
    let mut search = Search { pids, found: None };
    // SAFETY: `search` outlives the call; the callback only runs inside it.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

fn hwnd(handle: isize) -> HWND {
    HWND(handle as *mut c_void)
}

/// D8: lets the game take the foreground right after spawn.
pub fn allow_foreground(pid: u32) {
    // SAFETY: plain call.
    unsafe {
        let _ = AllowSetForegroundWindow(pid);
    }
}

/// Brings a window to the front (restoring it when minimised) and says whether it got there.
///
/// Windows lets a process take the foreground only in a few cases (it owns the foreground, it
/// started the foreground process, it got the last input event). A gamepad press is none of them,
/// so after the plain call the foreground window's input thread is attached to this one for the
/// moment of the call: the system then treats the caller as part of the foreground input queue.
/// That is the documented `AttachThreadInput` route; it sends no key to the game, unlike the
/// Alt-key trick, and it changes no system setting.
pub fn activate_window(handle: isize) -> bool {
    let target = hwnd(handle);
    // SAFETY: plain calls on window handles; every failure is harmless.
    unsafe {
        if !IsWindow(Some(target)).as_bool() {
            return false;
        }
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        let _ = SetForegroundWindow(target);
        // The switch can lose a race with the system (a window that is just closing or
        // opening): three tries, a few milliseconds apart.
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            let front = GetForegroundWindow();
            if front == target {
                return true;
            }
            let front_thread = if front.0.is_null() {
                0
            } else {
                GetWindowThreadProcessId(front, None)
            };
            let own_thread = GetCurrentThreadId();
            let attached = front_thread != 0
                && front_thread != own_thread
                && AttachThreadInput(own_thread, front_thread, true).as_bool();
            let _ = BringWindowToTop(target);
            let _ = SetForegroundWindow(target);
            if attached {
                let _ = AttachThreadInput(own_thread, front_thread, false);
            }
        }
        GetForegroundWindow() == target
    }
}

/// The window that has the foreground (0: none) and the process that owns it.
pub fn foreground() -> (isize, u32) {
    // SAFETY: plain calls.
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return (0, 0);
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        (window.0 as isize, pid)
    }
}

/// Where D9 puts a window: `percent` (30..=100) of the work area `(left, top, right, bottom)`,
/// centred. Returns `(x, y, width, height)`.
pub fn placement(area: (i32, i32, i32, i32), percent: u32) -> (i32, i32, i32, i32) {
    let percent = percent.clamp(30, 100) as i32;
    let (left, top, right, bottom) = area;
    let width = (right - left) * percent / 100;
    let height = (bottom - top) * percent / 100;
    (
        left + (right - left - width) / 2,
        top + (bottom - top - height) / 2,
        width,
        height,
    )
}

/// D9: resizes the window to `percent` of the work area of its monitor and centres it. The move is
/// posted (`SWP_ASYNCWINDOWPOS`), so a game that does not pump messages cannot block the caller.
pub fn place_window(handle: isize, percent: u32) {
    // SAFETY: plain calls on a window handle.
    unsafe {
        let monitor = MonitorFromWindow(hwnd(handle), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let area = info.rcWork;
        let (x, y, width, height) =
            placement((area.left, area.top, area.right, area.bottom), percent);
        let _ = SetWindowPos(
            hwnd(handle),
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// D9: one F11 key press (scan code 0x57) for the game's fullscreen toggle.
pub fn send_f11(handle: isize) {
    fn key(flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: 0x57,
                    dwFlags: flags | KEYEVENTF_SCANCODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    activate_window(handle);
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    // SAFETY: a valid INPUT array.
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::w;

    /// The tests share the process, so they must not see each other's windows.
    static WINDOWS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_window(width: i32, height: i32, visible: bool) -> Option<HWND> {
        let style = if visible {
            WS_POPUP | WS_VISIBLE
        } else {
            WS_POPUP
        };
        // SAFETY: creates a window of the predefined STATIC class on this thread.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("anyps5-core test window"),
                style,
                10,
                10,
                width,
                height,
                None,
                None,
                None,
                None,
            )
            .ok()
        }
    }

    #[test]
    fn window_rules_size_and_visibility() {
        let _serial = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: plain query.
        let pid = unsafe { GetCurrentProcessId() };
        let Some(big) = make_window(400, 300, true) else {
            eprintln!("no desktop available, skipping");
            return;
        };
        assert_eq!(find_window(&[pid]), Some(big.0 as isize));
        assert_eq!(find_window(&[pid.wrapping_add(1)]), None);
        // SAFETY: destroying our own window.
        unsafe {
            let _ = DestroyWindow(big);
        }
        let small = make_window(100, 100, true).unwrap();
        assert_eq!(find_window(&[pid]), None);
        unsafe {
            let _ = DestroyWindow(small);
        }
        let hidden = make_window(400, 300, false).unwrap();
        assert_eq!(find_window(&[pid]), None);
        unsafe {
            let _ = DestroyWindow(hidden);
        }
    }

    #[test]
    fn placement_centres_a_share_of_the_work_area() {
        // 1920x1040 work area below a 40 px taskbar at the top.
        assert_eq!(placement((0, 40, 1920, 1080), 50), (480, 300, 960, 520));
        assert_eq!(placement((0, 0, 1000, 1000), 100), (0, 0, 1000, 1000));
        // Out of range values are clamped to 30..=100.
        assert_eq!(
            placement((0, 0, 1000, 1000), 5),
            placement((0, 0, 1000, 1000), 30)
        );
        assert_eq!(placement((0, 0, 1000, 1000), 500), (0, 0, 1000, 1000));
        // A second monitor to the left (negative coordinates).
        assert_eq!(placement((-1920, 0, 0, 1080), 75), (-1680, 135, 1440, 810));
    }

    #[test]
    fn placing_a_window_that_never_pumps_messages_does_not_block() {
        let _serial = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = make_window(400, 300, true) else {
            return;
        };
        let started = std::time::Instant::now();
        place_window(window.0 as isize, 50);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        unsafe {
            let _ = DestroyWindow(window);
        }
    }

    #[test]
    fn job_counts_and_ends_processes() {
        let job = Job::new(true).unwrap();
        assert_eq!(job.active_processes(), 0);
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "ping -n 30 127.0.0.1 > nul"])
            .spawn()
            .unwrap();
        use std::os::windows::io::AsRawHandle;
        job.assign(child.as_raw_handle()).unwrap();
        assert!(job.active_processes() >= 1);
        assert!(job.process_ids().contains(&child.id()));
        job.terminate(7);
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(job.active_processes(), 0);
    }
}
