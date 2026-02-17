use anyhow::{Result, bail};
use log::{info, warn};
use std::ptr;
use widestring::U16CString;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Services::*;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY, SC_HANDLE};

const SERVICE_NAME: &str = "Autolocalhost";
const SERVICE_DISPLAY_NAME: &str = "Autolocalhost Service";
const SERVICE_DESCRIPTION: &str = "Local development environment automation service";

pub async fn is_service_running() -> Result<bool> {
    let manager = open_service_manager()?;
    let service = match open_service(&manager, SERVICE_NAME) {
        Ok(svc) => svc,
        Err(_) => return Ok(false), // Service doesn't exist
    };

    let mut status = SERVICE_STATUS::default();
    unsafe {
        QueryServiceStatus(service, &mut status)?;
    }

    Ok(status.dwCurrentState == SERVICE_RUNNING)
}

pub async fn stop_service() -> Result<()> {
    let manager = open_service_manager()?;
    let service = open_service(&manager, SERVICE_NAME)?;

    let mut status = SERVICE_STATUS::default();
    unsafe {
        match ControlService(service, SERVICE_CONTROL_STOP, &mut status) {
            Ok(_) => {
                info!("Service stopped successfully");
            }
            Err(e) => {
                warn!("Failed to stop service: {:?}", e);
            }
        }
    }

    Ok(())
}

pub async fn install_service() -> Result<()> {
    let manager = open_service_manager()?;

    let exe_path = crate::installer::get_install_dir().join("autolocalhost.exe");
    let exe_path_str = exe_path.to_string_lossy();
    let command_line = format!("\"{}\" start", exe_path_str);

    let service_name = U16CString::from_str(SERVICE_NAME)?;
    let display_name = U16CString::from_str(SERVICE_DISPLAY_NAME)?;
    let command_line_wide = U16CString::from_str(&command_line)?;

    let service = unsafe {
        CreateServiceW(
            manager,
            PCWSTR(service_name.as_ptr()),
            PCWSTR(display_name.as_ptr()),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            PCWSTR(command_line_wide.as_ptr()),
            None,
            None,
            None,
            None,
            None,
        )?
    };

    // Set service description
    let description = U16CString::from_str(SERVICE_DESCRIPTION)?;
    let mut service_desc = SERVICE_DESCRIPTIONW {
        lpDescription: PWSTR(description.as_ptr() as *mut u16),
    };

    unsafe {
        let _ = ChangeServiceConfig2W(
            service,
            SERVICE_CONFIG_DESCRIPTION,
            Some(&mut service_desc as *mut _ as *mut _),
        );
    }

    info!("Windows service installed successfully");
    Ok(())
}

pub async fn uninstall_service() -> Result<()> {
    let manager = open_service_manager()?;

    let service = match open_service(&manager, SERVICE_NAME) {
        Ok(svc) => svc,
        Err(_) => {
            info!("Service not found, nothing to uninstall");
            return Ok(());
        }
    };

    unsafe {
        match DeleteService(service) {
            Ok(_) => {
                info!("Service uninstalled successfully");
            }
            Err(e) => {
                warn!("Failed to delete service: {:?}", e);
            }
        }
    }

    Ok(())
}

pub async fn enable_autostart() -> Result<()> {
    // Service is already set to auto-start during installation
    info!("Service configured for automatic startup");
    Ok(())
}

pub async fn start_service() -> Result<()> {
    let manager = open_service_manager()?;
    let service = open_service(&manager, SERVICE_NAME)?;

    unsafe {
        StartServiceW(service, None)?;
    }

    info!("Service started successfully");
    Ok(())
}

fn open_service_manager() -> Result<SC_HANDLE> {
    let manager = unsafe {
        OpenSCManagerW(
            None,
            PCWSTR(ptr::null()),
            SC_MANAGER_ALL_ACCESS,
        )?
    };

    Ok(manager)
}

fn open_service(manager: &SC_HANDLE, name: &str) -> Result<SC_HANDLE> {
    let service_name = U16CString::from_str(name)?;
    let service = unsafe {
        OpenServiceW(
            *manager,
            PCWSTR(service_name.as_ptr()),
            SERVICE_ALL_ACCESS,
        )?
    };

    Ok(service)
}

// Check if we're running as administrator
pub fn check_privileges() -> Result<()> {
    unsafe {
        let mut token: HANDLE = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut token,
        )?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;

        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )?;

        if elevation.TokenIsElevated == 0 {
            bail!("Installation requires administrator privileges. Please run as administrator.");
        }
    }

    Ok(())
}
