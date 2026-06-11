use qctrl_rcon::RconClient;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub rcon_client: std::sync::Arc<RconClient>,
}
