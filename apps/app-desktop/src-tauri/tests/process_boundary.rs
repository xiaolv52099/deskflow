use anyhow::Result;
use core_protocol::CURRENT_PROTOCOL_VERSION;
use foundation::DATA_ROOT_ENV_VAR;
use local_ipc::{core_service_bin, send_command, CoreToUiEvent, UiToCoreCommand};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn launch_core_service() -> Result<Child> {
    let binary = core_service_bin(&workspace_root());
    if !binary.exists() {
        build_core_service()?;
    }
    let data_root = test_data_root();
    Ok(Command::new(binary)
        .env(DATA_ROOT_ENV_VAR, &data_root)
        .spawn()?)
}

fn build_core_service() -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "-p", "core-service"])
        .current_dir(workspace_root())
        .status()?;

    if !status.success() {
        anyhow::bail!("failed to build core-service before process boundary test");
    }

    Ok(())
}

#[tokio::test]
async fn desktop_shell_can_ping_and_shutdown_core_service() -> Result<()> {
    let mut child = launch_core_service()?;

    let ready = wait_until_ready().await?;
    match ready {
        CoreToUiEvent::Ready {
            protocol_version, ..
        } => {
            assert_eq!(protocol_version, CURRENT_PROTOCOL_VERSION);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    let shutdown = send_command(UiToCoreCommand::Shutdown).await?;
    assert_eq!(shutdown, CoreToUiEvent::ShuttingDown);

    let status = child.wait()?;
    assert!(status.success());

    Ok(())
}

async fn wait_until_ready() -> Result<CoreToUiEvent> {
    for _ in 0..20 {
        match send_command(UiToCoreCommand::Ping).await {
            Ok(event) => return Ok(event),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }

    anyhow::bail!("timed out waiting for core-service readiness")
}

fn test_data_root() -> PathBuf {
    let root = std::env::temp_dir()
        .join("deskflow-plus-app-tests")
        .join("process-boundary");
    if root.exists() {
        let _ = fs::remove_dir_all(&root);
    }
    root
}
