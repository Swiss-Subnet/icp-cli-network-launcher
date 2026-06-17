// End-to-end test of `--subnet=application:rental=<principal>`: spawns the launcher binary with
// the flag and proves a subnet admin can call `canister_metrics` on a canister it does not
// control, while a non-admin non-controller is rejected.
//
// Skipped unless POCKET_IC_BIN points at a pocket-ic server.

use std::fs;
use std::process::Child;
use std::time::{Duration, Instant};

use candid::{CandidType, Encode, Principal};
use pocket_ic::common::rest::RawEffectivePrincipal;
use pocket_ic::{PocketIc, RejectResponse};
use serde::Deserialize;

#[derive(CandidType)]
struct CanisterMetricsArgs {
    canister_id: Principal,
}

#[derive(Deserialize)]
struct Status {
    instance_id: usize,
    config_port: u16,
}

/// Kills the launcher on drop so a failed assertion never leaks the process.
struct LauncherGuard {
    child: Child,
    status_dir: tempfile::TempDir,
}

impl Drop for LauncherGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Spawns the launcher with `--subnet=application:rental=<admins>` and attaches a pocket-ic
/// client to the running instance. Returns `None` (skip) when POCKET_IC_BIN is unset.
fn launch_rental(admins: &[Principal]) -> Option<(PocketIc, LauncherGuard)> {
    let Ok(pocketic_bin) = std::env::var("POCKET_IC_BIN") else {
        eprintln!("skipping: POCKET_IC_BIN not set");
        return None;
    };

    let status_dir = tempfile::tempdir().unwrap();
    let admin_list = admins
        .iter()
        .map(|p| p.to_text())
        .collect::<Vec<_>>()
        .join(",");

    let child = std::process::Command::new(env!("CARGO_BIN_EXE_icp-cli-network-launcher"))
        .arg("--pocketic-server-path")
        .arg(&pocketic_bin)
        .arg("--status-dir")
        .arg(status_dir.path())
        .arg(format!("--subnet=application:rental={admin_list}"))
        .spawn()
        .expect("failed to spawn launcher");

    let mut guard = LauncherGuard { child, status_dir };

    // The launcher writes status.json only once the network is ready.
    let status_file = guard.status_dir.path().join("status.json");
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Ok(contents) = fs::read_to_string(&status_file) {
            if contents.ends_with('\n') {
                break serde_json::from_str::<Status>(&contents).expect("valid status.json");
            }
        }
        if Instant::now() > deadline {
            panic!("launcher did not become ready within 120s");
        }
        if let Ok(Some(exit)) = guard.child.try_wait() {
            panic!("launcher exited prematurely: {exit:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let server_url = format!("http://127.0.0.1:{}/", status.config_port)
        .parse()
        .unwrap();
    let pic = PocketIc::new_from_existing_instance(server_url, status.instance_id, None);
    Some((pic, guard))
}

fn call_canister_metrics(
    pic: &PocketIc,
    sender: Principal,
    target: Principal,
) -> Result<Vec<u8>, RejectResponse> {
    let args = Encode!(&CanisterMetricsArgs {
        canister_id: target,
    })
    .unwrap();
    pic.update_call_with_effective_principal(
        Principal::management_canister(),
        RawEffectivePrincipal::CanisterId(target.as_slice().to_vec()),
        sender,
        "canister_metrics",
        args,
    )
}

#[test]
fn subnet_admin_may_read_canister_metrics() {
    let admin = Principal::from_slice(&[1; 29]);
    let Some((pic, _guard)) = launch_rental(&[admin]) else {
        return;
    };
    let app_subnet = pic.topology().get_app_subnets()[0];
    let canister = pic.create_canister_on_subnet(Some(Principal::anonymous()), None, app_subnet);

    // Admin is not a controller, so the success below is solely from subnet_admins.
    let controllers = pic.get_controllers(canister);
    assert!(
        !controllers.contains(&admin),
        "test invariant broken: admin must not be a controller, controllers: {controllers:?}"
    );

    let result = call_canister_metrics(&pic, admin, canister);
    assert!(
        result.is_ok(),
        "subnet admin should be allowed to call canister_metrics, got: {result:?}"
    );
}

#[test]
fn non_admin_non_controller_is_rejected() {
    let admin = Principal::from_slice(&[1; 29]);
    let stranger = Principal::from_slice(&[2; 29]);
    let Some((pic, _guard)) = launch_rental(&[admin]) else {
        return;
    };
    let app_subnet = pic.topology().get_app_subnets()[0];
    let canister = pic.create_canister_on_subnet(Some(Principal::anonymous()), None, app_subnet);

    let result = call_canister_metrics(&pic, stranger, canister);
    assert!(
        result.is_err(),
        "a non-admin non-controller must be rejected, got: {result:?}"
    );
}
