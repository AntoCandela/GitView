//! Executes fixed authenticated GitHub reads; account epochs and demand caching belong to callers.
//! Raw provider bodies stay native and must be normalized before service publication.

mod process;
mod request;
mod response;
mod partial;

use std::ffi::OsString;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use super::model::{Failure, PrCode};
pub(crate) use request::{Connection, GhRead};
pub(crate) use response::ApiResponse;
use response::{classify_auth, classify_version, parse_read_api};
#[cfg(test)]
use response::parse_api;

const STDOUT_LIMIT: usize = 4 * 1024 * 1024;
const STDERR_LIMIT: usize = 1024 * 1024;
const CHILD_DEADLINE: Duration = Duration::from_secs(30);
const MAX_ACTIVE: usize = 2;
const PAGE_SIZE: u32 = 100;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_NODES: usize = 20_000;
const MAX_JSON_COLLECTION: usize = 2_000;

pub(crate) enum GhReply { Version, Authenticated, Api(ApiResponse) }

/// A successful probe identifies an observed account, not continuing authorization.
/// Batch owners must compare observations before/after reads and own their epoch.
pub(crate) struct AccountObservation { pub provider_user_id: String }

#[derive(Clone)]
pub(crate) struct GhReadAdapter {
    executable: OsString,
    permits: Arc<Semaphore>,
    deadline: Duration,
    #[cfg(test)]
    environment: Vec<(OsString, OsString)>,
}

impl Default for GhReadAdapter {
    fn default() -> Self {
        // Separate service instances must not each receive another two-process budget.
        static PERMITS: OnceLock<Arc<Semaphore>> = OnceLock::new();
        Self { executable: "gh".into(), permits: PERMITS.get_or_init(|| Arc::new(Semaphore::new(MAX_ACTIVE))).clone(),
            deadline: CHILD_DEADLINE, #[cfg(test)] environment: Vec::new() }
    }
}

impl GhReadAdapter {
    #[cfg(test)]
    pub(crate) fn fixture(executable: &std::path::Path, environment: Vec<(OsString, OsString)>) -> Self {
        Self { executable: executable.as_os_str().to_owned(), permits: Arc::new(Semaphore::new(MAX_ACTIVE)), deadline: CHILD_DEADLINE, environment }
    }

    /// Resolves `gh` through PATH at each spawn. Dropping a read kills and reaps its child,
    /// retaining the global concurrency and NativeWork permits until cleanup completes.
    pub(crate) async fn read(&self, read: GhRead) -> Result<GhReply, Failure> {
        let command = request::build(&read).map_err(PrCode::failure)?;
        let output = process::run(self, command).await?;
        match read {
            GhRead::ProbeVersion => { classify_version(&output.stdout, output.success).map_err(PrCode::failure)?; Ok(GhReply::Version) }
            GhRead::ProbeAuth => {
                classify_auth(&output.stdout).map_err(PrCode::failure)?;
                if !output.success { return Err(PrCode::AuthUnavailable.failure()); }
                Ok(GhReply::Authenticated)
            }
            _ => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                Ok(GhReply::Api(parse_read_api(&output.stdout, output.success, now, Some(&read))?))
            }
        }
    }

    pub(crate) async fn observe_account(&self) -> Result<AccountObservation, Failure> {
        self.read(GhRead::ProbeVersion).await?;
        self.read(GhRead::ProbeAuth).await?;
        let GhReply::Api(response) = self.read(GhRead::ReadViewer).await? else { return Err(PrCode::InvalidOutput.failure()); };
        let id = response.body.pointer("/data/viewer/id").and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or_else(|| PrCode::InvalidOutput.failure())?;
        Ok(AccountObservation { provider_user_id: id.to_owned() })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/github_transport.rs"]
mod unit_tests;

#[cfg(all(test, unix))]
#[path = "../../tests/integration/github_transport.rs"]
mod integration_tests;
