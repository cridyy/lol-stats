use regex::Regex;
use sysinfo::System;

use super::error::{AppError, AppResult};
use super::models::ClientAuth;

const UX_PROCESS_NAME: &str = "LeagueClientUx.exe";

/// 扫描正在运行的 LeagueClientUx.exe，并解析 LCU/RC 连接参数。
///
/// LeagueAkari 使用 `@leagueakari/league-akari-addons` 读取进程命令行。本项目是
/// Tauri + Rust，因此在 Rust 侧实现同等能力：先尝试 sysinfo 暴露的命令行，
/// 再通过 Windows 原生 API 读取目标进程的原始 CommandLine。
pub fn discover_clients() -> AppResult<Vec<ClientAuth>> {
    let system = System::new_all();
    let mut clients = Vec::new();
    let mut saw_ux = false;

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy();
        if !name.eq_ignore_ascii_case(UX_PROCESS_NAME) {
            continue;
        }

        saw_ux = true;
        let pid_u32 = pid.as_u32();
        let from_sysinfo = process
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");

        let command_line = if from_sysinfo.trim().is_empty() {
            read_process_command_line(pid_u32).unwrap_or_default()
        } else {
            from_sysinfo
        };

        if let Some(auth) = parse_ux_command_line(&command_line, pid_u32) {
            clients.push(auth);
        }
    }

    if !clients.is_empty() {
        return Ok(clients);
    }

    if saw_ux {
        Err(AppError::ClientAuthNotFound)
    } else {
        Err(AppError::ClientNotFound)
    }
}

/// 返回第一个可用客户端。国服通常只会有一个 League Client 实例。
pub fn discover_primary_client() -> AppResult<ClientAuth> {
    discover_clients()?
        .into_iter()
        .next()
        .ok_or(AppError::ClientAuthNotFound)
}

fn capture(regex: &Regex, text: &str) -> Option<String> {
    regex
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

/// 解析 LeagueClientUx.exe 命令行。
///
/// 正则字段参考 LeagueAkari 的 `src/main/utils/ux-cmd.ts`。`--app-pid` 理论上存在，
/// 但这里仍然接受外层枚举到的 pid 作为兜底。
pub fn parse_ux_command_line(command_line: &str, fallback_pid: u32) -> Option<ClientAuth> {
    let port_re = Regex::new(r"--app-port=([0-9]+)").ok()?;
    let auth_re = Regex::new(r"--remoting-auth-token=([A-Za-z0-9_-]+)").ok()?;
    let pid_re = Regex::new(r"--app-pid=([0-9]+)").ok()?;
    let region_re = Regex::new(r"--region=([A-Za-z0-9_-]+)").ok()?;
    let rso_re = Regex::new(r"--rso_platform_id=([A-Za-z0-9_-]+)").ok()?;
    let riot_port_re = Regex::new(r"--riotclient-app-port=([0-9]+)").ok()?;
    let riot_auth_re = Regex::new(r"--riotclient-auth-token=([A-Za-z0-9_-]+)").ok()?;

    let port = capture(&port_re, command_line)?.parse::<u16>().ok()?;
    let auth_token = capture(&auth_re, command_line)?;
    let pid = capture(&pid_re, command_line)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(fallback_pid);

    Some(ClientAuth {
        port,
        pid,
        auth_token,
        region: capture(&region_re, command_line).unwrap_or_default(),
        rso_platform_id: capture(&rso_re, command_line).unwrap_or_default(),
        riot_client_port: capture(&riot_port_re, command_line).and_then(|s| s.parse().ok()),
        riot_client_auth_token: capture(&riot_auth_re, command_line),
        source: "process-command-line".to_string(),
    })
}

#[cfg(target_os = "windows")]
fn read_process_command_line(pid: u32) -> AppResult<String> {
    windows_command_line::read_process_command_line(pid)
}

#[cfg(not(target_os = "windows"))]
fn read_process_command_line(_pid: u32) -> AppResult<String> {
    Err(AppError::Windows("当前仅支持 Windows 客户端".to_string()))
}

#[cfg(target_os = "windows")]
mod windows_command_line {
    use std::ffi::c_void;
    use std::mem::{size_of, MaybeUninit};
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    use crate::services::error::{AppError, AppResult};

    const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;
    const PROCESS_COMMAND_LINE_INFORMATION_CLASS: u32 = 60;

    #[repr(C)]
    struct ProcessBasicInformation {
        reserved1: *mut c_void,
        peb_base_address: *mut Peb,
        reserved2: [*mut c_void; 2],
        unique_process_id: usize,
        reserved3: *mut c_void,
    }

    #[repr(C)]
    struct Peb {
        reserved1: [u8; 2],
        being_debugged: u8,
        reserved2: [u8; 1],
        reserved3: [*mut c_void; 2],
        ldr: *mut c_void,
        process_parameters: *mut RtlUserProcessParameters,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }

    #[repr(C)]
    struct RtlUserProcessParameters {
        reserved1: [u8; 16],
        reserved2: [*mut c_void; 10],
        image_path_name: UnicodeString,
        command_line: UnicodeString,
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            process_handle: HANDLE,
            process_information_class: u32,
            process_information: *mut c_void,
            process_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    struct ProcessHandle(HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            if self.0 != null_mut() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    /// 读取目标进程命令行。
    ///
    /// 国服客户端常见情况下会拒绝 `PROCESS_VM_READ`，因此这里优先使用
    /// `ProcessCommandLineInformation`。这个调用只需要
    /// `PROCESS_QUERY_LIMITED_INFORMATION`，和 `rank-analysis` 的做法一致；如果系统
    /// 或客户端环境不支持，再回退到传统 PEB + `ReadProcessMemory`。
    pub fn read_process_command_line(pid: u32) -> AppResult<String> {
        match read_command_line_via_process_info(pid) {
            Ok(line) if !line.trim().is_empty() => Ok(line),
            Ok(_) => read_command_line_via_peb(pid),
            Err(primary_error) => match read_command_line_via_peb(pid) {
                Ok(line) => Ok(line),
                Err(fallback_error) => Err(AppError::Windows(format!(
                    "{primary_error}; PEB 兜底失败：{fallback_error}"
                ))),
            },
        }
    }

    fn read_command_line_via_process_info(pid: u32) -> AppResult<String> {
        let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

        if raw == null_mut() {
            return Err(AppError::Windows(format!(
                "OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) 失败，pid={pid}，{}",
                std::io::Error::last_os_error()
            )));
        }

        let handle = ProcessHandle(raw);
        let mut buffer = vec![0u8; 8192];
        let mut returned = 0u32;
        let mut status = unsafe {
            NtQueryInformationProcess(
                handle.0,
                PROCESS_COMMAND_LINE_INFORMATION_CLASS,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len() as u32,
                &mut returned,
            )
        };

        if status != 0 {
            if returned as usize > buffer.len() {
                buffer.resize(returned as usize, 0);
                status = unsafe {
                    NtQueryInformationProcess(
                        handle.0,
                        PROCESS_COMMAND_LINE_INFORMATION_CLASS,
                        buffer.as_mut_ptr() as *mut c_void,
                        buffer.len() as u32,
                        &mut returned,
                    )
                };
            }

            if status != 0 {
                return Err(AppError::Windows(format!(
                    "NtQueryInformationProcess(ProcessCommandLineInformation) 失败，status=0x{:08x}",
                    status as u32
                )));
            }
        }

        if returned == 0 {
            return Err(AppError::Windows(
                "ProcessCommandLineInformation 返回空缓冲区".to_string(),
            ));
        }

        read_command_line_from_local_buffer(&buffer)
    }

    fn read_command_line_via_peb(pid: u32) -> AppResult<String> {
        let raw =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };

        if raw == null_mut() {
            return Err(AppError::Windows(format!(
                "OpenProcess(PROCESS_VM_READ) 失败，pid={pid}，{}",
                std::io::Error::last_os_error()
            )));
        }

        let handle = ProcessHandle(raw);
        let pbi = query_process_basic_info(handle.0)?;
        let peb: Peb = read_struct(handle.0, pbi.peb_base_address as usize)?;
        let params: RtlUserProcessParameters =
            read_struct(handle.0, peb.process_parameters as usize)?;

        read_unicode_string(handle.0, params.command_line)
    }

    fn read_command_line_from_local_buffer(buffer: &[u8]) -> AppResult<String> {
        if buffer.len() < size_of::<UnicodeString>() {
            return Err(AppError::Windows(
                "ProcessCommandLineInformation 缓冲区过短".to_string(),
            ));
        }

        let unicode = unsafe { *(buffer.as_ptr() as *const UnicodeString) };
        if unicode.length == 0 || unicode.buffer.is_null() {
            return Ok(String::new());
        }

        let start = buffer.as_ptr() as usize;
        let end = start + buffer.len();
        let text_start = unicode.buffer as usize;
        let text_end = text_start.saturating_add(unicode.length as usize);

        if text_start < start || text_end > end {
            return Err(AppError::Windows(
                "ProcessCommandLineInformation 返回了非本地缓冲区指针".to_string(),
            ));
        }

        let units = unicode.length as usize / 2;
        let text = unsafe { std::slice::from_raw_parts(unicode.buffer, units) };
        Ok(String::from_utf16_lossy(text))
    }

    fn query_process_basic_info(handle: HANDLE) -> AppResult<ProcessBasicInformation> {
        let mut info = MaybeUninit::<ProcessBasicInformation>::zeroed();
        let mut returned = 0u32;
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                PROCESS_BASIC_INFORMATION_CLASS,
                info.as_mut_ptr() as *mut c_void,
                size_of::<ProcessBasicInformation>() as u32,
                &mut returned,
            )
        };

        if status < 0 {
            return Err(AppError::Windows(format!(
                "NtQueryInformationProcess 失败，status={status}"
            )));
        }

        Ok(unsafe { info.assume_init() })
    }

    fn read_struct<T>(handle: HANDLE, address: usize) -> AppResult<T> {
        if address == 0 {
            return Err(AppError::Windows("目标进程地址为空".to_string()));
        }

        let mut value = MaybeUninit::<T>::uninit();
        let mut bytes_read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                handle,
                address as *const c_void,
                value.as_mut_ptr() as *mut c_void,
                size_of::<T>(),
                &mut bytes_read,
            )
        };

        if ok == 0 || bytes_read != size_of::<T>() {
            return Err(AppError::Windows(
                "ReadProcessMemory 读取结构失败".to_string(),
            ));
        }

        Ok(unsafe { value.assume_init() })
    }

    fn read_unicode_string(handle: HANDLE, unicode: UnicodeString) -> AppResult<String> {
        if unicode.length == 0 || unicode.buffer.is_null() {
            return Ok(String::new());
        }

        let units = unicode.length as usize / 2;
        let mut buffer = vec![0u16; units];
        let mut bytes_read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                handle,
                unicode.buffer as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                unicode.length as usize,
                &mut bytes_read,
            )
        };

        if ok == 0 {
            return Err(AppError::Windows(
                "ReadProcessMemory 读取命令行失败".to_string(),
            ));
        }

        Ok(String::from_utf16_lossy(&buffer))
    }
}
